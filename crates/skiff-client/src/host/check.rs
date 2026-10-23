//! The check list of one remote Host, read only. One strict, batch ssh run
//! sends the probe script ([`PROBE`]) and answers the first rows: can this
//! computer reach the server, does it know the host key, can it log in
//! without a prompt. The probe then says where skiffd's socket is and
//! whether skiffd answers there. A real `-L` forward and a `ping` over it
//! answer the last rows. Nothing here writes anywhere: the caller stores the
//! socket the probe found.
//!
//! The lines follow the design rules: plain causes, the Host's name, never
//! ssh's own output and never a command to run.

use std::{
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use skiff_host_api::HostId;
use tokio::{io::AsyncWriteExt, process::Command};

use super::{
    local::HostSelection,
    ssh::{SshError, SshOptions, SshTarget},
};

/// The probe: a fixed POSIX `sh` script, sent on stdin. It changes nothing.
pub const PROBE: &str = include_str!("probe.sh");

/// How long the probe's ssh run may take, login included.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// ssh's own connect timeout, inside [`PROBE_TIMEOUT`].
const CONNECT_TIMEOUT: u64 = 15;

/// What the probe printed.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub os: String,
    pub arch: String,
    pub runtime_dir: String,
    /// The socket it looked at: the one asked for, else `<runtime>/skiff/<name>`.
    pub socket: String,
    /// A socket is there.
    pub socket_live: bool,
    /// The skiff CLI it asked, or empty.
    pub cli: String,
    /// `skiff status --json` there, when a CLI and a socket were found.
    pub status: Option<Status>,
}

/// `skiff status --json`, as much as the check reads.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub skiff: String,
    pub skiffd: Option<StatusDaemon>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct StatusDaemon {
    pub version: String,
    pub protocol: u32,
}

/// One row of the check list.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Row {
    Reach,
    HostKey,
    Login,
    Sessions,
    Forwarding,
    Identity,
}

impl Row {
    pub const ALL: [Row; 6] = [Row::Reach, Row::HostKey, Row::Login, Row::Sessions, Row::Forwarding, Row::Identity];

    pub fn title(self) -> &'static str {
        match self {
            Row::Reach => "Reach the server",
            Row::HostKey => "Host key",
            Row::Login => "Log in without a prompt",
            Row::Sessions => "Sessions service (skiffd)",
            Row::Forwarding => "Socket forwarding",
            Row::Identity => "Skiff identity and version",
        }
    }
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Passed,
    Failed,
    Skipped,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub row: Row,
    pub title: &'static str,
    pub state: State,
    pub text: String,
}

/// The whole list. `socket`: the socket the probe found, to store.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub rows: Vec<Line>,
    pub ok: bool,
    pub socket: Option<String>,
    /// The Host id skiffd reported, when it answered.
    pub host_id: Option<String>,
}

/// What a check needs to know about the Host.
pub struct CheckInput {
    /// The login. Its `socket` is ignored: see `socket_override`.
    pub target: SshTarget,
    /// The Host's shown name, for the lines.
    pub name: String,
    /// A socket the person set. None: the probe finds it.
    pub socket_override: Option<String>,
    /// The socket name the probe looks for in the runtime folder.
    pub socket_name: String,
    /// The identity this Host must report, once pinned.
    pub pin: Option<HostId>,
    /// A private folder for the check's own forward.
    pub forward_dir: PathBuf,
}

/// The arguments of the probe's ssh run: strict, batch, no shared master,
/// no extra channels. The remote command is fixed; only the socket name
/// (from this app) and a checked override go to it, single-quoted.
pub fn probe_args(target: &SshTarget, options: &SshOptions, socket_name: &str, socket_override: Option<&str>) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    if let Some(config) = &options.config {
        args.push("-F".into());
        args.push(config.into());
    }
    for value in [
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "UpdateHostKeys=no",
        &format!("ConnectTimeout={CONNECT_TIMEOUT}"),
        "ControlMaster=no",
        "ControlPath=none",
        "ForwardAgent=no",
        "ForwardX11=no",
        "PermitLocalCommand=no",
        "RequestTTY=no",
        "LogLevel=INFO",
    ] {
        args.push("-o".into());
        args.push(value.into());
    }
    args.push("-T".into());
    args.extend(target.login_args());
    args.push("--".into());
    args.push(target.destination.clone().into());
    let mut command = format!("sh -s -- {}", quote(socket_name));
    if let Some(s) = socket_override {
        command.push(' ');
        command.push_str(&quote(s));
    }
    args.push(command.into());
    args
}

/// One shell word in single quotes, for sh, bash, zsh and fish alike.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// What the probe's ssh run gave.
pub enum ProbeRun {
    Probed(Probe, Duration),
    /// ssh stopped before the probe printed: its stderr, kept for the cause.
    Ssh(String),
    TimedOut,
    /// The probe ran but printed no object.
    Garbled,
}

/// Runs the probe over one strict, batch ssh run.
pub async fn run_probe(target: &SshTarget, options: &SshOptions, socket_name: &str, socket_override: Option<&str>) -> Result<ProbeRun, String> {
    let started = Instant::now();
    let mut child = Command::new(&options.program)
        .args(probe_args(target, options, socket_name, socket_override))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Skiff cannot run ssh: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A failed write shows as ssh's own failure below.
        let _ = stdin.write_all(PROBE.as_bytes()).await;
    }
    let out = match tokio::time::timeout(PROBE_TIMEOUT, child.wait_with_output()).await {
        Ok(out) => out.map_err(|e| format!("Skiff cannot run ssh: {e}"))?,
        Err(_) => return Ok(ProbeRun::TimedOut),
    };
    if out.status.code() == Some(255) {
        return Ok(ProbeRun::Ssh(String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(match text.lines().rev().find_map(|l| serde_json::from_str::<Probe>(l.trim()).ok()) {
        Some(p) => ProbeRun::Probed(p, started.elapsed()),
        None => ProbeRun::Garbled,
    })
}

/// A socket override that may go to the probe and to `-L`.
pub fn valid_socket(s: &str) -> bool {
    s.starts_with('/') && !s.contains(':') && !s.chars().any(|c| c.is_control())
}

/// Runs the check list. Rows after the first failure are skipped.
pub async fn check(input: &CheckInput, options: &SshOptions) -> Report {
    let mut list = List::default();
    let name = input.name.as_str();
    let server = input.target.destination.rsplit('@').next().unwrap_or(&input.target.destination).to_string();
    let over = input.socket_override.as_deref().filter(|s| valid_socket(s));
    let probe = match run_probe(&input.target, options, &input.socket_name, over).await {
        Err(e) => {
            list.fail(Row::Reach, e);
            return list.finish(None, None);
        }
        Ok(ProbeRun::TimedOut) => {
            list.fail(Row::Reach, format!("{server} did not answer within {CONNECT_TIMEOUT} s. Check again, or check the network."));
            return list.finish(None, None);
        }
        Ok(ProbeRun::Ssh(stderr)) => {
            login_failure(&mut list, &stderr, name, &server);
            return list.finish(None, None);
        }
        Ok(ProbeRun::Garbled) => {
            list.pass(Row::Reach, format!("{server} answers."));
            list.pass(Row::HostKey, "Trusted.".into());
            list.pass(Row::Login, "Logged in with your key.".into());
            list.fail(Row::Sessions, format!("Skiff cannot read the state of {name}. Its login shell printed something Skiff does not know."));
            return list.finish(None, None);
        }
        Ok(ProbeRun::Probed(p, took)) => {
            list.pass(Row::Reach, format!("{server} answers. {} ms.", took.as_millis()));
            list.pass(Row::HostKey, "Trusted.".into());
            list.pass(Row::Login, "Logged in with your key. No password needed.".into());
            p
        }
    };
    let socket = probe.socket.clone();
    let found = (over.is_none() && probe.socket_live).then(|| socket.clone());
    let daemon = probe.status.as_ref().map(|s| s.skiffd.as_ref());
    match (probe.socket_live, daemon) {
        (false, _) | (true, Some(None)) => {
            list.fail(Row::Sessions, format!("Skiff's sessions service is not running on {name}."));
            return list.finish(found, None);
        }
        (true, Some(Some(d))) => list.pass(Row::Sessions, format!("skiffd {} answers.", d.version)),
        (true, None) => list.pass(Row::Sessions, "Its socket is there.".into()),
    }

    // The real forward and a ping over it, with no pin: this check stores nothing.
    let target = SshTarget { socket: socket.clone(), ..input.target.clone() };
    let selection = match HostSelection::ssh(target, options.clone(), input.forward_dir.clone(), None) {
        Ok(s) => s,
        Err(e) => {
            list.fail(Row::Forwarding, e);
            return list.finish(found, None);
        }
    };
    let info = async {
        let mut c = selection.connect().await?;
        tokio::time::timeout(Duration::from_secs(15), c.daemon_info()).await.map_err(|_| anyhow::anyhow!("no answer"))?
    };
    let info = match info.await {
        Ok(info) => info,
        Err(e) => {
            // The probe saw a live socket, so a refused channel is forwarding.
            let text = match e.downcast_ref::<SshError>() {
                Some(SshError::NoSkiffd { .. } | SshError::ForwardingRefused(_)) => forwarding_off(name),
                _ => format!("Skiff's sessions service on {name} did not answer through ssh."),
            };
            list.fail(Row::Forwarding, text);
            return list.finish(found, None);
        }
    };
    list.pass(Row::Forwarding, "Skiff reaches the socket through ssh.".into());
    let Some(identity) = info.identity else {
        list.fail(Row::Identity, format!("Skiff on {name} is too old to say who it is."));
        return list.finish(found, None);
    };
    let host_id = identity.host_id.clone();
    if input.pin.as_ref().is_some_and(|p| p.as_str() != host_id) {
        list.fail(
            Row::Identity,
            format!("{name} answers as another Skiff install than before. Skiff stopped. Accept only if you know why, such as a reinstall of that server."),
        );
        return list.finish(found, Some(host_id));
    }
    let here = skiff_core::PROTOCOL;
    match info.protocol {
        p if p < here => list.fail(
            Row::Identity,
            format!("Skiff on {name} speaks an older version. You can see and type in its sessions. To start and end sessions here, update it."),
        ),
        p if p > here => list.fail(Row::Identity, format!("Skiff on {name} speaks a newer version than this computer. Update Skiff on this computer.")),
        _ => {
            let same = if input.pin.is_some() { " Same Skiff install as before." } else { "" };
            list.pass(Row::Identity, format!("skiffd {}, protocol {}.{same}", info.version, info.protocol))
        }
    }
    list.finish(found, Some(host_id))
}

fn forwarding_off(name: &str) -> String {
    format!("{name} does not allow Skiff's connection to its sessions service. Its admin must allow socket forwarding for ssh (AllowStreamLocalForwarding).")
}

/// The first rows, from what ssh said when it stopped.
fn login_failure(list: &mut List, stderr: &str, name: &str, server: &str) {
    let has = |m: &str| stderr.contains(m);
    if has("Could not resolve hostname") || has("nodename nor servname") || has("Name or service not known") {
        return list.fail(Row::Reach, format!("This computer cannot find {server}. Check the name, or pick an alias from ssh config."));
    }
    if has("Connection timed out") || has("Operation timed out") {
        return list.fail(Row::Reach, format!("{server} did not answer within {CONNECT_TIMEOUT} s. Check again, or check the network."));
    }
    if ["Connection refused", "No route to host", "Network is unreachable", "Connection closed by", "Connection reset"].iter().any(|m| has(m)) && !has("Permission denied") {
        return list.fail(Row::Reach, format!("{server} does not answer. Check that this computer is online and that the server is on."));
    }
    list.pass(Row::Reach, format!("{server} answers."));
    let target = SshTarget::new(name, "/");
    match super::ssh::classify(stderr, &target) {
        Some(SshError::HostKeyChanged(_)) => {
            return list.fail(Row::HostKey, format!("The host key of {name} changed since the last connection. Skiff does not connect."));
        }
        Some(SshError::HostKeyUnknown(_)) => {
            return list.fail(Row::HostKey, format!("This computer does not know the host key of {name} yet."));
        }
        _ => {}
    }
    list.pass(Row::HostKey, "Trusted.".into());
    if has("Permission denied") || has("Too many authentication failures") {
        // `Permission denied (publickey).`: the server allows keys only.
        let methods = stderr.split("Permission denied (").nth(1).and_then(|r| r.split(')').next()).unwrap_or_default();
        let text = if methods.split(',').all(|m| m.trim() == "publickey") && !methods.is_empty() {
            format!("{name} accepts only keys it already knows, and allows no password. Ask its admin to add this computer's key.")
        } else {
            format!("{name} does not accept a key from this computer yet.")
        };
        return list.fail(Row::Login, text);
    }
    list.fail(Row::Login, format!("ssh to {name} stopped for an unknown reason."));
}

#[derive(Default)]
struct List {
    rows: Vec<Line>,
}

impl List {
    fn pass(&mut self, row: Row, text: String) {
        self.rows.push(Line { row, title: row.title(), state: State::Passed, text });
    }
    fn fail(&mut self, row: Row, text: String) {
        self.rows.push(Line { row, title: row.title(), state: State::Failed, text });
    }
    fn finish(mut self, socket: Option<String>, host_id: Option<String>) -> Report {
        let ok = self.rows.len() == Row::ALL.len() && self.rows.iter().all(|r| r.state == State::Passed);
        for row in Row::ALL.into_iter().skip(self.rows.len()) {
            self.rows.push(Line { row, title: row.title(), state: State::Skipped, text: "Skipped until the row above passes.".into() });
        }
        Report { rows: self.rows, ok, socket, host_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(stderr: &str) -> Vec<(Row, State, String)> {
        let mut list = List::default();
        login_failure(&mut list, stderr, "devbox", "devbox.lan");
        list.finish(None, None).rows.into_iter().map(|l| (l.row, l.state, l.text)).collect()
    }

    fn failed(stderr: &str) -> (Row, String) {
        let rows = lines(stderr);
        let f = rows.iter().find(|r| r.1 == State::Failed).expect("a failed row");
        // Every row after it is skipped, and nothing repeats ssh's output.
        let at = rows.iter().position(|r| r.1 == State::Failed).unwrap();
        assert!(rows[at + 1..].iter().all(|r| r.1 == State::Skipped), "{rows:?}");
        assert!(!f.2.contains("ssh:") && !f.2.contains(".ssh/"), "{}", f.2);
        (f.0, f.2.clone())
    }

    #[test]
    fn each_ssh_failure_names_one_row_and_one_plain_cause() {
        let cases = [
            ("ssh: Could not resolve hostname devbox.lann: Name or service not known\n", Row::Reach, "cannot find devbox.lan"),
            ("ssh: connect to host 10.0.4.21 port 22: Connection timed out\n", Row::Reach, "within 15 s"),
            ("ssh: connect to host 10.0.4.21 port 22: No route to host\n", Row::Reach, "does not answer"),
            ("@@@\n@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @\nHost key verification failed.\n", Row::HostKey, "changed since"),
            ("No ED25519 host key is known for devbox.lan and you have requested strict checking.\nHost key verification failed.\n", Row::HostKey, "does not know the host key"),
            ("me@devbox.lan: Permission denied (publickey).\n", Row::Login, "accepts only keys"),
            ("me@devbox.lan: Permission denied (publickey,password).\n", Row::Login, "does not accept a key"),
            ("kex_exchange_identification: read: Connection reset by peer\n", Row::Reach, "does not answer"),
            ("something new\n", Row::Login, "unknown reason"),
        ];
        for (stderr, row, words) in cases {
            let (got, text) = failed(stderr);
            assert_eq!(got, row, "{stderr}");
            assert!(text.contains(words), "{stderr}: {text}");
        }
    }

    #[test]
    fn the_probe_command_is_fixed_and_quotes_its_values() {
        let t = SshTarget { user: Some("me".into()), port: Some(2222), ..SshTarget::new("devbox", "") };
        let args: Vec<String> = probe_args(&t, &SshOptions { program: "ssh".into(), config: None }, "skiffd.sock", Some("/run/a b/it's.sock"))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        for want in ["BatchMode=yes", "StrictHostKeyChecking=yes", "ControlPath=none", "ForwardAgent=no"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        assert!(!args.iter().any(|a| a.contains("accept-new") || a.contains("StrictHostKeyChecking=no")));
        let n = args.len();
        assert_eq!(&args[n - 7..], ["-l", "me", "-p", "2222", "--", "devbox", r"sh -s -- 'skiffd.sock' '/run/a b/it'\''s.sock'"]);
    }

    /// The probe as the server runs it: here, with `sh`, no ssh.
    #[test]
    fn the_probe_prints_one_object_and_finds_the_socket() {
        let dir = std::env::temp_dir().join(format!("skiff-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("skiff")).unwrap();
        let sock = dir.join("skiff/skiffd-x.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let run = |args: &[&str]| {
            let mut child = std::process::Command::new("sh")
                .args(["-s", "--"])
                .args(args)
                .env("XDG_RUNTIME_DIR", &dir)
                .env("HOME", &dir)
                .env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            std::io::Write::write_all(&mut child.stdin.take().unwrap(), PROBE.as_bytes()).unwrap();
            let out = child.wait_with_output().unwrap();
            serde_json::from_slice::<Probe>(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
        };
        let p = run(&["skiffd-x.sock"]);
        assert_eq!(p.runtime_dir, dir.to_string_lossy());
        assert_eq!(p.socket, sock.to_string_lossy());
        assert!(p.socket_live && p.status.is_none(), "{p:?}");
        assert!(!p.os.is_empty() && !p.arch.is_empty());
        let none = run(&["skiffd.sock"]);
        assert!(!none.socket_live);
        let chosen = run(&["skiffd.sock", &sock.to_string_lossy()]);
        assert!(chosen.socket_live && chosen.socket == sock.to_string_lossy());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

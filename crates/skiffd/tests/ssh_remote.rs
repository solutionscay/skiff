//! M2 over a real SSH stream-local forward: a private skiffd, a private
//! loopback sshd with throwaway keys and known_hosts, and the system `ssh`
//! with a private config (`-F`). Nothing reads or writes the user's ~/.ssh,
//! and no other host is contacted. Skips when this machine has no usable sshd.
#![cfg(unix)]

use std::{
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use skiff_client::{
    host::{
        local::{
            legacy_agents, legacy_attach, legacy_create, legacy_delete_group, legacy_groups, legacy_kill, legacy_output,
            legacy_rename, legacy_save_group, HostSelection, IdentitySource, SelectedHost,
        },
        ssh::{SshError, SshOptions, SshTarget},
        Continuity, OutputGap, TerminalFrame,
    },
    Client,
};
use skiff_core::session::SessionSpec;
use skiff_host_api::Capability;

mod common;

const SSHD: &str = "/usr/sbin/sshd";
const SSH: &str = "/usr/bin/ssh";

struct Fixture {
    dir: PathBuf,
    daemon: Child,
    sshd: Option<Child>,
    port: u16,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop_sshd();
        // Our own `ssh` clients, by their private config path only.
        let _ = Command::new("pkill").arg("-f").arg(self.dir.join("ssh_config")).status();
        let stopped = common::stop(&mut self.daemon, &self.dir);
        if std::thread::panicking() {
            for log in ["daemon.log", "sshd.log"] {
                eprintln!("--- {log}\n{}", std::fs::read_to_string(self.dir.join(log)).unwrap_or_default());
            }
        }
        if stopped {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

impl Fixture {
    fn socket(&self) -> PathBuf {
        self.dir.join("skiffd.sock")
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.dir.join(name), text).unwrap();
    }

    fn sshd_config(&self, name: &str, forwarding: bool) {
        let d = self.dir.display();
        self.write(name, &format!(
            "Port {}\nListenAddress 127.0.0.1\nHostKey {d}/host_key\nAuthorizedKeysFile {d}/authorized_keys\n\
             PidFile {d}/sshd.pid\nStrictModes no\nUsePAM no\nPasswordAuthentication no\n\
             KbdInteractiveAuthentication no\nAllowStreamLocalForwarding {}\nLogLevel VERBOSE\n",
            self.port,
            if forwarding { "yes" } else { "no" }
        ));
    }

    /// Starts sshd; false when this machine cannot run one unprivileged.
    fn start_sshd(&mut self, config: &str) -> bool {
        self.stop_sshd();
        let mut child = Command::new(SSHD)
            .arg("-D")
            .arg("-f")
            .arg(self.dir.join(config))
            .arg("-E")
            .arg(self.dir.join("sshd.log"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                self.sshd = Some(child);
                return true;
            }
            if child.try_wait().unwrap().is_some() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
        false
    }

    /// Stops the listener and every connection it serves: its process group.
    fn stop_sshd(&mut self) {
        if let Some(mut child) = self.sshd.take() {
            // SAFETY: a plain signal to this test's own sshd process group.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM) };
            let _ = child.wait();
        }
    }

    fn options(&self, config: &str) -> SshOptions {
        SshOptions { program: SSH.into(), config: Some(self.dir.join(config)) }
    }

    fn selection(&self, config: &str, socket: &Path, forward: &str) -> HostSelection {
        let target = SshTarget::new("fixture", socket.to_string_lossy().into_owned());
        HostSelection::ssh(target, self.options(config), self.dir.join(forward), None).unwrap()
    }
}

fn keygen(path: &Path) {
    let ok = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", ""]).arg("-f").arg(path).status().unwrap();
    assert!(ok.success());
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn fixture(name: &str) -> Option<Fixture> {
    fixture_with(name, false)
}

fn fixture_with(name: &str, no_identity: bool) -> Option<Fixture> {
    if !Path::new(SSHD).is_file() || !Path::new(SSH).is_file() {
        eprintln!("skipped: no {SSHD} or {SSH}");
        return None;
    }
    // Short: the forward's socket path must fit sun_path.
    let dir = std::env::temp_dir().join(format!("skssh-{name}{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    if no_identity {
        // A store skiffd refuses to read: its pong then reports no identity,
        // as an older skiffd's does.
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/local-host-id"), "not an id").unwrap();
        std::fs::set_permissions(dir.join("config/local-host-id"), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let log = std::fs::File::create(dir.join("daemon.log")).unwrap();
    let daemon = Command::new(env!("CARGO_BIN_EXE_skiffd"))
        .env("SKIFF_SOCKET", dir.join("skiffd.sock"))
        .env("SKIFF_STATE", dir.join("workspace.json"))
        .env("SKIFF_CONFIG", dir.join("config/projects.toml"))
        .env("SKIFF_LEDGER", dir.join("ledger.db"))
        .env("SKIFF_SHELL_ENV", "0")
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut f = Fixture { dir, daemon, sshd: None, port: free_port() };
    keygen(&f.dir.join("host_key"));
    keygen(&f.dir.join("other_host_key"));
    keygen(&f.dir.join("client_key"));
    std::fs::copy(f.dir.join("client_key.pub"), f.dir.join("authorized_keys")).unwrap();
    let key = |name: &str| {
        let text = std::fs::read_to_string(f.dir.join(name)).unwrap();
        text.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
    };
    let known = format!("[127.0.0.1]:{} {}\n", f.port, key("host_key.pub"));
    let changed = format!("[127.0.0.1]:{} {}\n", f.port, key("other_host_key.pub"));
    f.write("known_hosts", &known);
    f.write("changed_hosts", &changed);
    f.write("empty_hosts", "");
    for (config, hosts) in [("ssh_config", "known_hosts"), ("unknown_config", "empty_hosts"), ("changed_config", "changed_hosts")] {
        let d = f.dir.display();
        f.write(config, &format!(
            "Host fixture\n  HostName 127.0.0.1\n  Port {}\n  IdentityFile {d}/client_key\n  IdentitiesOnly yes\n  \
             UserKnownHostsFile {d}/{hosts}\n  GlobalKnownHostsFile /dev/null\n",
            f.port
        ));
    }
    f.sshd_config("sshd_config", true);
    f.sshd_config("sshd_noforward", false);
    if !f.start_sshd("sshd_config") {
        eprintln!("skipped: {SSHD} does not run unprivileged here");
        return None;
    }
    Some(f)
}

/// #166 slice 6, over the loopback sshd with a temp home and runtime
/// folder. A managed install moves up with the reload handover: the daemon
/// and the session keep their pids, and the screen keeps its text. It never
/// moves down. A new skiffd that cannot adopt the sessions changes nothing:
/// `current` goes back, and the live sessions are named.
#[tokio::test]
async fn an_update_keeps_the_session_pid_and_screen_and_never_goes_down() {
    use skiff_client::host::install::{self, Updated};
    use std::os::unix::fs::PermissionsExt;
    use std::io::Write;
    let skiffd = Path::new(env!("CARGO_BIN_EXE_skiffd"));
    let skiff = skiffd.with_file_name("skiff");
    if !skiff.exists() {
        eprintln!("skipped: build the skiff CLI first (cargo build -p skiff-cli)");
        return;
    }
    let Some(mut f) = fixture("p") else { return };
    let d = f.dir.clone();
    for sub in ["home", "run", "beside", "broken"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    std::fs::set_permissions(d.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let text = std::fs::read_to_string(d.join("sshd_config")).unwrap();
    f.write("sshd_update", &format!("{text}SetEnv HOME={0}/home XDG_RUNTIME_DIR={0}/run\n", d.display()));
    assert!(f.start_sshd("sshd_update"));
    for dir in ["beside", "broken"] {
        std::fs::copy(&skiff, d.join(dir).join("skiff")).unwrap();
        std::fs::write(d.join(dir).join("skiff-host"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(d.join(dir).join("skiff-host"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::copy(skiffd, d.join("beside/skiffd")).unwrap();
    // A skiffd that cannot adopt sessions: it does not answer --reload-info.
    std::fs::write(d.join("broken/skiffd"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(d.join("broken/skiffd"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let arch = std::env::consts::ARCH;
    let bundle = |v: &'static str, dir: &'static str| {
        let beside = d.join(dir);
        async move { install::dev_bundle(v, arch, &beside).await.unwrap() }
    };
    // The folder each bundle unpacks to: its version and its build.
    let key = |v: &str, dir: &str| PathBuf::from(format!("{v}+{}", install::dev_build(&d.join(dir)).unwrap()));
    let target = SshTarget::new("fixture", "");
    let options = f.options("ssh_config");
    let name = "skiffd-dev.sock";
    let server = d.join("home/.local/share/skiff/server");
    let pidfile = server.join("skiffd.pid");
    struct Stop(PathBuf);
    impl Drop for Stop {
        fn drop(&mut self) {
            if let Ok(pid) = std::fs::read_to_string(&self.0).map(|p| p.trim().parse::<i32>().unwrap_or(0)) {
                if pid > 0 {
                    // SAFETY: a signal to the one process this test started.
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                }
            }
        }
    }
    // Each step shows as it happens, in order.
    use install::Progress;
    let lines = std::sync::Mutex::new(Vec::<Progress>::new());
    let step = |p: Progress| lines.lock().unwrap().push(p);
    let first = install::install(&target, &options, &bundle("9.9.8-test", "beside").await, "9.9.8-test", name, 0, "fixture", &step).await.unwrap();
    let _stop = Stop(pidfile.clone());
    assert!(first.running, "{first:?}");
    let seen = lines.lock().unwrap().clone();
    let text = |p: &Progress| match p {
        Progress::Step { n, text } => format!("#{n} {text}"),
        Progress::Line { text } => text.clone(),
        Progress::Sent { sent, total } => format!("sent {sent}/{total}"),
    };
    let all: Vec<String> = seen.iter().map(text).collect();
    let at = |w: &str| all.iter().position(|l| l.starts_with(w)).unwrap_or_else(|| panic!("{w} in {all:?}"));
    assert!(at("#3 Upload") < at("#4 Unpacking Skiff 9.9.8-test") && at("#4") < at("#5 Switching") && at("#5") < at("#6 Starting skiffd") && at("#6") < at("skiffd runs"), "{all:?}");
    // The upload counts its bytes up to the whole; ssh's own lines pass the
    // allowlist only, and never name a key file.
    let total = match seen.iter().rev().find(|p| matches!(p, Progress::Sent { .. })) {
        Some(Progress::Sent { sent, total }) => (*sent == *total).then_some(*total),
        _ => None,
    };
    assert!(total.is_some_and(|t| t > 0), "{all:?}");
    assert!(all.iter().any(|l| l.starts_with("Authenticated to ")), "{all:?}");
    assert!(!all.iter().any(|l| l.contains("/.ssh/") || l.starts_with("debug")), "{all:?}");
    // The managed version is the bundle's, never skiffd's crate version.
    let probed = |run| match run {
        skiff_client::host::check::ProbeRun::Probed(p, _) => p,
        _ => panic!("the probe did not run"),
    };
    let p = probed(skiff_client::host::check::run_probe(&target, &options, name, None).await.unwrap());
    assert_eq!(p.managed_version(), Some("9.9.8-test"), "{p:?}");

    // A session with text on its screen, straight on the fixture's socket.
    let socket = d.join("run/skiff").join(name);
    let c = local_client(&socket).await;
    let daemon_pid = c.daemon_info().await.unwrap().pid;
    let s = c
        .create_session(SessionSpec { command: Some("/bin/sh".into()), cwd: Some(d.clone()), cols: 60, rows: 8, name: Some("build".into()), ..Default::default() })
        .await
        .unwrap();
    let mut out = c.output(&s.id);
    c.subscribe(&s.id).await.unwrap();
    c.write(&s.id, b"stty -echo; PS1=''; echo before-$((40+2))\r".to_vec()).await.unwrap();
    seen_local(&mut out, "before-42").await;
    drop(out);
    drop(c);

    // Up: the reload keeps the daemon, the session and its screen.
    let up = install::update(&target, &options, &bundle("9.9.9-test", "beside").await, "9.9.9-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(up, Updated::Done { from: "9.9.8-test".into(), restarted: false });
    assert_eq!(std::fs::read_link(server.join("current")).unwrap(), key("9.9.9-test", "beside"));
    assert!(server.join(key("9.9.8-test", "beside")).join("skiffd").exists(), "the version before stays, for a rollback");
    let c = local_client(&socket).await;
    assert_eq!(c.daemon_info().await.unwrap().pid, daemon_pid, "skiffd kept its pid");
    let after = c.list_sessions().await.unwrap().into_iter().find(|x| x.id == s.id).expect("the session survived");
    assert_eq!(after.pid, s.pid, "the session kept its pid");
    let mut out = c.output(&s.id);
    c.subscribe(&s.id).await.unwrap();
    seen_local(&mut out, "before-42").await;
    c.write(&s.id, b"echo after-$((40+3))\r".to_vec()).await.unwrap();
    seen_local(&mut out, "after-43").await;

    // Never down: an older bundle changes nothing.
    let down = install::update(&target, &options, &bundle("9.9.7-test", "beside").await, "9.9.7-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(down, Updated::Newer { from: "9.9.9-test".into() });
    assert_eq!(std::fs::read_link(server.join("current")).unwrap(), key("9.9.9-test", "beside"));

    // An update that stopped after it moved `current` (here: an install
    // over the running version, which does no reload) is finished by the
    // next update: the skiffd that runs is the old one, and it reloads.
    let moved = install::install(&target, &options, &bundle("9.9.11-test", "beside").await, "9.9.11-test", name, 0, "fixture", &|_| {}).await.unwrap();
    assert!(moved.changed, "{moved:?}");
    assert_eq!(std::fs::read_link(server.join("current")).unwrap(), key("9.9.11-test", "beside"));
    let exe = || std::fs::read_link(format!("/proc/{}/exe", daemon_pid.unwrap())).unwrap();
    assert!(exe().ends_with(key("9.9.9-test", "beside").join("skiffd")), "{:?}", exe());
    let p = probed(skiff_client::host::check::run_probe(&target, &options, name, None).await.unwrap());
    assert_eq!(p.managed_version(), Some("9.9.9-test"), "the bundle that runs, not `current`: {p:?}");
    let finished = install::update(&target, &options, &bundle("9.9.11-test", "beside").await, "9.9.11-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(finished, Updated::Done { from: "9.9.9-test".into(), restarted: false });
    assert!(exe().ends_with(key("9.9.11-test", "beside").join("skiffd")), "{:?}", exe());
    // The reload closed the old connection, as every reload does.
    let c = local_client(&socket).await;
    assert_eq!(c.daemon_info().await.unwrap().pid, daemon_pid, "skiffd kept its pid");
    let mut out = c.output(&s.id);
    c.subscribe(&s.id).await.unwrap();
    c.write(&s.id, b"echo again-$((40+5))\r".to_vec()).await.unwrap();
    seen_local(&mut out, "again-45").await;
    // The same version again, with the skiffd that runs it: nothing changes.
    let same = install::update(&target, &options, &bundle("9.9.11-test", "beside").await, "9.9.11-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(same, Updated::Same);

    // The same version in another build (a dev app with new binaries):
    // build B goes in beside build A, and skiffd reloads onto it.
    std::fs::create_dir_all(d.join("rebuilt")).unwrap();
    for b in ["skiffd", "skiff", "skiff-host"] {
        std::fs::copy(d.join("beside").join(b), d.join("rebuilt").join(b)).unwrap();
    }
    // Bytes after the ELF image: another hash, the same program.
    std::fs::OpenOptions::new().append(true).open(d.join("rebuilt/skiffd")).unwrap().write_all(b"build-b").unwrap();
    assert_ne!(key("9.9.11-test", "rebuilt"), key("9.9.11-test", "beside"));
    let p = probed(skiff_client::host::check::run_probe(&target, &options, name, None).await.unwrap());
    let a = install::dev_build(&d.join("beside")).unwrap();
    assert_eq!(p.managed_build(), Some(a.as_str()), "{p:?}");
    assert!(!install::same_build(Some(&install::dev_build(&d.join("rebuilt")).unwrap()), &a));
    let rebuilt = install::update(&target, &options, &bundle("9.9.11-test", "rebuilt").await, "9.9.11-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(rebuilt, Updated::Done { from: "9.9.11-test".into(), restarted: false });
    assert_eq!(std::fs::read_link(server.join("current")).unwrap(), key("9.9.11-test", "rebuilt"));
    assert!(exe().ends_with(key("9.9.11-test", "rebuilt").join("skiffd")), "{:?}", exe());
    assert!(server.join(key("9.9.11-test", "beside")).join("skiffd").exists(), "build A stays, for a rollback");
    let c = local_client(&socket).await;
    assert_eq!(c.daemon_info().await.unwrap().pid, daemon_pid, "skiffd kept its pid");
    let mut out = c.output(&s.id);
    c.subscribe(&s.id).await.unwrap();
    c.write(&s.id, b"echo rebuilt-$((40+6))\r".to_vec()).await.unwrap();
    seen_local(&mut out, "rebuilt-46").await;
    // Build B again: Same. A reinstall of it reloads all the same.
    let again = install::update(&target, &options, &bundle("9.9.11-test", "rebuilt").await, "9.9.11-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(again, Updated::Same);
    let forced = install::update(&target, &options, &bundle("9.9.11-test", "rebuilt").await, "9.9.11-test", name, 0, "fixture", false, true, &|_| {}).await.unwrap();
    assert_eq!(forced, Updated::Done { from: "9.9.11-test".into(), restarted: false });
    let c = local_client(&socket).await;
    assert_eq!(c.daemon_info().await.unwrap().pid, daemon_pid, "skiffd kept its pid");
    let mut out = c.output(&s.id);
    c.subscribe(&s.id).await.unwrap();
    c.write(&s.id, b"echo forced-$((40+7))\r".to_vec()).await.unwrap();
    seen_local(&mut out, "forced-47").await;

    // No handover: nothing changes, and the update names the session.
    let stuck = install::update(&target, &options, &bundle("9.9.12-test", "broken").await, "9.9.12-test", name, 0, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(stuck, Updated::NoHandover { from: "9.9.11-test".into(), sessions: vec!["build".into()] });
    assert_eq!(std::fs::read_link(server.join("current")).unwrap(), key("9.9.11-test", "rebuilt"), "current went back");
    assert_eq!(c.daemon_info().await.unwrap().pid, daemon_pid);
    c.write(&s.id, b"echo still-$((40+4))\r".to_vec()).await.unwrap();
    seen_local(&mut out, "still-44").await;
    assert!(!server.join(".lock").exists(), "the install lock is gone");
}

/// Waits for `needle` in a session's output on a direct client.
async fn seen_local(out: &mut skiff_client::Output, needle: &str) {
    use skiff_core::protocol::Event;
    let mut seen = String::new();
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    while !seen.contains(needle) {
        match tokio::time::timeout_at(until, out.recv()).await {
            Ok(Ok(Event::Output { data, .. } | Event::Snapshot { data, .. })) => seen.push_str(&String::from_utf8_lossy(&data)),
            Ok(Ok(_)) => {}
            Ok(Err(e)) => panic!("output ended ({e}) before {needle:?} in {seen:?}"),
            Err(_) => panic!("no {needle:?} in {seen:?}"),
        }
    }
}

async fn local_client(socket: &Path) -> Client {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(c) = Client::connect(socket).await {
            if c.daemon_info().await.is_ok() {
                return c;
            }
        }
        assert!(Instant::now() < until, "skiffd did not answer");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn select(selection: &HostSelection) -> anyhow::Result<SelectedHost> {
    let mut pending = selection.connect().await?;
    pending.daemon_info().await?;
    pending.activate().await
}

async fn until_seen(output: &mut skiff_client::host::TerminalOutput, needle: &str) {
    let mut seen = String::new();
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    while !seen.contains(needle) {
        match tokio::time::timeout_at(until, output.recv()).await {
            Ok(Ok(TerminalFrame::Snapshot { data, .. } | TerminalFrame::Output { data, .. })) => {
                seen.push_str(&String::from_utf8_lossy(&data))
            }
            Ok(Err(e)) => panic!("output ended ({e:?}) before {needle:?} in {seen:?}"),
            Err(_) => panic!("no {needle:?} in {seen:?}"),
        }
    }
}

fn ssh_error(e: &anyhow::Error) -> Option<&SshError> {
    e.downcast_ref::<SshError>()
}

#[tokio::test]
async fn a_remote_terminal_over_a_real_forward_survives_a_dropped_link() {
    let Some(mut f) = fixture("a") else { return };
    // The remote CLI starts the work, as M2 expects.
    let host_side = local_client(&f.socket()).await;
    let shell = host_side
        .create_session(SessionSpec { label: "remote".into(), command: Some("/bin/sh".into()), cols: 80, rows: 24, ..Default::default() })
        .await
        .unwrap();

    let selection = f.selection("ssh_config", &f.socket(), "fwd");
    assert!(!selection.is_local());
    assert!(selection.process_control().is_none(), "a remote endpoint is never managed");
    let host = select(&selection).await.unwrap();
    assert_eq!(host.identity_source(), IdentitySource::Host);
    let hello = host.connection().hello().clone();
    assert_eq!(host.connection().continuity(), Continuity::FirstConnection);
    // The same protocol: terminals, and M3's lifecycle, groups and agents.
    for cap in [Capability::session_terminal(), Capability::session_lifecycle(), Capability::session_groups(), Capability::session_agents()] {
        assert!(hello.capabilities.contains(&cap), "{cap:?}");
    }
    // The forward's peer is `ssh`: its pid is never the daemon's.
    assert_ne!(host.client().daemon_pid.map(|p| p as u32), f.daemon.id().into());

    let conn = host.connection();
    let listed = skiff_client::host::local::legacy_inventory(&conn).await.unwrap();
    assert!(listed.iter().any(|s| s.id == shell.id));
    let mut output = legacy_output(&conn, &shell.id).unwrap();
    legacy_attach(&conn, &shell.id).await.unwrap();
    skiff_client::host::local::legacy_input(&conn, &shell.id, b"echo M2-$((40+2))\n".to_vec()).unwrap();
    until_seen(&mut output, "M2-42").await;

    // The link drops: ssh ends. The stream closes and input is refused.
    f.stop_sshd();
    let _ = Command::new("pkill").arg("-f").arg(f.dir.join("ssh_config")).status();
    let until = Instant::now() + Duration::from_secs(10);
    while !conn.is_closed() {
        assert!(Instant::now() < until, "the dropped link was not noticed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(output.recv().await.err(), Some(OutputGap::Closed));
    assert!(skiff_client::host::local::legacy_input(&conn, &shell.id, b"echo LOST\n".to_vec()).is_err());
    // The remote session runs on, the same child.
    let same = host_side.list_sessions().await.unwrap().into_iter().find(|s| s.id == shell.id).unwrap();
    assert_eq!(same.pid, shell.pid);
    drop(conn);
    drop(host);

    // While the link is down, connecting fails and waits before ssh runs again.
    let down = select(&selection).await.err().unwrap();
    assert!(matches!(ssh_error(&down), Some(SshError::Unreachable(_) | SshError::Exited { .. })), "{down:#}");
    let waiting = select(&selection).await.err().unwrap();
    assert!(matches!(ssh_error(&waiting), Some(SshError::Waiting { .. })), "{waiting:#}");

    // Back up: the same skiffd run answers, with a fresh subscription and snapshot.
    assert!(f.start_sshd("sshd_config"));
    let until = Instant::now() + Duration::from_secs(10);
    let again = loop {
        match select(&selection).await {
            Ok(host) => break host,
            Err(e) => {
                assert!(Instant::now() < until, "no reconnect: {e:#}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    };
    assert_eq!(again.connection().continuity(), Continuity::SameInstance);
    assert_eq!(again.connection().hello().host.host_id, hello.host.host_id);
    let conn = again.connection();
    let mut output = legacy_output(&conn, &shell.id).unwrap();
    legacy_attach(&conn, &shell.id).await.unwrap();
    // The snapshot redraws what ran before; LOST was never sent.
    until_seen(&mut output, "M2-42").await;
    skiff_client::host::local::legacy_input(&conn, &shell.id, b"echo BACK-$((1+1))\n".to_vec()).unwrap();
    until_seen(&mut output, "BACK-2").await;
    let screen = host_side.screen(&shell.id, 24).await.unwrap().join("\n");
    assert!(!screen.contains("LOST"), "{screen}");

    // The app quits: its forward goes, and the remote session stays.
    drop(conn);
    drop(again);
    drop(selection);
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Each forward's own directory goes; the shared private parent stays.
    let left: Vec<_> = std::fs::read_dir(f.dir.join("fwd")).unwrap().collect();
    assert!(left.is_empty(), "the forward's directory is removed: {left:?}");
    let after = host_side.list_sessions().await.unwrap().into_iter().find(|s| s.id == shell.id).unwrap();
    assert_eq!((after.pid, after.state == skiff_core::session::SessionState::Done), (shell.pid, false));
}

/// M3 over the real forward: the agent list is the remote account's own (its
/// config, its PATH); a session starts in a folder on the Host and runs
/// there; rename, groups and end act on the remote skiffd. The app's own
/// config is never read for it.
#[tokio::test]
async fn remote_lifecycle_groups_and_agents_come_from_the_host_over_a_real_forward() {
    let Some(f) = fixture("m") else { return };
    let host_side = local_client(&f.socket()).await;
    std::fs::create_dir_all(f.dir.join("config")).unwrap();
    f.write("config/projects.toml", "[agents.commands]\nhostonly = \"/bin/sh -i\"\n");
    let work = f.dir.join("host λ");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();

    let selection = f.selection("ssh_config", &f.socket(), "fwd");
    let host = select(&selection).await.unwrap();
    let conn = host.connection();
    let agents = legacy_agents(&conn).await.unwrap();
    let mine = agents.iter().find(|a| a.id == "hostonly").expect("the Host account's own agent");
    assert_eq!((mine.command.as_str(), mine.custom, mine.installed, mine.enabled), ("/bin/sh -i", true, true, true));

    let words: Vec<String> = mine.command.split_whitespace().map(String::from).collect();
    let spec = SessionSpec {
        label: "hostonly".into(),
        name: Some("on the Host".into()),
        cwd: Some(work.clone()),
        command: Some(words[0].clone()),
        args: words[1..].to_vec(),
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let created = legacy_create(&conn, spec).await.unwrap();
    assert_eq!(created.cwd, work);
    let mut output = legacy_output(&conn, &created.id).unwrap();
    legacy_attach(&conn, &created.id).await.unwrap();
    skiff_client::host::local::legacy_input(&conn, &created.id, b"echo AT-$(pwd)\n".to_vec()).unwrap();
    until_seen(&mut output, &format!("AT-{}", work.display())).await;

    let renamed = legacy_rename(&conn, &created.id, "renamed on the Host").await.unwrap();
    assert_eq!(renamed.name.as_deref(), Some("renamed on the Host"));
    let group: skiff_core::group::Group = serde_json::from_value(serde_json::json!({
        "name":"Remote","layout":{"type":"pane","session":created.id},"cwd":work.to_str().unwrap()}))
    .unwrap();
    let saved = legacy_save_group(&conn, group).await.unwrap();
    assert!(!saved.id.is_empty());
    // The Host keeps it: its own CLI sees the same group.
    let theirs = host_side.list_groups().await.unwrap();
    assert!(theirs.iter().any(|g| g.id == saved.id && g.cwd.as_deref() == work.to_str()));
    assert!(legacy_groups(&conn).await.unwrap().iter().any(|g| g.id == saved.id));
    legacy_delete_group(&conn, &saved.id).await.unwrap();

    legacy_kill(&conn, &created.id).await.unwrap();
    let left = host_side.list_sessions().await.unwrap();
    assert!(!left.iter().any(|s| s.id == created.id), "the remote session ended on the Host");
    assert!(selection.process_control().is_none());
}

/// Resize contention, with the app over the forward and a client on the Host
/// (as `skiff attach` from Termius). A session has one PTY size. skiffd
/// applies each resize in the order it receives them: the last one applied
/// is the size for every client. It tells every client each new size, so the
/// app learns of a size it did not send. Nobody re-asserts on that event.
#[tokio::test]
async fn the_last_applied_resize_is_the_size_for_every_attached_client() {
    let Some(f) = fixture("r") else { return };
    let host_side = local_client(&f.socket()).await;
    let shell = host_side
        .create_session(SessionSpec { label: "shared".into(), command: Some("/bin/sh".into()), cols: 80, rows: 24, ..Default::default() })
        .await
        .unwrap();
    let selection = f.selection("ssh_config", &f.socket(), "fwd");
    let host = select(&selection).await.unwrap();
    let conn = host.connection();
    let mut updates = skiff_client::host::local::legacy_updates(&conn).unwrap();
    legacy_attach(&conn, &shell.id).await.unwrap();
    let size = || async {
        let s = host_side.list_sessions().await.unwrap().into_iter().find(|s| s.id == shell.id).unwrap();
        (s.cols, s.rows)
    };
    // The app sizes it, then the Host-side client, then the app again.
    skiff_client::host::local::legacy_resize(&conn, &shell.id, 100, 30).await.unwrap();
    assert_eq!(size().await, (100, 30));
    host_side.resize(&shell.id, 120, 40).await.unwrap();
    assert_eq!(size().await, (120, 40), "the later resize wins");
    // The app hears the size it did not send.
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match tokio::time::timeout_at(until, updates.recv()).await {
            Ok(Some(skiff_core::protocol::Event::SessionUpdated { session })) if session.id == shell.id && (session.cols, session.rows) == (120, 40) => break,
            Ok(Some(_)) => {}
            other => panic!("no foreign size event: {other:?}"),
        }
    }
    // Nothing re-asserts: the size stays until a client resizes again.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(size().await, (120, 40));
    skiff_client::host::local::legacy_resize(&conn, &shell.id, 90, 20).await.unwrap();
    assert_eq!(size().await, (90, 20));
    // Two at once: whichever skiffd applies last is the size, and both see it.
    let (a, b) = tokio::join!(
        skiff_client::host::local::legacy_resize(&conn, &shell.id, 70, 21),
        host_side.resize(&shell.id, 71, 22)
    );
    a.unwrap();
    b.unwrap();
    let last = size().await;
    assert!(last == (70, 21) || last == (71, 22), "{last:?}");
    let screen_size = host_side.list_sessions().await.unwrap().into_iter().find(|s| s.id == shell.id).map(|s| (s.cols, s.rows));
    assert_eq!(screen_size, Some(last));
}

#[tokio::test]
async fn unknown_or_changed_keys_closed_forwarding_and_a_wrong_socket_are_explained_and_refused() {
    let Some(mut f) = fixture("b") else { return };
    let _ = local_client(&f.socket()).await;
    let unknown = select(&f.selection("unknown_config", &f.socket(), "u")).await.err().unwrap();
    assert!(matches!(ssh_error(&unknown), Some(SshError::HostKeyUnknown(_))), "{unknown:#}");
    let changed = select(&f.selection("changed_config", &f.socket(), "c")).await.err().unwrap();
    assert!(matches!(ssh_error(&changed), Some(SshError::HostKeyChanged(_))), "{changed:#}");
    // Strict checking learned nothing: the fixture's known_hosts files are unchanged.
    assert_eq!(std::fs::read_to_string(f.dir.join("empty_hosts")).unwrap(), "");

    let missing = select(&f.selection("ssh_config", Path::new("/nonexistent/skiffd.sock"), "m")).await.err().unwrap();
    assert!(matches!(ssh_error(&missing), Some(SshError::NoSkiffd { .. })), "{missing:#}");

    assert!(f.start_sshd("sshd_noforward"));
    let refused = select(&f.selection("ssh_config", &f.socket(), "r")).await.err().unwrap();
    // This sshd says no more than for a missing socket. The probe, over the
    // same sshd, sees the live socket: the cause is the blocked forward.
    let Some(SshError::NoSkiffd { .. }) = ssh_error(&refused).cloned() else {
        return assert!(matches!(ssh_error(&refused), Some(SshError::ForwardingRefused(_))), "{refused:#}");
    };
    let target = SshTarget::new("fixture", f.socket().to_string_lossy().into_owned());
    let run = skiff_client::host::check::run_probe(&target, &f.options("ssh_config"), "skiffd.sock", Some(&target.socket)).await.unwrap();
    let skiff_client::host::check::ProbeRun::Probed(probe, _) = run else { panic!("the probe did not run") };
    assert!(probe.socket_live, "{probe:?}");
    let refined = ssh_error(&refused).cloned().unwrap().refine(probe.socket_live);
    assert!(matches!(refined, SshError::ForwardingRefused(_)), "{refined}");
    assert!(refined.to_string().contains("AllowStreamLocalForwarding"), "{refined}");
    // A missing socket stays a missing skiffd.
    let missing = ssh_error(&missing).cloned().unwrap().refine(false);
    assert!(matches!(missing, SshError::NoSkiffd { .. }), "{missing}");
}

#[tokio::test]
async fn a_remote_skiffd_without_identity_is_refused_and_no_local_identity_stands_in() {
    let Some(f) = fixture_with("c", true) else { return };
    let info = local_client(&f.socket()).await.daemon_info().await.unwrap();
    assert_eq!(info.identity, None);
    let selection = f.selection("ssh_config", &f.socket(), "i");
    let refused = select(&selection).await.err().unwrap();
    assert!(refused.to_string().contains("reports no Host identity"), "{refused:#}");
    // No bootstrap file was read or made for the remote Host.
    assert!(!f.dir.join("i/local-host-id").exists());
    assert!(std::fs::read_dir(&f.dir).unwrap().all(|e| e.unwrap().file_name() != "local-host-id"));
}

/// M4 over a real forward: the Host's `skiff-host` (a real process with the
/// Host's private state, started on the Host side by the test, never by the
/// app) serves that Host's projects and Git status through a second forward
/// to the socket beside its skiffd. Both planes report one Host. Stopping
/// the data plane leaves the remote terminal working.
#[tokio::test]
async fn remote_projects_and_git_status_come_from_the_hosts_data_plane_over_a_real_forward() {
    use skiff_client::host::data::{self, DataError};
    let Some(f) = fixture("d") else { return };
    let _ = local_client(&f.socket()).await;
    let repo = f.dir.join("host repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-C"])
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::create_dir_all(f.dir.join("config")).unwrap();
    f.write("config/projects.toml", &format!("[[project]]\nname = \"on the Host\"\npath = \"{}\"\nfleet = \"far\"\n", repo.display()));
    // M6: a private ledger with one result inside its folder and one outside.
    let ledger = f.dir.join("agent/ledger.db");
    std::fs::create_dir_all(f.dir.join("agent/results")).unwrap();
    std::fs::write(f.dir.join("agent/results/r.md"), "# Result\n").unwrap();
    let inside = f.dir.join("agent/results/r.md").display().to_string();
    let outside = f.dir.join("config/projects.toml").display().to_string();
    let sql = format!(
        "CREATE TABLE tasks (id INTEGER PRIMARY KEY, uid TEXT UNIQUE, kind TEXT, parent TEXT, pane_id TEXT, pane_gen INTEGER, project TEXT, task TEXT,
           sent_at TEXT, state TEXT, result_path TEXT, ref TEXT, updated_at TEXT, type TEXT, socket TEXT, reported_at TEXT, title TEXT, closed_at TEXT, gone_at TEXT);
         INSERT INTO tasks (uid, kind, project, task, sent_at, state, result_path, updated_at) VALUES ('aaaa0001', 'task', '', 'in', 'x', 'done', '{inside}', 'x');
         INSERT INTO tasks (uid, kind, project, task, sent_at, state, result_path, updated_at) VALUES ('aaaa0002', 'task', '', 'out', 'x', 'done', '{outside}', 'x');
         PRAGMA user_version = 8;"
    );
    let made = Command::new("sqlite3").arg(&ledger).arg(sql).output();
    let ledger_made = made.is_ok_and(|o| o.status.success());
    if !ledger_made {
        eprintln!("skipped the M6 ledger check: no sqlite3");
    }

    let binary = std::env::var_os("SKIFF_HOST_BIN").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_BIN_EXE_skiffd")).with_file_name("skiff-host"));
    assert!(binary.is_file(), "build skiff-host first: cargo build -p skiff-host (or set SKIFF_HOST_BIN)");
    let start = || {
        Command::new(&binary)
            .arg("--session-socket")
            .arg(f.socket())
            .env("SKIFF_CONFIG", f.dir.join("config/projects.toml"))
            .env("SKIFF_LEDGER", &ledger)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::options().create(true).append(true).open(f.dir.join("skiff-host.log")).unwrap())
            .spawn()
            .unwrap()
    };
    struct Service(Child);
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let service = Service(start());
    let data_socket = skiff_core::socket::data_socket(&f.socket());
    let until = Instant::now() + Duration::from_secs(10);
    while std::os::unix::net::UnixStream::connect(&data_socket).is_err() {
        assert!(Instant::now() < until, "skiff-host did not listen");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let selection = f.selection("ssh_config", &f.socket(), "fwd");
    let host = select(&selection).await.unwrap();
    let sessions = host.connection();
    let host_id = sessions.hello().host.host_id.clone();
    let plane = selection.connect_data(&host_id).await.unwrap();
    let c = plane.connection();
    assert_eq!(c.hello().host, sessions.hello().host, "one Host on both forwards");
    assert_ne!(c.hello().instance_id, sessions.hello().instance_id);

    let projects = data::project_list(&c).await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!((projects[0].name.as_str(), projects[0].fleet.as_deref()), ("on the Host", Some("far")));
    let worktree = projects[0].worktrees[0].path.clone();
    assert_eq!(Path::new(&worktree).canonicalize().unwrap(), repo.canonicalize().unwrap());
    let changes = data::git_changes(&c, &worktree).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].path.as_str(), changes[0].added, changes[0].removed), ("a.txt", Some(1), Some(0)));
    // Its diff text comes from the Host's git over the same forward.
    let diff = data::git_diff(&c, &worktree, Some("a.txt")).await.unwrap();
    assert!(diff.text.contains("a/a.txt") && !diff.truncated && !diff.binary, "{diff:?}");
    // M5: its files, listed and read on the Host over the same forward.
    let listing = data::file_list(&c, &worktree, "").await.unwrap();
    assert_eq!(listing.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.txt"]);
    let read = data::file_read(&c, &worktree, "a.txt", skiff_host_api::ReadPurpose::Copy).await.unwrap();
    assert_eq!(read.bytes.as_deref(), Some(b"one\ntwo\n".as_slice()));
    assert!(matches!(data::file_read(&c, &worktree, "../x", skiff_host_api::ReadPurpose::Preview).await, Err(skiff_host_api::Error::NotFound { .. })));
    // M6: the Host's ledger rows and a result by its row, over the same forward;
    // a result outside the ledger's folder is not served.
    if ledger_made {
        let rows = data::ledger_rows(&c).await.unwrap();
        assert_eq!(rows.iter().map(|r| (r.uid.as_str(), r.result_exists)).collect::<Vec<_>>(), [("aaaa0001", true), ("aaaa0002", false)]);
        assert_eq!(data::ledger_result(&c, "aaaa0001").await.unwrap().text.as_deref(), Some("# Result\n"));
        assert!(matches!(data::ledger_result(&c, "aaaa0002").await, Err(skiff_host_api::Error::NotFound { .. })));
        assert!(!data::ledger_stamp(&c).await.unwrap().is_empty());
        assert!(data::skipper_settings(&c).await.is_ok());
    }

    // A terminal on the session plane, then the data plane stops on the Host.
    let spec = SessionSpec { cwd: Some(repo.clone()), cols: 80, rows: 24, ..Default::default() };
    let s = legacy_create(&sessions, spec).await.unwrap();
    let mut output = legacy_output(&sessions, &s.id).unwrap();
    legacy_attach(&sessions, &s.id).await.unwrap();
    // SAFETY: the test's own child.
    unsafe { libc::kill(service.0.id() as i32, libc::SIGTERM) };
    drop(service);
    assert!(matches!(data::project_list(&c).await, Err(skiff_host_api::Error::TransportLost)));
    drop(plane);
    let down = selection.connect_data(&host_id).await.err().unwrap();
    assert!(matches!(down.downcast_ref(), Some(DataError::NotRunning)), "{down:#}");
    skiff_client::host::local::legacy_input(&sessions, &s.id, b"echo STILL-$((7*6))\n".to_vec()).unwrap();
    until_seen(&mut output, "STILL-42").await;
    assert!(!sessions.is_closed());
    legacy_kill(&sessions, &s.id).await.unwrap();
}

/// #166 R2: Add project on a remote Host, picked from the Host's folders
/// over a real forward. Every path comes from a listing; nothing is typed.
/// A plain folder becomes a repository through the Host, then a project.
#[tokio::test]
async fn remote_add_project_is_picked_from_the_hosts_folders_over_a_real_forward() {
    use skiff_client::host::data;
    let Some(f) = fixture("f") else { return };
    let _ = local_client(&f.socket()).await;
    let home = f.dir.join("home");
    let api = home.join("code/api");
    std::fs::create_dir_all(&api).unwrap();
    std::fs::create_dir_all(home.join("code/notes")).unwrap();
    let ok = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).arg("-C").arg(&api)
        .args(["init", "-q", "-b", "main"]).status().unwrap();
    assert!(ok.success());
    std::fs::create_dir_all(f.dir.join("config")).unwrap();
    f.write("config/projects.toml", "");
    let binary = std::env::var_os("SKIFF_HOST_BIN").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_BIN_EXE_skiffd")).with_file_name("skiff-host"));
    assert!(binary.is_file(), "build skiff-host first: cargo build -p skiff-host (or set SKIFF_HOST_BIN)");
    struct Service(Child);
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _service = Service(
        Command::new(&binary)
            .arg("--session-socket")
            .arg(f.socket())
            .env("SKIFF_CONFIG", f.dir.join("config/projects.toml"))
            .env("HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::options().create(true).append(true).open(f.dir.join("skiff-host.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let data_socket = skiff_core::socket::data_socket(&f.socket());
    let until = Instant::now() + Duration::from_secs(10);
    while std::os::unix::net::UnixStream::connect(&data_socket).is_err() {
        assert!(Instant::now() < until, "skiff-host did not listen");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let selection = f.selection("ssh_config", &f.socket(), "fwd");
    let host = select(&selection).await.unwrap();
    let plane = selection.connect_data(&host.connection().hello().host.host_id).await.unwrap();
    let c = plane.connection();
    assert!(c.hello().capabilities.contains(&Capability::folders()));

    // Home, then a click on each row: the next path is the listing's own.
    let start = data::folder_home(&c).await.unwrap();
    assert_eq!(Path::new(&start), home.canonicalize().unwrap());
    let open = |l: &skiff_host_api::FolderListing, name: &str| {
        assert!(l.entries.iter().any(|e| e.name == name), "{name} in {l:?}");
        format!("{}/{name}", l.real)
    };
    let top = data::folder_list(&c, &start, false, None).await.unwrap();
    let code = data::folder_list(&c, &open(&top, "code"), false, None).await.unwrap();
    let rows: Vec<_> = code.entries.iter().map(|e| (e.name.as_str(), e.repo.as_ref().and_then(|r| r.branch.as_deref()))).collect();
    assert_eq!(rows, [("api", Some("main")), ("notes", None)]);

    // Add project: the picked repository.
    let picked = open(&code, "api");
    let folder = data::project_inspect(&c, &picked).await.unwrap();
    assert!(folder.root.is_some(), "{folder:?}");
    let (_, revision) = data::project_rows(&c).await.unwrap();
    let (added, _) = data::project_add(&c, &revision.unwrap(), folder.root.as_deref().unwrap(), None, None, None).await.unwrap();
    assert_eq!(added.name, "api");

    // Not a repository: Initialize repository through the Host, then add it.
    let notes = open(&code, "notes");
    assert!(data::project_inspect(&c, &notes).await.unwrap().root.is_none());
    let init = data::project_init(&c, &notes).await.unwrap();
    assert!(init.root.is_some() && home.join("code/notes/.git").is_dir(), "{init:?}");
    let (_, revision) = data::project_rows(&c).await.unwrap();
    data::project_add(&c, &revision.unwrap(), init.root.as_deref().unwrap(), None, None, None).await.unwrap();
    let names: Vec<_> = data::project_list(&c).await.unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["api", "notes"]);

    // New folder… takes one name only.
    for bad in ["..", "a/b"] {
        assert!(matches!(data::folder_create(&c, &code.real, bad).await, Err(skiff_host_api::Error::Invalid { .. })), "{bad}");
    }
    let made = data::folder_create(&c, &code.real, "scratch").await.unwrap();
    assert!(Path::new(&made).is_dir());

    // #166 R4: New session › Choose a folder…: a session starts in the
    // picked folder, and the Host's Recent lists it after.
    let sessions = host.connection();
    let picked = open(&data::folder_list(&c, &code.real, false, None).await.unwrap(), "scratch");
    let spec = SessionSpec { cwd: Some(PathBuf::from(&picked)), cols: 80, rows: 24, ..Default::default() };
    let s = legacy_create(&sessions, spec).await.unwrap();
    let mut output = legacy_output(&sessions, &s.id).unwrap();
    legacy_attach(&sessions, &s.id).await.unwrap();
    skiff_client::host::local::legacy_input(&sessions, &s.id, b"echo AT-$(pwd)\n".to_vec()).unwrap();
    until_seen(&mut output, &format!("AT-{picked}")).await;
    let places = data::folder_places(&c).await.unwrap();
    assert!(places.recent.contains(&picked), "{places:?}");
    assert!(places.recent.contains(&api.canonicalize().unwrap().display().to_string()), "the added project: {places:?}");
    legacy_kill(&sessions, &s.id).await.unwrap();
}

/// #166 slice 3, over the loopback sshd. Unknown key: a scan learns it into
/// a temp file only, a trust adds one known_hosts line, a second trust adds
/// none. Changed key: replace leaves only the new one. Install: one secret
/// through askpass adds the key once; a second install adds nothing, and a
/// wrong secret changes nothing. The server's home is a temp folder
/// (sshd SetEnv HOME), and every known_hosts is the fixture's: nothing
/// touches this account's ~/.ssh.
#[tokio::test]
async fn a_host_key_is_trusted_once_and_a_key_is_installed_once() {
    use skiff_client::host::{check, keys};
    let Some(mut f) = fixture("k") else { return };
    let d = f.dir.clone();
    let target = SshTarget::new("fixture", "");
    let lines = |name: &str| std::fs::read_to_string(d.join(name)).unwrap().lines().filter(|l| !l.is_empty()).count();
    let probed = |r: check::ProbeRun| matches!(r, check::ProbeRun::Probed(..));

    // Unknown key.
    let unknown = f.options("unknown_config");
    assert!(!probed(check::run_probe(&target, &unknown, "skiffd.sock", None).await.unwrap()));
    let scan = keys::scan_host_key(&target, &unknown, &d).await.unwrap();
    assert_eq!(scan.file, d.join("empty_hosts"));
    assert_eq!((scan.lines.len(), scan.fingerprints.len(), scan.known.len()), (1, 1, 0), "{scan:?}");
    assert_eq!(lines("empty_hosts"), 0, "the scan wrote nothing the person owns");
    keys::trust_host_key(&scan, false).await.unwrap();
    keys::trust_host_key(&scan, false).await.unwrap();
    assert_eq!(lines("empty_hosts"), 1);
    assert!(probed(check::run_probe(&target, &unknown, "skiffd.sock", None).await.unwrap()), "strict checking now passes");

    // Changed key: the old one shows, and replace keeps only the new one.
    let changed = f.options("changed_config");
    let scan = keys::scan_host_key(&target, &changed, &d).await.unwrap();
    assert_eq!(scan.known.len(), 1);
    assert_ne!(scan.known[0], scan.fingerprints[0]);
    keys::trust_host_key(&scan, true).await.unwrap();
    assert_eq!(lines("changed_hosts"), 1);
    assert!(probed(check::run_probe(&target, &changed, "skiffd.sock", None).await.unwrap()));

    // Install. A locked key stands in for the password, which an
    // unprivileged sshd cannot check: the same askpass path carries it.
    std::fs::create_dir_all(d.join("home")).unwrap();
    for (name, pass) in [("locked_key", "pass one"), ("new_key", "")] {
        assert!(Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", pass, "-f"]).arg(d.join(name)).status().unwrap().success());
    }
    let mut authorized = std::fs::read_to_string(d.join("authorized_keys")).unwrap();
    authorized.push_str(&std::fs::read_to_string(d.join("locked_key.pub")).unwrap());
    f.write("authorized_keys", &authorized);
    let conf = f.dir.join("sshd_config");
    let text = std::fs::read_to_string(&conf).unwrap().replace(
        &format!("AuthorizedKeysFile {}/authorized_keys", d.display()),
        &format!("AuthorizedKeysFile {0}/authorized_keys {0}/home/.ssh/authorized_keys\nSetEnv HOME={0}/home", d.display()),
    );
    f.write("sshd_home", &text);
    assert!(f.start_sshd("sshd_home"));
    for (config, key) in [("locked_config", "locked_key"), ("new_config", "new_key")] {
        f.write(config, &format!(
            "Host fixture\n  HostName 127.0.0.1\n  Port {}\n  IdentityFile {}/{key}\n  IdentitiesOnly yes\n  UserKnownHostsFile {}/known_hosts\n  GlobalKnownHostsFile /dev/null\n",
            f.port, d.display(), d.display()
        ));
    }
    let new_key = std::fs::read_to_string(d.join("new_key.pub")).unwrap();
    let with_new = f.options("new_config");
    assert!(!probed(check::run_probe(&target, &with_new, "skiffd.sock", None).await.unwrap()), "the new key is not there yet");
    let locked = f.options("locked_config");
    let wrong = keys::install_key(&target, &locked, "publickey", &new_key, "wrong".into()).await;
    assert_eq!(wrong, Err(keys::StepError::Refused));
    assert!(!d.join("home/.ssh/authorized_keys").exists(), "a refused secret changed nothing");
    let first = keys::install_key(&target, &locked, "publickey", &new_key, "pass one".into()).await.unwrap();
    assert_eq!(first, keys::Installed { added: true, ssh_dir_own: true, file_own: true });
    let second = keys::install_key(&target, &locked, "publickey", &new_key, "pass one".into()).await.unwrap();
    assert!(!second.added);
    let installed = std::fs::read_to_string(d.join("home/.ssh/authorized_keys")).unwrap();
    assert_eq!(installed.lines().filter(|l| l.trim() == new_key.trim()).count(), 1, "{installed}");
    assert!(probed(check::run_probe(&target, &with_new, "skiffd.sock", None).await.unwrap()), "the new key logs in with no prompt");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(d.join("home/.ssh/authorized_keys")).unwrap().permissions().mode() & 0o777, 0o600);

    // Fix permissions: the file is put aside and a new one holds the key.
    let fixed = keys::fix_key_file(&target, &locked, "publickey", &new_key, "pass one".into()).await;
    assert_eq!(fixed, Ok(()));
    let aside = std::fs::read_dir(d.join("home/.ssh")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("authorized_keys.skiff-")).count();
    assert_eq!(aside, 1);
    assert!(probed(check::run_probe(&target, &with_new, "skiffd.sock", None).await.unwrap()));
}

/// #166 slice 5, over the loopback sshd. The server's home and runtime
/// folder are temp folders (sshd SetEnv), so the skiffd it starts listens
/// in the fixture, never on this account's own socket. With no systemd
/// user manager in that session, the install takes the fallback start. A
/// second install changes nothing. Linger is never touched here.
#[tokio::test]
async fn skiff_installs_once_into_the_home_and_its_skiffd_answers() {
    use skiff_client::host::{check, install};
    let Some(mut f) = fixture("n") else { return };
    let d = f.dir.clone();
    for sub in ["home", "run", "beside"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let text = std::fs::read_to_string(d.join("sshd_config")).unwrap();
    f.write("sshd_install", &format!("{text}SetEnv HOME={0}/home XDG_RUNTIME_DIR={0}/run\n", d.display()));
    assert!(f.start_sshd("sshd_install"));
    let linger = || Command::new("loginctl").args(["show-user", &std::env::var("USER").unwrap_or_default(), "-p", "Linger", "--value"]).output().map(|o| o.stdout).unwrap_or_default();
    let linger_before = linger();

    // This test's own build: the real skiffd, and stand-ins for the others.
    let skiffd = Path::new(env!("CARGO_BIN_EXE_skiffd"));
    std::fs::copy(skiffd, d.join("beside/skiffd")).unwrap();
    for b in ["skiff-host", "skiff"] {
        std::fs::write(d.join("beside").join(b), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(d.join("beside").join(b), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let version = "9.9.9-test";
    let arch = std::env::consts::ARCH;
    let b = install::dev_bundle(version, arch, &d.join("beside")).await.unwrap();
    let target = SshTarget::new("fixture", "");
    let options = f.options("ssh_config");
    let name = "skiffd-dev.sock";
    let probe = |o: &SshOptions| {
        let (t, o) = (target.clone(), o.clone());
        async move {
            match check::run_probe(&t, &o, name, None).await.unwrap() {
                check::ProbeRun::Probed(p, _) => p,
                _ => panic!("the probe did not run"),
            }
        }
    };
    let before = probe(&options).await;
    assert_eq!((before.managed.as_ref(), before.socket_live, before.systemd), (None, false, false), "{before:?}");
    assert_eq!(before.runtime_dir, d.join("run").to_string_lossy());

    let first = install::install(&target, &options, &b, version, name, before.free_kb, "fixture", &|_| {}).await.unwrap();
    let pidfile = d.join("home/.local/share/skiff/server/skiffd.pid");
    // Stops only the skiffd this test started, by its pid file.
    struct Stop(PathBuf);
    impl Drop for Stop {
        fn drop(&mut self) {
            if let Ok(pid) = std::fs::read_to_string(&self.0).map(|p| p.trim().parse::<i32>().unwrap_or(0)) {
                if pid > 0 {
                    // SAFETY: a signal to the one process this test started.
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                }
            }
        }
    }
    let _stop = Stop(pidfile.clone());
    assert_eq!((first.changed, first.how.as_str(), first.running, first.cli_linked), (true, "fallback", true, true), "{first:?}");
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    let after = probe(&options).await;
    assert_eq!(after.managed.as_ref().map(|m| m.version.as_str()), Some(version));
    assert!(after.socket_live && after.fallback, "{after:?}");
    assert!(d.join("run/skiff").join(name).exists(), "skiffd listens in the fixture's runtime folder");
    // A release socket beside it: the probe finds both, keeps this app's,
    // and lists the two with the version each reports. The managed CLI is
    // a stand-in that prints nothing, so a cargo one answers `status`.
    let rel = d.join("run/skiff/skiffd.sock");
    let listener = std::os::unix::net::UnixListener::bind(&rel).unwrap();
    let cargo = d.join("home/.cargo/bin");
    std::fs::create_dir_all(&cargo).unwrap();
    let status = r#"#!/bin/sh
case $2 in
  *skiffd.sock) v=2.0.0 ;;
  *) v=9.9.9-test ;;
esac
printf '{"skiff":"%s","skiffd":{"version":"%s","protocol":27}}\n' "$v" "$v"
"#;
    std::fs::write(cargo.join("skiff"), status).unwrap();
    std::fs::set_permissions(cargo.join("skiff"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let both = probe(&options).await;
    let found: Vec<_> = both.sockets.iter().map(|s| (s.name.as_str(), s.version.as_str())).collect();
    assert_eq!(found, [("skiffd.sock", "2.0.0"), ("skiffd-dev.sock", version)], "{both:?}");
    assert!(both.socket.ends_with(name) && both.socket_live, "{both:?}");
    // The stored one wins while it is there.
    let stored = rel.to_string_lossy().into_owned();
    let p = match check::run_probe(&target, &options, name, Some(&stored)).await.unwrap() {
        check::ProbeRun::Probed(p, _) => p,
        _ => panic!("the probe did not run"),
    };
    assert_eq!(p.socket, stored);
    assert_eq!(p.status.and_then(|s| s.skiffd).map(|d| d.version).as_deref(), Some("2.0.0"));
    drop(listener);
    std::fs::remove_file(&rel).unwrap();
    std::fs::remove_dir_all(d.join("home/.cargo")).unwrap();
    let server = d.join("home/.local/share/skiff/server");
    let key = std::fs::read_link(server.join("current")).unwrap();
    assert!(key.to_string_lossy().starts_with(&format!("{version}+dev-")), "{key:?}");
    let mtime = std::fs::metadata(server.join(&key).join("skiffd")).unwrap().modified().unwrap();

    // A second install changes nothing, and the same skiffd keeps running.
    let second = install::install(&target, &options, &b, version, name, after.free_kb, "fixture", &|_| {}).await.unwrap();
    assert_eq!((second.changed, second.running), (false, true), "{second:?}");
    assert_eq!(std::fs::read_to_string(&pidfile).unwrap(), pid);
    assert_eq!(std::fs::metadata(server.join(&key).join("skiffd")).unwrap().modified().unwrap(), mtime);
    assert!(!std::fs::read_dir(&server).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".incoming")), "no upload left behind");

    // The whole check list passes over the real forward; keep-running is
    // a warning (no systemd user manager here).
    let input = check::CheckInput {
        target: target.clone(),
        name: "fixture".into(),
        stored: None,
        socket_name: name.into(),
        pin: None,
        forward_dir: d.join("chk"),
        home: None,
        agent: false,
        installed: None,
        version: version.into(),
        build: None,
        linger_refused: false,
        update_failed: None,
    };
    let report = check::check(&input, &options).await;
    assert!(report.ok, "{:#?}", report.rows);
    let keep = report.rows.last().unwrap();
    assert_eq!(keep.state, check::State::Warning);
    // Start on a running one changes nothing either.
    let again = install::start(&target, &options, name).await.unwrap();
    assert!(again.running);
    assert_eq!(std::fs::read_to_string(&pidfile).unwrap(), pid);
    assert_eq!(linger(), linger_before, "this account's linger did not change");
}

/// #166 slice 7, over the loopback sshd with a temp home and runtime
/// folder: the install starts the real skiff-host beside skiffd, and the
/// projects row passes. Stopped, the row is a warning with Start, and the
/// Host stays usable; Start brings it back. An install of a new version
/// restarts skiff-host and leaves skiffd's pid alone; the same version
/// again restarts nothing. No systemd user manager here: the fallback.
#[tokio::test]
async fn skiff_host_runs_beside_the_installed_skiffd_and_comes_back() {
    use skiff_client::host::{check, install};
    let Some(mut f) = fixture("h") else { return };
    let d = f.dir.clone();
    for sub in ["home", "run", "beside"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let text = std::fs::read_to_string(d.join("sshd_config")).unwrap();
    f.write("sshd_install", &format!("{text}SetEnv HOME={0}/home XDG_RUNTIME_DIR={0}/run\n", d.display()));
    assert!(f.start_sshd("sshd_install"));

    let skiffd = Path::new(env!("CARGO_BIN_EXE_skiffd"));
    let host = std::env::var_os("SKIFF_HOST_BIN").map(PathBuf::from).unwrap_or_else(|| skiffd.with_file_name("skiff-host"));
    assert!(host.is_file(), "build skiff-host first: cargo build -p skiff-host (or set SKIFF_HOST_BIN)");
    std::fs::copy(skiffd, d.join("beside/skiffd")).unwrap();
    std::fs::copy(&host, d.join("beside/skiff-host")).unwrap();
    std::fs::write(d.join("beside/skiff"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(d.join("beside/skiff"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let arch = std::env::consts::ARCH;
    let target = SshTarget::new("fixture", "");
    let options = f.options("ssh_config");
    let name = "skiffd-dev.sock";
    let server = d.join("home/.local/share/skiff/server");
    // Stops only the skiffd and skiff-host this test started, by their pid files.
    struct Stop(PathBuf);
    impl Drop for Stop {
        fn drop(&mut self) {
            for file in ["skiff-host.pid", "skiffd.pid"] {
                if let Ok(pid) = std::fs::read_to_string(self.0.join(file)).map(|p| p.trim().parse::<i32>().unwrap_or(0)) {
                    if pid > 0 {
                        // SAFETY: a signal to one process this test started.
                        unsafe { libc::kill(pid, libc::SIGTERM) };
                    }
                }
            }
        }
    }
    let _stop = Stop(server.clone());
    let pid = |file: &str| std::fs::read_to_string(server.join(file)).unwrap().trim().to_string();
    let input = check::CheckInput {
        target: target.clone(),
        name: "fixture".into(),
        stored: None,
        socket_name: name.into(),
        pin: None,
        forward_dir: d.join("chk"),
        home: None,
        agent: false,
        installed: None,
        version: "9.9.9-a".into(),
        build: None,
        linger_refused: false,
        update_failed: None,
    };
    let projects = |r: &check::Report| r.rows.iter().find(|l| l.row == check::Row::Projects).cloned().unwrap();

    let a = install::dev_bundle("9.9.9-a", arch, &d.join("beside")).await.unwrap();
    let first = install::install(&target, &options, &a, "9.9.9-a", name, 1 << 30, "fixture", &|_| {}).await.unwrap();
    assert_eq!((first.changed, first.how.as_str(), first.running, first.host_running), (true, "fallback", true, true), "{first:?}");
    assert!(d.join("run/skiff/skiffd-dev-host.sock").exists(), "skiff-host listens beside skiffd in the fixture");
    let report = check::check(&input, &options).await;
    assert!(report.ok, "{:#?}", report.rows);
    let row = projects(&report);
    assert_eq!(row.state, check::State::Passed, "{row:?}");

    // Stopped: a warning with Start; the Host is still usable.
    let host_pid = pid("skiff-host.pid");
    // SAFETY: the skiff-host this test started.
    unsafe { libc::kill(host_pid.parse().unwrap(), libc::SIGTERM) };
    let until = Instant::now() + Duration::from_secs(5);
    while d.join("run/skiff/skiffd-dev-host.sock").exists() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let report = check::check(&input, &options).await;
    let row = projects(&report);
    assert_eq!((row.state, row.fix), (check::State::Warning, Some(check::Fix::StartSkiff)), "{row:?}");
    assert!(report.ok, "sessions still work: {:#?}", report.rows);
    let started = install::start(&target, &options, name).await.unwrap();
    assert!(started.running && started.host_running, "{started:?}");
    assert_ne!(pid("skiff-host.pid"), host_pid);
    assert_eq!(projects(&check::check(&input, &options).await).state, check::State::Passed);

    // A new version restarts skiff-host; skiffd keeps its pid (its update is slice 6).
    let (daemon_pid, host_pid) = (pid("skiffd.pid"), pid("skiff-host.pid"));
    let b = install::dev_bundle("9.9.9-b", arch, &d.join("beside")).await.unwrap();
    let next = install::install(&target, &options, &b, "9.9.9-b", name, 1 << 30, "fixture", &|_| {}).await.unwrap();
    assert_eq!((next.changed, next.running, next.host_running), (true, true, true), "{next:?}");
    assert_eq!(pid("skiffd.pid"), daemon_pid);
    let restarted = pid("skiff-host.pid");
    assert_ne!(restarted, host_pid, "the new version's skiff-host runs");
    let exe = std::fs::read_link(format!("/proc/{restarted}/exe")).unwrap();
    assert!(exe.parent().unwrap().to_string_lossy().ends_with(&("/9.9.9-b+".to_string() + &install::dev_build(&d.join("beside")).unwrap())), "{exe:?}");
    assert!(std::fs::read_link(format!("/proc/{host_pid}/exe")).is_err(), "the old one stopped");
    // The same version again restarts nothing.
    let same = install::install(&target, &options, &b, "9.9.9-b", name, 1 << 30, "fixture", &|_| {}).await.unwrap();
    assert_eq!((same.changed, same.host_running), (false, true), "{same:?}");
    assert_eq!(pid("skiff-host.pid"), restarted);
    // An update restarts skiff-host on the new version too (slice 6 with
    // slice 7's restart); skiffd keeps its pid.
    let c = install::dev_bundle("9.9.9-c", arch, &d.join("beside")).await.unwrap();
    let up = install::update(&target, &options, &c, "9.9.9-c", name, 1 << 30, "fixture", false, false, &|_| {}).await.unwrap();
    assert!(matches!(up, install::Updated::Done { .. }), "{up:?}");
    assert_eq!(pid("skiffd.pid"), daemon_pid);
    let updated = pid("skiff-host.pid");
    assert_ne!(updated, restarted, "the update restarted skiff-host");
    let exe = std::fs::read_link(format!("/proc/{updated}/exe")).unwrap();
    assert!(exe.parent().unwrap().to_string_lossy().ends_with(&("/9.9.9-c+".to_string() + &install::dev_build(&d.join("beside")).unwrap())), "{exe:?}");
    // The same version again: Same, and skiff-host keeps its pid.
    let again = install::update(&target, &options, &c, "9.9.9-c", name, 1 << 30, "fixture", false, false, &|_| {}).await.unwrap();
    assert_eq!(again, install::Updated::Same);
    assert_eq!(pid("skiff-host.pid"), updated);
}

/// #166 slice 8, over the loopback sshd with a temp home and runtime
/// folder, as above. Uninstall stops the managed skiffd and deletes the
/// managed folder and Skiff's link. Projects, other Skiff data and a
/// process Skiff did not start stay.
#[tokio::test]
async fn remove_and_uninstall_deletes_only_the_managed_install() {
    use skiff_client::host::install;
    let Some(mut f) = fixture("u") else { return };
    let d = f.dir.clone();
    for sub in ["home/projects", "home/.local/share/skiff", "run", "beside"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let text = std::fs::read_to_string(d.join("sshd_config")).unwrap();
    f.write("sshd_uninstall", &format!("{text}SetEnv HOME={0}/home XDG_RUNTIME_DIR={0}/run\n", d.display()));
    assert!(f.start_sshd("sshd_uninstall"));
    std::fs::copy(Path::new(env!("CARGO_BIN_EXE_skiffd")), d.join("beside/skiffd")).unwrap();
    // A stand-in skiff that lists one session, as `skiff ls --json` does.
    for (b, body) in [("skiff-host", "exit 0"), ("skiff", r#"echo '[{"id":"a1","label":"shell","name":"build"}]'"#)] {
        std::fs::write(d.join("beside").join(b), format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(d.join("beside").join(b), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let version = "9.9.9-test";
    let b = install::dev_bundle(version, std::env::consts::ARCH, &d.join("beside")).await.unwrap();
    let target = SshTarget::new("fixture", "");
    let options = f.options("ssh_config");
    let name = "skiffd-dev.sock";
    let done = install::install(&target, &options, &b, version, name, 0, "fixture", &|_| {}).await.unwrap();
    assert!(done.running && done.cli_linked, "{done:?}");
    let server = d.join("home/.local/share/skiff/server");
    let pid: i32 = std::fs::read_to_string(server.join("skiffd.pid")).unwrap().trim().parse().unwrap();
    struct Stop(i32);
    impl Drop for Stop {
        fn drop(&mut self) {
            // SAFETY: a signal to the one process this test started.
            unsafe { libc::kill(self.0, libc::SIGTERM) };
        }
    }
    let _stop = Stop(pid);
    // Files that are not Skiff's install, and a process that is not Skiff's
    // behind a pid file in the managed folder: all of them stay.
    std::fs::write(d.join("home/projects/keep.txt"), "mine").unwrap();
    std::fs::write(d.join("home/.local/share/skiff/keep"), "data").unwrap();
    let mut other = Command::new("sleep").arg("30").spawn().unwrap();
    std::fs::write(server.join("skiff-host.pid"), other.id().to_string()).unwrap();

    let p = install::preview(&target, &options, name).await.unwrap();
    assert_eq!(p, install::Preview { managed: true, sessions: vec!["build".into()] });
    install::uninstall(&target, &options, "fixture").await.unwrap();

    let alive = |p: i32| unsafe { libc::kill(p, 0) } == 0;
    for _ in 0..50 {
        if !alive(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(!alive(pid), "the managed skiffd stopped");
    assert!(!server.exists(), "the managed folder is gone");
    assert!(std::fs::symlink_metadata(d.join("home/.local/bin/skiff")).is_err(), "Skiff's link is gone");
    assert!(d.join("home/projects/keep.txt").exists() && d.join("home/.local/share/skiff/keep").exists());
    assert!(other.try_wait().unwrap().is_none(), "a process Skiff did not start keeps running");
    let _ = other.kill();

    // Nothing is Skiff's any more: no box, and an uninstall refuses.
    assert!(!install::preview(&target, &options, name).await.unwrap().managed);
    let e = install::uninstall(&target, &options, "fixture").await.unwrap_err();
    assert!(e.contains("did not install"), "{e}");
}

/// The macOS path over a real ssh run, on Linux: stand-ins for `uname`
/// (Darwin) and `launchctl` (it starts the plist's program with setsid and
/// keeps its pid), and the real skiffd. Real launchd is not tested here.
#[tokio::test]
async fn a_mac_server_gets_skiff_and_a_launch_agent_once() {
    use skiff_client::host::{check, install};
    use std::os::unix::fs::PermissionsExt;
    let Some(mut f) = fixture("o") else { return };
    let d = f.dir.clone();
    for sub in ["home", "run", "beside", "bin"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    std::fs::set_permissions(d.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
    // The launchctl stand-in starts, reports and stops each agent by its
    // label, with one pid file per label in the fixture.
    let pid_of = |label: &str| d.join(format!("com.solutionscay.skiff.{label}.pid"));
    let pidfile = pid_of("skiffd");
    let stubs = [
        ("uname", "#!/bin/sh\n[ \"$1\" = -m ] && exec /bin/uname -m\necho Darwin\n".to_string()),
        (
            "launchctl",
            format!(
                "#!/bin/sh\ndir={0}\ncase $1 in\n  print) case $2 in \"user/$(id -u)/\"*) l=${{2##*/}}; kill -0 \"$(cat \"$dir/$l.pid\" 2>/dev/null)\" 2>/dev/null && echo 'state = running' ;; *) exit 1 ;; esac ;;\n  bootstrap) l=$(basename \"$3\" .plist); set -- $(sed -n '/<array>/,/<\\/array>/s|.*<string>\\(.*\\)</string>.*|\\1|p' \"$3\"); setsid nohup \"$@\" </dev/null >/dev/null 2>&1 & echo $! > \"$dir/$l.pid\" ;;\n  bootout) l=${{2##*/}}; kill \"$(cat \"$dir/$l.pid\" 2>/dev/null)\" 2>/dev/null; rm -f \"$dir/$l.pid\"; echo \"$*\" >> \"$dir/other\" ;;\n  *) echo \"$*\" >> \"$dir/other\" ;;\nesac\n",
                d.display()
            ),
        ),
    ];
    for (name, text) in stubs {
        std::fs::write(d.join("bin").join(name), text).unwrap();
        std::fs::set_permissions(d.join("bin").join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let text = std::fs::read_to_string(d.join("sshd_config")).unwrap();
    f.write("sshd_mac", &format!("{text}SetEnv HOME={0}/home XDG_RUNTIME_DIR={0}/run PATH={0}/bin:/usr/bin:/bin\n", d.display()));
    assert!(f.start_sshd("sshd_mac"));
    // Stops only the skiffd and skiff-host the stand-in started, by their pid files.
    struct Stop(Vec<PathBuf>);
    impl Drop for Stop {
        fn drop(&mut self) {
            for file in &self.0 {
                if let Ok(pid) = std::fs::read_to_string(file).map(|p| p.trim().parse::<i32>().unwrap_or(0)) {
                    if pid > 0 {
                        // SAFETY: a signal to one process this test started.
                        unsafe { libc::kill(pid, libc::SIGTERM) };
                    }
                }
            }
        }
    }
    let _stop = Stop(vec![pid_of("skiff-host"), pidfile.clone()]);

    let skiffd = Path::new(env!("CARGO_BIN_EXE_skiffd"));
    let host = std::env::var_os("SKIFF_HOST_BIN").map(PathBuf::from).unwrap_or_else(|| skiffd.with_file_name("skiff-host"));
    assert!(host.is_file(), "build skiff-host first: cargo build -p skiff-host (or set SKIFF_HOST_BIN)");
    std::fs::copy(skiffd, d.join("beside/skiffd")).unwrap();
    std::fs::copy(&host, d.join("beside/skiff-host")).unwrap();
    std::fs::write(d.join("beside/skiff"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(d.join("beside/skiff"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let version = "9.9.9-mac";
    let b = install::dev_bundle(version, std::env::consts::ARCH, &d.join("beside")).await.unwrap();
    let target = SshTarget::new("fixture", "");
    let options = f.options("ssh_config");
    let name = "skiffd-dev.sock";
    let probe = || {
        let (t, o) = (target.clone(), options.clone());
        async move {
            match check::run_probe(&t, &o, name, None).await.unwrap() {
                check::ProbeRun::Probed(p, _) => p,
                _ => panic!("the probe did not run"),
            }
        }
    };
    let before = probe().await;
    assert_eq!((before.os.as_str(), before.bundle_os(), before.managed.as_ref(), before.unit_enabled), ("Darwin", Some("macos"), None, false), "{before:?}");

    let first = install::install(&target, &options, &b, version, name, before.free_kb, "fixture", &|_| {}).await.unwrap();
    assert_eq!((first.changed, first.how.as_str(), first.running, first.linger.as_str()), (true, "launchd", true, ""), "{first:?}");
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    assert!(d.join("home/Library/LaunchAgents/com.solutionscay.skiff.skiffd.plist").exists());
    assert!(d.join("home/Library/LaunchAgents/com.solutionscay.skiff.skiff-host.plist").exists());
    assert!(first.host_running, "skiff-host runs from its own agent: {first:?}");
    let host_pid = std::fs::read_to_string(pid_of("skiff-host")).unwrap();
    assert!(d.join("run/skiff").join(name).exists(), "skiffd listens in the fixture's runtime folder");
    let after = probe().await;
    assert!(after.socket_live && after.unit_enabled && after.unit_active && after.host_unit_enabled && !after.systemd, "{after:?}");

    // A second install and a Start change nothing: no second load, same pid.
    let second = install::install(&target, &options, &b, version, name, after.free_kb, "fixture", &|_| {}).await.unwrap();
    assert_eq!((second.changed, second.running), (false, true), "{second:?}");
    assert!(install::start(&target, &options, name).await.unwrap().running);
    assert_eq!(std::fs::read_to_string(&pidfile).unwrap(), pid);
    assert_eq!(std::fs::read_to_string(pid_of("skiff-host")).unwrap(), host_pid, "the same version again restarts no skiff-host");
    assert!(!std::fs::read_to_string(d.join("other")).unwrap_or_default().contains("bootout"));

    // The whole check list passes, and Keeps running passes on a Mac.
    let input = check::CheckInput {
        target: target.clone(),
        name: "fixture".into(),
        stored: None,
        socket_name: name.into(),
        pin: None,
        forward_dir: d.join("chk"),
        home: None,
        agent: false,
        installed: None,
        version: version.into(),
        build: None,
        linger_refused: false,
        update_failed: None,
    };
    let report = check::check(&input, &options).await;
    assert!(report.ok, "{:#?}", report.rows);
    assert_eq!(report.platform.as_ref().map(|p| p.0.as_str()), Some("macos"));
    assert_eq!(report.rows.last().unwrap().state, check::State::Passed, "{:#?}", report.rows);

    // An update on a Mac restarts skiff-host with kickstart -k (its agent
    // keeps the label), and never restarts skiffd's agent.
    let b2 = install::dev_bundle("9.9.9-mac2", std::env::consts::ARCH, &d.join("beside")).await.unwrap();
    let up = install::update(&target, &options, &b2, "9.9.9-mac2", name, after.free_kb, "fixture", false, false, &|_| {}).await.unwrap();
    assert!(matches!(up, install::Updated::Done { .. }), "{up:?}");
    let calls = std::fs::read_to_string(d.join("other")).unwrap_or_default();
    assert!(calls.contains("kickstart -k user/") && calls.contains("com.solutionscay.skiff.skiff-host"), "{calls}");
    assert!(!calls.lines().any(|l| l.starts_with("kickstart -k") && l.ends_with("com.solutionscay.skiff.skiffd")), "{calls}");
    assert_eq!(std::fs::read_to_string(&pidfile).unwrap(), pid, "skiffd kept its pid");

    // Uninstall on a Mac: both agents leave launchd and their files go.
    install::uninstall(&target, &options, "fixture").await.unwrap();
    for l in ["skiffd", "skiff-host"] {
        assert!(!d.join(format!("home/Library/LaunchAgents/com.solutionscay.skiff.{l}.plist")).exists(), "{l}");
        assert!(!pid_of(l).exists(), "{l} still runs");
    }
    assert!(!d.join("home/.local/share/skiff/server").exists());
    assert!(!d.join("run/skiff").join(name).exists() || std::os::unix::net::UnixStream::connect(d.join("run/skiff").join(name)).is_err(), "skiffd stopped");
}

/// #166 R7 and R8 over a real forward: a Skipper setting and an agent
/// setting written from the client land in the Host's config; Start Skipper
/// starts Skipper on the Host, and the Host's skiff-host starts its watcher.
/// A managed install's Skipper folder defaults to the account's home.
#[tokio::test]
async fn skipper_and_agents_are_changed_and_started_on_the_host_over_a_real_forward() {
    use skiff_client::host::data;
    use skiff_host_api::{AgentEdit, Error as E, SkipperEdit};
    let Some(f) = fixture("k") else { return };
    let local = local_client(&f.socket()).await;
    // A custom agent that every machine has: `sh`.
    std::fs::create_dir_all(f.dir.join("config")).unwrap();
    f.write("config/projects.toml", "[agents.commands]\nsk = \"sh\"\n");
    let bin = Path::new(env!("CARGO_BIN_EXE_skiffd")).to_path_buf();
    let host_bin = std::env::var_os("SKIFF_HOST_BIN").map(PathBuf::from).unwrap_or_else(|| bin.with_file_name("skiff-host"));
    assert!(host_bin.is_file(), "build skiff-host first: cargo build -p skiff-host");
    assert!(bin.with_file_name("skiff").is_file(), "build the CLI first: cargo build -p skiff-cli");
    struct Service(Child);
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let service = Service(
        Command::new(&host_bin)
            .arg("--session-socket")
            .arg(f.socket())
            .env("SKIFF_CONFIG", f.dir.join("config/projects.toml"))
            .env("SKIFF_LEDGER", f.dir.join("ledger.db"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(f.dir.join("skiff-host.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let data_socket = skiff_core::socket::data_socket(&f.socket());
    let until = Instant::now() + Duration::from_secs(10);
    while std::os::unix::net::UnixStream::connect(&data_socket).is_err() {
        assert!(Instant::now() < until, "skiff-host did not listen");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let selection = f.selection("ssh_config", &f.socket(), "fwd");
    let host = select(&selection).await.unwrap();
    let plane = selection.connect_data(&host.connection().hello().host.host_id).await.unwrap();
    let c = plane.connection();
    let config = || std::fs::read_to_string(f.dir.join("config/projects.toml")).unwrap();

    // R8: an agent change names its revision; a stale one is refused.
    let (agents, revision) = data::agent_config(&c).await.unwrap();
    assert!(agents.iter().any(|a| a.id == "sk" && a.installed), "{agents:?}");
    let (agents, next) = data::agent_edit(&c, &revision, AgentEdit::Command { agent: "sk".into(), command: "sh -i".into() }).await.unwrap();
    assert!(agents.iter().any(|a| a.id == "sk" && a.command == "sh -i"), "{agents:?}");
    assert!(config().contains("sk = \"sh -i\""), "{}", config());
    let stale = data::agent_edit(&c, &revision, AgentEdit::Remove { agent: "sk".into() }).await;
    assert!(matches!(stale, Err(E::Conflict { .. })), "{stale:?}");
    data::agent_edit(&c, &next, AgentEdit::Command { agent: "sk".into(), command: "sh".into() }).await.unwrap();

    // R7: Skipper's settings change on the Host; its folder is read there.
    data::skipper_edit(&c, SkipperEdit::Agent(Some("sk".into()))).await.unwrap();
    let on = data::skipper_edit(&c, SkipperEdit::Enabled(true)).await.unwrap();
    assert!(on.enabled && on.folder_status == "ready", "{on:?}");
    assert!(config().contains("agent = \"sk\""), "{}", config());
    let preview = data::skipper_folder_preview(&c, &f.dir.join("other").to_string_lossy()).await.unwrap();
    assert_eq!((preview.kind.as_str(), preview.skipper_runs), ("new", false), "{preview:?}");

    // Start Skipper: the Host starts the session, and a watcher for it.
    let opened = data::skipper_open(&c, None).await.unwrap().expect("a session");
    assert!(opened.created && opened.session.coordinator, "{opened:?}");
    assert_eq!(opened.watch_error, None);
    let id = opened.session.id.clone();
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let s = local.list_sessions().await.unwrap().into_iter().find(|s| s.id == id).unwrap();
        if s.watch.is_some() {
            break;
        }
        assert!(Instant::now() < until, "no watcher registered for Skipper");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // One already runs: the Host starts no second.
    assert!(!data::skipper_watch(&c, &id).await.unwrap());
    // Its session ends, and with it the watcher.
    local.kill(&id).await.unwrap();
    drop(plane);
    drop(service);

    // A managed install keeps its Skipper folder in home, never in the
    // runtime folder a restart empties.
    let home = f.dir.join("mhome");
    let run = f.dir.join("mrun");
    let managed = home.join(".local/share/skiff/server/9.9.9");
    std::fs::create_dir_all(&managed).unwrap();
    std::fs::create_dir_all(run.join("skiff")).unwrap();
    std::fs::copy(&host_bin, managed.join("skiff-host")).unwrap();
    let sock = run.join("skiff").join(skiff_core::socket::build_socket_name());
    let service = Service(
        Command::new(managed.join("skiff-host"))
            .arg("--session-socket")
            .arg(&sock)
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &run)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("SKIFF_CONFIG")
            .env_remove("SKIFF_STATE")
            .env_remove("SKIFF_LEDGER")
            .env_remove("SKIFF_SOCKET")
            .env_remove("SKIFF_REAL_STATE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(f.dir.join("managed-host.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    let data_socket = skiff_core::socket::data_socket(&sock);
    let until = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        if let Ok(s) = tokio::net::UnixStream::connect(&data_socket).await {
            break s;
        }
        assert!(Instant::now() < until, "the managed skiff-host did not listen");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let c = skiff_client::host::HostConnection::connect_with_timeout(stream, None, Duration::from_secs(5)).await.unwrap();
    let settings = data::skipper_settings(&c).await.unwrap();
    assert_eq!(Path::new(&settings.configured), home.join("skiff-agent"));
    assert!(home.join(".config/skiff/local-host-id").is_file(), "the managed install's state is the account's");
    drop(service);
}

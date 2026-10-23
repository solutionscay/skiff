//! Install and start Skiff on a Linux or macOS server, in the account's
//! home only: `~/.local/share/skiff/server/<version>`, a `current` link, a
//! systemd user unit each for skiffd and skiff-host (or, without a user
//! manager, a plain background start), linger, and `~/.local/bin/skiff`
//! where that name is free. On macOS a LaunchAgent each in
//! `~/Library/LaunchAgents` keeps them running instead. No sudo, and no
//! system folder. Each step runs only after the person
//! confirms it in Settings › Hosts.
//!
//! The bundle is the signed release bundle for the server's OS and arch,
//! from the release that matches this app's version, checked with the
//! app's key ([`super::bundle`]) before it leaves this computer. A debug
//! build with no such release uses its own binaries instead
//! ([`dev_bundle`]); a release build never does.
//!
//! A skiffd that Skiff did not install is never replaced, stopped or
//! started: the caller installs only when the probe found no live socket
//! and no managed install, and starts only the managed one.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::Serialize;
use tokio::{io::AsyncWriteExt, process::Command};

use super::{
    bundle,
    check::{batch_args, quote},
    ssh::{SshOptions, SshTarget},
};

/// The one script Skiff runs on a server, sent on stdin with a mode: the
/// read-only probe, the key steps, and the managed install's steps.
pub const SERVER: &str = include_str!("server.sh");
/// The repository whose releases carry the server bundles.
pub const RELEASES: &str = "https://github.com/solutionscay/skiff/releases/download";
const UNIT_SKIFFD: &str = include_str!("../../../../scripts/server-bundle/systemd/skiffd.service");
const UNIT_HOST: &str = include_str!("../../../../scripts/server-bundle/systemd/skiff-host.service");

/// How long one upload or install step may take.
const STEP_TIMEOUT: Duration = Duration::from_secs(180);

/// A bundle ready to send, and where it came from.
pub struct Bundle {
    pub name: String,
    pub bytes: Vec<u8>,
    /// `release`, or `this build` for a debug build's own binaries.
    pub source: &'static str,
}

/// What the install or a start did on the server.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// A new version went in. False: that version was there already.
    pub changed: bool,
    /// `systemd` or `fallback`.
    pub how: String,
    /// `yes`, `refused`, or empty without systemd.
    pub linger: String,
    /// skiffd's socket is there after the step.
    pub running: bool,
    /// skiff-host (projects) listens beside it after the step.
    pub host_running: bool,
    /// `~/.local/bin/skiff` is ours. False: another file has that name.
    pub cli_linked: bool,
}

/// A version Skiff sends as a value: digits, letters, dots, `+` and `-`.
pub(super) fn plain_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
}

/// The SHA-256 of a file, by the system tool.
async fn sha256(path: &Path) -> Result<String, String> {
    let out = match Command::new("sha256sum").arg(path).output().await {
        Ok(o) if o.status.success() => o,
        _ => Command::new("shasum").args(["-a", "256"]).arg(path).output().await.map_err(|e| e.to_string())?,
    };
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().filter(|h| h.len() == 64).map(String::from).ok_or_else(|| "Skiff cannot hash the bundle.".into())
}

fn private_dir(what: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("skiff-{what}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Downloads the signed bundle for `os`/`arch` from the release `v<version>`
/// and checks it with the app's key. Nothing unchecked comes back.
pub async fn download(version: &str, os: &str, arch: &str) -> Result<Bundle, String> {
    let name = bundle::bundle_name(version, os, arch);
    let dir = private_dir("download")?;
    let bytes = fetch(&format!("{RELEASES}/v{version}/{name}"), &dir.join("b")).await;
    let sig = fetch(&format!("{RELEASES}/v{version}/{name}.minisig"), &dir.join("s")).await;
    let _ = std::fs::remove_dir_all(&dir);
    let (Some(bytes), Some(sig)) = (bytes, sig) else {
        return Err(format!("Skiff could not download Skiff {version} for {os} {arch}. Check that this computer is online."));
    };
    let sig = String::from_utf8(sig).map_err(|_| format!("The downloaded Skiff {version} is not signed by Skiff. Skiff did not install it."))?;
    bundle::verify(&name, &bytes, &sig).map_err(|_| format!("The downloaded Skiff {version} is not signed by Skiff. Skiff did not install it."))?;
    Ok(Bundle { name, bytes, source: "release" })
}

/// One HTTPS download to `to`, then its bytes. None: it failed.
async fn fetch(url: &str, to: &Path) -> Option<Vec<u8>> {
    let ok = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--max-time", "300", "-o"])
        .arg(to)
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success());
    if ok { std::fs::read(to).ok() } else { None }
}

/// A debug build's own server: skiffd, skiff-host and skiff beside this
/// app's executable, packed as a bundle. Only for a server with this
/// computer's OS and arch, and never in a release build.
pub async fn dev_bundle(version: &str, arch: &str, beside: &Path) -> Result<Bundle, String> {
    if !cfg!(debug_assertions) {
        return Err("Only a dev build of Skiff installs its own binaries.".into());
    }
    let here = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => other,
    };
    let os = std::env::consts::OS;
    if !matches!(os, "linux" | "macos") || here != arch {
        return Err(format!("This dev build has no Skiff {version} for {arch}: it runs on another system, and no release has that version."));
    }
    let name = format!("skiff-server-{version}-{os}-{arch}");
    let stage = private_dir("bundle")?;
    let dir = stage.join(&name);
    let build = async {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut manifest = bundle::Manifest {
            name: "skiff-server".into(),
            version: version.into(),
            protocol: skiff_core::PROTOCOL,
            os: os.into(),
            arch: arch.into(),
            target: "this build".into(),
            commit: "dev".into(),
            files: Default::default(),
        };
        for b in ["skiffd", "skiff-host", "skiff"] {
            let from = beside.join(b);
            std::fs::copy(&from, dir.join(b)).map_err(|_| format!("This dev build has no {b} beside it. Build the workspace first."))?;
            manifest.files.insert(b.into(), sha256(&dir.join(b)).await?);
        }
        // The systemd units go in a Linux bundle only, and install.sh in
        // every bundle, as in build.sh.
        let units: &[(&str, &str)] = if os == "linux" { &[("systemd/skiffd.service", UNIT_SKIFFD), ("systemd/skiff-host.service", UNIT_HOST)] } else { &[] };
        for &(f, text) in units.iter().chain(&[("install.sh", SERVER)]) {
            if let Some(parent) = dir.join(f).parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(dir.join(f), text).map_err(|e| e.to_string())?;
            manifest.files.insert(f.into(), sha256(&dir.join(f)).await?);
        }
        let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("manifest.json"), json).map_err(|e| e.to_string())?;
        let tar = stage.join(format!("{name}.tar.gz"));
        let ok = Command::new("tar").arg("-C").arg(&stage).arg("-czf").arg(&tar).arg(&name).status().await.is_ok_and(|s| s.success());
        if !ok {
            return Err("tar could not pack this build's binaries.".to_string());
        }
        std::fs::read(&tar).map_err(|e| e.to_string())
    };
    let bytes = build.await;
    let _ = std::fs::remove_dir_all(&stage);
    Ok(Bundle { name: format!("{name}.tar.gz"), bytes: bytes?, source: "this build" })
}

/// One strict, batch ssh run of `command`, with `stdin`. Its stdout.
pub(super) async fn run(target: &SshTarget, options: &SshOptions, command: String, stdin: &[u8]) -> Result<String, String> {
    let mut child = Command::new(&options.program)
        .args(batch_args(target, options, command))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Skiff cannot run ssh: {e}"))?;
    let mut input = child.stdin.take().expect("piped");
    let data = stdin.to_vec();
    let writer = tokio::spawn(async move {
        let _ = input.write_all(&data).await;
    });
    let out = tokio::time::timeout(STEP_TIMEOUT, child.wait_with_output()).await.map_err(|_| "The server did not finish in time.".to_string())?.map_err(|e| e.to_string())?;
    let _ = writer.await;
    if out.status.code() == Some(255) {
        return Err("ssh stopped before the step ran. Check again.".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(super) fn value<'a>(out: &'a str, key: &str) -> &'a str {
    out.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('=')).unwrap_or("")
}

fn started(out: &str) -> Outcome {
    Outcome {
        changed: value(out, "changed") == "yes",
        how: value(out, "how").into(),
        linger: value(out, "linger").into(),
        running: value(out, "skiff-start") == "running",
        host_running: value(out, "host") == "running",
        cli_linked: value(out, "cli") == "linked",
    }
}

/// Checks the room on the server and uploads `b` beside the managed folder,
/// named by its SHA-256, which it answers. Two uploads at once never meet.
async fn upload(target: &SshTarget, options: &SshOptions, b: &Bundle, free_kb: u64, name: &str) -> Result<String, String> {
    // The bundle, its unpacked copy, and the one before.
    let need = (b.bytes.len() as u64 * 4) / 1024;
    if free_kb > 0 && free_kb < need {
        return Err(no_room(name, b));
    }
    let dir = private_dir("upload")?;
    let file = dir.join(&b.name);
    std::fs::write(&file, &b.bytes).map_err(|e| e.to_string())?;
    let hash = sha256(&file).await;
    let _ = std::fs::remove_dir_all(&dir);
    let hash = hash?;
    let upload = format!(r#"sh -c 'umask 077; d="$HOME/.local/share/skiff/server"; mkdir -p "$d" && cat > "$d/.incoming-{hash}.tar.gz"'"#);
    run(target, options, upload, &b.bytes).await?;
    Ok(hash)
}

fn no_room(name: &str, b: &Bundle) -> String {
    format!("{name} has no room for Skiff. It needs {} MB in your home folder.", ((b.bytes.len() as u64 * 4) / 1024).div_ceil(1024))
}

/// The server's words for a failed unpack, as plain lines.
fn unpack_error(out: &str, name: &str, version: &str, b: &Bundle) -> String {
    match value(out, "skiff-install") {
        "bad-hash" | "no-upload" => format!("The upload to {name} did not arrive whole. Nothing changed. Try again."),
        "no-room" => no_room(name, b),
        "bad-bundle" => format!("The bundle for {name} is not Skiff {version}. Nothing changed."),
        _ => format!("The install on {name} did not finish. Check again."),
    }
}

/// Uploads `b` to the server, unpacks it as `version` in the managed
/// folder, and starts its skiffd. The same version again changes nothing
/// and starts nothing that runs. `free_kb`: the probe's free space.
pub async fn install(target: &SshTarget, options: &SshOptions, b: &Bundle, version: &str, socket_name: &str, free_kb: u64, name: &str) -> Result<Outcome, String> {
    if !plain_version(version) || !plain_version(socket_name) {
        return Err("This version has a form Skiff does not send.".into());
    }
    let hash = upload(target, options, b, free_kb, name).await?;
    let command = format!("sh -s -- install {} {} {}", quote(version), quote(&hash), quote(socket_name));
    let out = run(target, options, command, SERVER.as_bytes()).await?;
    match value(&out, "skiff-install") {
        "done" => Ok(started(&out)),
        "busy" => Err(busy(name)),
        _ => Err(unpack_error(&out, name, version, b)),
    }
}

fn busy(name: &str) -> String {
    format!("Another computer is installing Skiff on {name}. Check again in a minute.")
}

/// What an update did on the server.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Updated {
    /// The new version runs. `restarted`: skiffd started again and its
    /// sessions ended, as the person agreed. Else they kept running.
    Done { from: String, restarted: bool },
    /// That version runs already: another computer updated it first.
    Same,
    /// The server runs a newer version, `from`. Nothing changed: never down.
    Newer { from: String },
    /// skiffd cannot hand its sessions to the new version. Nothing changed.
    /// The live sessions an update would end, by name.
    NoHandover { from: String, sessions: Vec<String> },
}

/// Moves the managed install up to `version`, never down. skiffd reloads
/// onto the new binary and keeps its sessions. When it cannot, nothing
/// changes, unless `end_sessions`: then skiffd restarts on the new version.
/// A failed update leaves the old version running.
#[allow(clippy::too_many_arguments)]
pub async fn update(target: &SshTarget, options: &SshOptions, b: &Bundle, version: &str, socket_name: &str, free_kb: u64, name: &str, end_sessions: bool) -> Result<Updated, String> {
    if !plain_version(version) || !plain_version(socket_name) {
        return Err("This version has a form Skiff does not send.".into());
    }
    let hash = upload(target, options, b, free_kb, name).await?;
    let agreed = if end_sessions { "yes" } else { "no" };
    let command = format!("sh -s -- update {} {} {} {agreed}", quote(version), quote(&hash), quote(socket_name));
    let out = run(target, options, command, SERVER.as_bytes()).await?;
    updated(&out, name, version, b)
}

fn updated(out: &str, name: &str, version: &str, b: &Bundle) -> Result<Updated, String> {
    let from = value(out, "from").to_string();
    match value(out, "skiff-update") {
        "done" => Ok(Updated::Done { from, restarted: value(out, "reload") == "restarted" }),
        "same" => Ok(Updated::Same),
        "newer" => Ok(Updated::Newer { from }),
        "no-handover" => Ok(Updated::NoHandover { from, sessions: live_sessions(value(out, "sessions")) }),
        "busy" => Err(busy(name)),
        "lost" => Err(format!("skiffd on {name} started again on Skiff {version} instead of reloading. Its sessions ended.")),
        "failed" if value(out, "skiff-install") != "done" => Err(unpack_error(out, name, version, b)),
        _ => Err(format!("The update on {name} did not finish. Skiff {from} still runs there. Try again.")),
    }
}

/// The names of the live sessions in `skiff ls --json`.
fn live_sessions(json: &str) -> Vec<String> {
    let list: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    list.iter()
        .filter(|s| s["state"].as_str() != Some("done"))
        .map(|s| s["name"].as_str().or(s["label"].as_str()).unwrap_or("a session").to_string())
        .collect()
}

/// Starts the managed skiffd and its skiff-host on the server, each if it
/// does not run.
pub async fn start(target: &SshTarget, options: &SshOptions, socket_name: &str) -> Result<Outcome, String> {
    if !plain_version(socket_name) {
        return Err("This socket name has a form Skiff does not send.".into());
    }
    let out = run(target, options, format!("sh -s -- start {}", quote(socket_name)), SERVER.as_bytes()).await?;
    match value(&out, "skiff-start") {
        "missing" => Err("Skiff is not installed there any more. Check again.".into()),
        _ => Ok(started(&out)),
    }
}

/// Turns on lingering for the account. False: the server refused it.
pub async fn linger(target: &SshTarget, options: &SshOptions) -> Result<bool, String> {
    let out = run(target, options, "sh -s -- linger".into(), SERVER.as_bytes()).await?;
    Ok(value(&out, "linger") == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_versions_go_to_the_server() {
        for ok in ["1.6.6", "2.0.0-rc.1", "skiffd.sock", "skiffd-dev.sock"] {
            assert!(plain_version(ok), "{ok}");
        }
        for bad in ["", "1.0;rm", "1 0", "$(x)", "a'b", "a/b"] {
            assert!(!plain_version(bad), "{bad}");
        }
    }

    #[test]
    fn the_update_lines_read_as_one_result() {
        let b = Bundle { name: "b".into(), bytes: vec![0; 4096], source: "release" };
        let read = |out: &str| updated(out, "devbox", "2.1.0", &b);
        assert_eq!(read("from=2.0.0\nchanged=yes\nreload=kept\nskiff-update=done\n"), Ok(Updated::Done { from: "2.0.0".into(), restarted: false }));
        assert_eq!(read("from=2.0.0\nreload=restarted\nskiff-update=done\n"), Ok(Updated::Done { from: "2.0.0".into(), restarted: true }));
        assert_eq!(read("from=2.2.0\nskiff-update=newer\n"), Ok(Updated::Newer { from: "2.2.0".into() }));
        let live = r#"[{"label":"zsh","name":"build","state":"working"},{"label":"api","name":null,"state":"idle"},{"label":"old","name":null,"state":"done"}]"#;
        assert_eq!(
            read(&format!("from=2.0.0\nsessions={live}\nskiff-update=no-handover\n")),
            Ok(Updated::NoHandover { from: "2.0.0".into(), sessions: vec!["build".into(), "api".into()] })
        );
        assert!(read("from=2.0.0\nskiff-install=done\nskiff-update=failed\n").unwrap_err().contains("Skiff 2.0.0 still runs"));
        assert!(read("from=2.0.0\nskiff-install=bad-hash\nskiff-update=failed\n").unwrap_err().contains("did not arrive whole"));
    }

    #[test]
    fn the_step_lines_read_as_an_outcome() {
        let o = started("changed=yes\ncli=linked\nhow=systemd\nlinger=refused\nhost=running\nskiff-start=running\nskiff-install=done\n");
        assert_eq!(o, Outcome { changed: true, how: "systemd".into(), linger: "refused".into(), running: true, host_running: true, cli_linked: true });
        assert!(!started("changed=no\nskiff-start=stopped\n").running);
    }

    /// server.sh and the probe as a Mac sees them: stand-ins for `uname`
    /// and `launchctl` (which logs its arguments, and answers `print` from
    /// a file), a temp home, and a live socket so the start does not wait.
    #[test]
    fn a_mac_gets_a_launch_agent_once_and_a_loaded_one_is_only_kickstarted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("skiff-mac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A home with an `&`, to see the plist escape it.
        let home = dir.join("a&b");
        let (bin, run) = (dir.join("bin"), dir.join("run"));
        for d in [&bin, &run.join("skiff"), &home.join(".local/share/skiff/server/1.0.0")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::os::unix::fs::symlink("1.0.0", home.join(".local/share/skiff/server/current")).unwrap();
        std::fs::write(home.join(".local/share/skiff/server/1.0.0/skiffd"), "").unwrap();
        std::fs::write(home.join(".local/share/skiff/server/installed-by"), "app\n").unwrap();
        std::fs::write(home.join(".local/share/skiff/server/1.0.0/manifest.json"), r#"{"version": "1.0.0", "protocol": 27}"#).unwrap();
        let log = dir.join("launchctl.log");
        let stubs = [
            ("uname", "#!/bin/sh\n[ \"$1\" = -m ] && echo arm64 || echo Darwin\n".to_string()),
            ("launchctl", format!("#!/bin/sh\necho \"$*\" >> '{0}'\n[ \"$1\" = print ] && grep -qx \"$2\" '{0}.loaded' 2>/dev/null\nexit $?\n", log.display())),
        ];
        for (name, text) in stubs {
            std::fs::write(bin.join(name), text).unwrap();
            std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::set_permissions(home.join(".local/share/skiff/server/1.0.0/skiffd"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let _sock = std::os::unix::net::UnixListener::bind(run.join("skiff/skiffd.sock")).unwrap();
        let sh = |script: &str, args: &[&str]| {
            let mut child = std::process::Command::new("sh")
                .args(["-s", "--"])
                .args(args)
                .env("HOME", &home)
                .env("XDG_RUNTIME_DIR", &run)
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            std::io::Write::write_all(&mut child.stdin.take().unwrap(), script.as_bytes()).unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
        };
        let uid = sh("id -u", &[]).trim().to_string();
        let plist = home.join("Library/LaunchAgents/com.solutionscay.skiff.skiffd.plist");

        // Not loaded: one bootstrap. /dev/console is not this account's
        // here, so it goes to the user domain, which ssh reaches.
        let out = sh(SERVER, &["start", "skiffd.sock"]);
        let o = started(&out);
        assert_eq!((o.how.as_str(), o.running, value(&out, "domain")), ("launchd", true, "user"), "{out}");
        let text = std::fs::read_to_string(&plist).unwrap();
        let want = format!("<string>{}/.local/share/skiff/server/current/skiffd</string>", dir.join("a&amp;b").display());
        assert!(text.contains(&want), "{text}");
        for key in ["<key>RunAtLoad</key>\n  <true/>", "<key>Crashed</key>\n    <true/>", "<string>com.solutionscay.skiff.skiffd</string>"] {
            assert!(text.contains(key), "{key}");
        }
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains(&format!("bootstrap user/{uid} {}", plist.display())), "{calls}");
        assert!(!calls.contains("kickstart") && !calls.contains("bootout"), "{calls}");
        // skiff-host gets its own agent, for this skiffd's socket, always kept alive.
        let host = std::fs::read_to_string(home.join("Library/LaunchAgents/com.solutionscay.skiff.skiff-host.plist")).unwrap();
        assert!(host.contains("/current/skiff-host</string>") && host.contains("<string>--session-socket</string>") && host.contains("<key>KeepAlive</key>\n  <true/>"), "{host}");
        assert!(host.contains(&format!("<string>{}</string>", run.join("skiff/skiffd.sock").display())), "{host}");

        // The probe sees the file; launchd has not loaded it in this stand-in.
        let p: super::super::check::Probe = serde_json::from_str(&sh(SERVER, &["probe", "skiffd.sock"])).unwrap();
        assert_eq!((p.os.as_str(), p.systemd, p.unit_enabled, p.unit_active), ("Darwin", false, true, false));

        // Loaded in the GUI domain: only a kickstart, never a second load.
        std::fs::write(format!("{}.loaded", log.display()), format!("gui/{uid}/com.solutionscay.skiff.skiffd\ngui/{uid}/com.solutionscay.skiff.skiff-host\n")).unwrap();
        std::fs::remove_file(&log).unwrap();
        let out = sh(SERVER, &["start", "skiffd.sock"]);
        assert_eq!(value(&out, "domain"), "gui", "{out}");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains(&format!("kickstart gui/{uid}/com.solutionscay.skiff.skiffd")) && !calls.contains("bootstrap"), "{calls}");
        assert!(calls.contains(&format!("kickstart gui/{uid}/com.solutionscay.skiff.skiff-host")) && !calls.contains("kickstart -k"), "{calls}");
        let p: super::super::check::Probe = serde_json::from_str(&sh(SERVER, &["probe", "skiffd.sock"])).unwrap();
        assert!(p.unit_active && p.host_unit_enabled);

        // Uninstall: both agents leave launchd, and their files go.
        let out = sh(SERVER, &["uninstall"]);
        assert_eq!(value(&out, "skiff-uninstall"), "done", "{out}");
        let calls = std::fs::read_to_string(&log).unwrap();
        for l in ["skiff-host", "skiffd"] {
            assert!(calls.contains(&format!("bootout gui/{uid}/com.solutionscay.skiff.{l}")), "{calls}");
            assert!(!home.join(format!("Library/LaunchAgents/com.solutionscay.skiff.{l}.plist")).exists());
        }
        assert!(!home.join(".local/share/skiff/server").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

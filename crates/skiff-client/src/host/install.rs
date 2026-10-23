//! Install and start Skiff on a Linux server, in the account's home only:
//! `~/.local/share/skiff/server/<version>`, a `current` link, a systemd
//! user unit each for skiffd and skiff-host (or, without a user manager, a
//! plain background start), linger, and `~/.local/bin/skiff` where that name is free. No
//! sudo, and no system folder. Each step runs only after the person
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

/// The managed install's steps on the server: install, start, linger.
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
fn plain_version(v: &str) -> bool {
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
/// app's executable, packed as a bundle. Only for a Linux server with this
/// computer's arch, and never in a release build.
pub async fn dev_bundle(version: &str, arch: &str, beside: &Path) -> Result<Bundle, String> {
    if !cfg!(debug_assertions) {
        return Err("Only a dev build of Skiff installs its own binaries.".into());
    }
    let here = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => other,
    };
    if !cfg!(target_os = "linux") || here != arch {
        return Err(format!("This dev build has no Skiff {version} for Linux {arch}: it runs on another system, and no release has that version."));
    }
    let name = format!("skiff-server-{version}-linux-{arch}");
    let stage = private_dir("bundle")?;
    let dir = stage.join(&name);
    let build = async {
        std::fs::create_dir_all(dir.join("systemd")).map_err(|e| e.to_string())?;
        let mut manifest = bundle::Manifest {
            name: "skiff-server".into(),
            version: version.into(),
            protocol: skiff_core::PROTOCOL,
            os: "linux".into(),
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
        for (f, text) in [("systemd/skiffd.service", UNIT_SKIFFD), ("systemd/skiff-host.service", UNIT_HOST)] {
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
async fn run(target: &SshTarget, options: &SshOptions, command: String, stdin: &[u8]) -> Result<String, String> {
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

fn value<'a>(out: &'a str, key: &str) -> &'a str {
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

/// Uploads `b` to the server, unpacks it as `version` in the managed
/// folder, and starts its skiffd. The same version again changes nothing
/// and starts nothing that runs. `free_kb`: the probe's free space.
pub async fn install(target: &SshTarget, options: &SshOptions, b: &Bundle, version: &str, socket_name: &str, free_kb: u64, name: &str) -> Result<Outcome, String> {
    if !plain_version(version) || !plain_version(socket_name) {
        return Err("This version has a form Skiff does not send.".into());
    }
    // The bundle, its unpacked copy, and the one before.
    let need = (b.bytes.len() as u64 * 4) / 1024;
    if free_kb > 0 && free_kb < need {
        return Err(format!("{name} has no room for Skiff. It needs {} MB in your home folder.", need.div_ceil(1024)));
    }
    let dir = private_dir("upload")?;
    let file = dir.join(&b.name);
    std::fs::write(&file, &b.bytes).map_err(|e| e.to_string())?;
    let hash = sha256(&file).await;
    let _ = std::fs::remove_dir_all(&dir);
    let hash = hash?;
    let upload = r#"sh -c 'umask 077; d="$HOME/.local/share/skiff/server"; mkdir -p "$d" && cat > "$d/.incoming.tar.gz"'"#;
    run(target, options, upload.into(), &b.bytes).await?;
    let command = format!("sh -s -- install {} {} {}", quote(version), quote(&hash), quote(socket_name));
    let out = run(target, options, command, SERVER.as_bytes()).await?;
    match value(&out, "skiff-install") {
        "done" => Ok(started(&out)),
        "bad-hash" | "no-upload" => Err(format!("The upload to {name} did not arrive whole. Nothing changed. Try again.")),
        "no-room" => Err(format!("{name} has no room for Skiff. It needs {} MB in your home folder.", need.div_ceil(1024))),
        "bad-bundle" => Err(format!("The bundle for {name} is not Skiff {version}. Nothing changed.")),
        _ => Err(format!("The install on {name} did not finish. Check again.")),
    }
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
    fn the_step_lines_read_as_an_outcome() {
        let o = started("changed=yes\ncli=linked\nhow=systemd\nlinger=refused\nhost=running\nskiff-start=running\nskiff-install=done\n");
        assert_eq!(o, Outcome { changed: true, how: "systemd".into(), linger: "refused".into(), running: true, host_running: true, cli_linked: true });
        assert!(!started("changed=no\nskiff-start=stopped\n").running);
    }
}

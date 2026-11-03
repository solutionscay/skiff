use std::{sync::atomic::Ordering, time::Duration};
use skiff_client::{Client, host::{local::{connection_status, ProcessControl}}};
use skiff_core::session::SessionState;
use tauri::State;
use super::{ensure_host, process::daemon_binary, App, DaemonStatus, Warning, HELLO_TIMEOUT};
use crate::reload::{self, NoPlan};

pub(crate) enum Check {
    Use,
    /// The old daemon was stopped: connect again, to a fresh one.
    Replaced,
    Hung,
}

/// Compares the daemon with this app. Version order decides, whatever the
/// build: one computer runs one skiffd, and the newer build updates it. An
/// older daemon that can reload is planned for a reload onto the bundled
/// skiffd, which keeps its sessions. The same version stays as it is. Else
/// an older daemon, or an incompatible one that is not newer, with no live
/// session is stopped with SIGTERM and replaced without a word. A newer
/// daemon is never replaced. One that does not stop, or has live sessions,
/// keeps running: this records a warning that says the update is not
/// finished.
pub(crate) async fn check_daemon(app: &App, c: &Client, may_replace: bool) -> Check {
    let info = tokio::time::timeout(HELLO_TIMEOUT, c.daemon_info()).await.ok().and_then(|r| r.ok());
    check_info(app, c, info, may_replace).await
}

/// [`check_daemon`] with the daemon's `pong` already read; `None`: no answer.
pub(super) async fn check_info(app: &App, c: &Client, info: Option<skiff_client::DaemonInfo>, may_replace: bool) -> Check {
    let Some(info) = info else {
        *app.warning.lock().await = Some(Warning {
            kind: "hung",
            message: format!(
                "skiffd accepts connections but did not answer within {}s. \
                 This app cannot reach its sessions. Restarting stops it and starts a new one.",
                HELLO_TIMEOUT.as_secs()
            ),
            sessions: None,
        });
        return Check::Hung;
    };
    let version = info.version.clone();
    // The skiffd this app ships with, not the app version.
    let order = compare_versions(&version, skiff_core::VERSION);
    let same_protocol = info.protocol == skiff_core::PROTOCOL;
    if same_protocol && order.is_eq() {
        *app.warning.lock().await = None;
        return Check::Use;
    }
    // Never a downgrade, and one try per app run. Planning runs the bundled
    // skiffd and replacing signals the daemon: endpoint-local only.
    let control = app.selection().process_control();
    let mut old_build = None;
    let planned = order.is_lt() && !app.reload_tried.load(Ordering::Relaxed);
    if let (true, Some(local)) = (planned, &control) {
        match reload::plan(local, &info).await {
            Ok(plan) => {
                *app.reload.lock().await = Some(plan);
                *app.warning.lock().await = None;
                return Check::Use;
            }
            Err(NoPlan::Stale(bundled)) => old_build = Some(bundled),
            Err(NoPlan::Other(why)) => {
                eprintln!("skiffd {version} does not reload: {why}");
                *app.no_reload.lock().unwrap() = Some(why);
            }
        }
    }
    let why = app.no_reload.lock().unwrap().clone().filter(|_| order.is_lt());
    let live = tokio::time::timeout(HELLO_TIMEOUT, c.list_sessions())
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|v| v.iter().filter(|s| s.state != SessionState::Done).count());
    // The skiffd this app starts is no newer than the one that runs. A
    // restart starts the same build again and asks again: no prompt, and no
    // silent replace. Only a dev build gets here.
    if let Some(bundled) = old_build {
        let why = format!(
            "the skiffd this app starts ({}) is {bundled}, older than the app's {}. Rebuild it: cargo build -p skiffd",
            daemon_binary().display(),
            skiff_core::VERSION
        );
        eprintln!("skiffd {version} does not reload: {why}");
        *app.warning.lock().await = match same_protocol {
            true => None,
            false => Some(restart_warning(&version, info.protocol, live, Some(&why))),
        };
        return Check::Use;
    }
    // Never a downgrade: a newer daemon keeps running, whatever its protocol.
    let stale = !order.is_gt() && (!same_protocol || order.is_lt());
    // Never SIGKILL here: a daemon that does not stop keeps running as it is.
    if let (true, true, Some(0), Some(pid), Some(control)) = (may_replace, stale, live, c.daemon_pid, control) {
        if stop_daemon(&control, pid, false).await {
            *app.replaced.lock().await = Some(version);
            return Check::Replaced;
        }
    }
    *app.warning.lock().await = Some(restart_warning(&version, info.protocol, live, why.as_deref()));
    Check::Use
}

/// The warning for a daemon that differs from this app and keeps running.
/// `why`: the reload that failed, and the daemon carries on as before.
pub(crate) fn restart_warning(version: &str, protocol: u32, live: Option<usize>, why: Option<&str>) -> Warning {
    let ours = skiff_core::VERSION;
    let n = live.unwrap_or(0);
    let running = match n {
        1 => "1 session is running".to_string(),
        n => format!("{n} sessions are running"),
    };
    let ends = "They keep running. A restart stops them, and panes come back as shells in their folders.";
    let failed = why.map(|w| format!("It did not reload: {w}. ")).unwrap_or_default();
    let order = compare_versions(version, ours);
    if protocol != skiff_core::PROTOCOL {
        Warning {
            kind: "protocol",
            message: format!(
                "skiffd {version} speaks another protocol than this app ({ours}). {failed}\
                 Parts of the app fail while it runs. {running}. {ends}"
            ),
            sessions: live,
        }
    } else if !order.is_gt() {
        // The words `skiff status` uses too.
        let why = why.unwrap_or("it did not hand over its sessions");
        Warning {
            kind: "outdated",
            message: skiff_client::update::unfinished(version, ours, why, live, "Restart skiffd"),
            sessions: live,
        }
    } else {
        Warning {
            kind: "newer",
            message: format!(
                "skiffd {version} is newer than this app ({ours}). \
                 They speak the same protocol, so the app keeps using it."
            ),
            sessions: live,
        }
    }
}

pub(crate) use skiff_core::compare_versions;

/// SIGTERM, up to 3 s to exit. With `escalate`, which only a restart the
/// user asked for passes, then SIGKILL and up to 2 s more: a hung daemon may
/// ignore SIGTERM. True once the process is gone: a new daemon refuses to
/// start while the old one still holds the socket.
pub(super) async fn stop_daemon(_local: &ProcessControl, pid: i32, escalate: bool) -> bool {
    // SAFETY: kill and waitpid take plain integers and touch no memory of ours.
    let gone = || unsafe {
        // The app may have started it: reap the zombie, which kill(0) still sees.
        libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) == pid
            || (libc::kill(pid, 0) != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH))
    };
    let steps: &[(i32, u32)] = if escalate { &[(libc::SIGTERM, 30), (libc::SIGKILL, 20)] } else { &[(libc::SIGTERM, 30)] };
    for &(signal, ticks) in steps {
        if unsafe { libc::kill(pid, signal) } != 0 {
            return gone();
        }
        for _ in 0..ticks {
            if gone() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    false
}

pub(crate) async fn status_now(app: &App) -> DaemonStatus {
    let selection = app.selection();
    // A remote Host shows as its destination and socket. Its forward's peer
    // pid is this Mac's `ssh`, never the daemon: it is not shown.
    let socket = match selection.ssh_target() {
        Some(t) => format!("{}:{}", t.destination, selection.remote_socket().unwrap_or_default()),
        None => selection.socket().display().to_string(),
    };
    let local = selection.is_local();
    match ensure_host(app).await {
        Ok((host, spawned)) => {
            let c = host.client();
            let hello = tokio::time::timeout(HELLO_TIMEOUT, connection_status(&host.connection())).await.ok().and_then(|r| r.ok());
            DaemonStatus {
                connected: true,
                version: hello.as_ref().map(|(v, _)| v.clone()),
                protocol: hello.map(|(_, p)| p),
                socket,
                pid: c.daemon_pid.filter(|_| local),
                spawned,
                warning: app.warning.lock().await.clone(),
                replaced: app.replaced.lock().await.clone(),
                reload: app.reload.lock().await.is_some(),
            }
        }
        Err(_) => DaemonStatus {
            connected: false,
            version: None,
            protocol: None,
            socket,
            pid: None,
            spawned: false,
            warning: app.warning.lock().await.clone(),
            replaced: None,
            reload: false,
        },
    }
}

/// Stops the running daemon, which ends its sessions, and starts a new one.
#[tauri::command]
pub(crate) async fn restart_daemon(app: State<'_, App>) -> Result<DaemonStatus, String> {
    {
        let mut guard = app.host.lock().await;
        // A peer pid is the local socket's peer: signal it only for a local endpoint.
        let selection = app.selection();
        let control = selection.process_control().ok_or("skiffd on this Host is not managed by this app")?;
        let pid = match guard.as_ref() {
            Some(host) => host.client().daemon_pid,
            None => selection.connect().await.ok().and_then(|c| c.client().daemon_pid),
        };
        *guard = None;
        *app.live.lock().unwrap() = None;
        if let Some(pid) = pid {
            if !stop_daemon(&control, pid, true).await {
                return Err(format!("skiffd (pid {pid}) did not stop"));
            }
        }
        *app.warning.lock().await = None;
        *app.failed.lock().unwrap() = None;
        *app.no_reload.lock().unwrap() = None;
        *app.reload.lock().await = None;
    }
    for (_, task) in app.subs.lock().await.drain() {
        task.abort();
    }
    daemon_status(app).await
}

#[tauri::command]
pub(crate) async fn daemon_status(app: State<'_, App>) -> Result<DaemonStatus, String> {
    Ok(status_now(&app).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::process::local_control;
    use skiff_client::host::local::HostSelection;
    use std::{path::PathBuf, cmp::Ordering::*};

    #[test]
    fn orders_versions_by_number() {
        assert_eq!(compare_versions("0.3.2", "0.5.0"), Less);
        assert_eq!(compare_versions("0.10.0", "0.9.9"), Greater);
        assert_eq!(compare_versions("0.5.0", "0.5.0"), Equal);
    }

    #[test]
    #[allow(clippy::zombie_processes, reason = "stop_daemon reaps the pid with waitpid")]
    fn kills_a_process_that_ignores_sigterm() {
        let child = std::process::Command::new("sh")
            // exec keeps the ignored TERM disposition and leaves no child
            // sleep behind when the SIGKILL fallback stops this process.
            .args(["-c", "trap '' TERM; exec sleep 30"])
            .spawn()
            .unwrap();
        // Let the shell set its trap first.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        assert!(rt.block_on(stop_daemon(&local_control(), child.id() as i32, true)));
    }

    /// The automatic stop sends SIGTERM only. A process that ignores it
    /// keeps running, and the stop says so.
    #[test]
    fn the_automatic_stop_never_kills() {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "trap '' TERM; exec sleep 30"])
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let stopped = rt.block_on(stop_daemon(&local_control(), child.id() as i32, false));
        let running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(!stopped);
        assert!(running, "the process was stopped");
    }

    /// A stand-in daemon on a private socket: it answers `ping` as
    /// `version` on `protocol` and lists no session. The connection names
    /// `pid` as the daemon, a process the test owns, so a stop shows on it.
    /// With `reloads`, the pong says it can hand over (state 2, its pid).
    async fn fake_daemon(dir: &std::path::Path, version: &'static str, protocol: u32, pid: u32, reloads: bool) -> skiff_client::Client {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let socket = dir.join(format!("{version}.sock"));
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (rd, mut wr) = stream.into_split();
            let mut lines = tokio::io::BufReader::new(rd).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let req: serde_json::Value = serde_json::from_str(&line).unwrap();
                let id = req["id"].clone();
                let reply = match req["cmd"].as_str() {
                    Some("ping") if reloads => serde_json::json!({"id": id, "type": "pong", "version": version, "protocol": protocol, "reload_state": 2, "pid": pid}),
                    Some("ping") => serde_json::json!({"id": id, "type": "pong", "version": version, "protocol": protocol}),
                    Some("list_sessions") => serde_json::json!({"id": id, "type": "sessions", "sessions": []}),
                    other => serde_json::json!({"id": id, "type": "error", "message": format!("{other:?}")}),
                };
                let mut out = serde_json::to_vec(&reply).unwrap();
                out.push(b'\n');
                wr.write_all(&out).await.unwrap();
            }
        });
        let mut c = skiff_client::Client::connect(&socket).await.unwrap();
        c.daemon_pid = Some(pid as i32);
        c
    }

    /// Never a downgrade: a newer daemon on another protocol with no live
    /// session keeps running, and the app does not replace it. An older one
    /// in the same place is stopped and replaced, which shows the stop works.
    #[test]
    fn a_newer_daemon_is_never_replaced() {
        let dir = std::env::temp_dir().join(format!("skiff-app-newer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let other = skiff_core::PROTOCOL + 1;
        for (version, replaced) in [("99.0.0", false), ("0.0.1", true)] {
            let mut stand_in = std::process::Command::new("sleep").arg("30").spawn().unwrap();
            let app = super::App::default();
            let check = rt.block_on(async {
                let c = fake_daemon(&dir, version, other, stand_in.id(), false).await;
                super::check_daemon(&app, &c, true).await
            });
            // The stop reaps what it ends, so a gone child reads as ECHILD.
            let running = stand_in.try_wait().is_ok_and(|s| s.is_none());
            let _ = stand_in.kill();
            let _ = stand_in.wait();
            assert_eq!(matches!(check, super::Check::Replaced), replaced, "skiffd {version}");
            assert_eq!(running, !replaced, "skiffd {version}: signalled");
            assert_eq!(rt.block_on(app.replaced.lock()).is_some(), replaced);
            if !replaced {
                assert_eq!(rt.block_on(app.warning.lock()).as_ref().map(|w| w.kind), Some("protocol"));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Version order decides, whatever the build. An older daemon that can
    /// hand over is planned for a reload onto the bundled skiffd, with no
    /// warning. A newer one is left alone, with the "newer" notice. The
    /// same version is used as it is. Nothing is signalled in any case.
    #[test]
    fn an_older_daemon_reloads_and_a_newer_one_is_left_alone() {
        let _env = crate::reload::tests::ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("skiff-app-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The bundled skiffd: it answers --reload-info as this build's version.
        let bin = dir.join("skiffd");
        std::fs::write(&bin, format!("#!/bin/sh\necho '{{\"version\":\"{}\",\"handover\":[2],\"fingerprint\":\"x\"}}'\n", skiff_core::VERSION)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old_daemon = std::env::var_os("SKIFF_DAEMON");
        std::env::set_var("SKIFF_DAEMON", &bin);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut outcome = Vec::new();
        for (version, want) in [("0.0.1", "reload"), ("99.0.0", "newer"), (skiff_core::VERSION, "same")] {
            let mut stand_in = std::process::Command::new("sleep").arg("30").spawn().unwrap();
            let app = super::App::default();
            let check = rt.block_on(async {
                let c = fake_daemon(&dir, version, skiff_core::PROTOCOL, stand_in.id(), true).await;
                super::check_daemon(&app, &c, true).await
            });
            let running = stand_in.try_wait().is_ok_and(|s| s.is_none());
            let _ = stand_in.kill();
            let _ = stand_in.wait();
            let planned = rt.block_on(app.reload.lock()).is_some();
            let warning = rt.block_on(app.warning.lock()).as_ref().map(|w| w.kind);
            outcome.push((want, matches!(check, super::Check::Use), running, planned, warning));
        }
        match old_daemon {
            Some(v) => std::env::set_var("SKIFF_DAEMON", v),
            None => std::env::remove_var("SKIFF_DAEMON"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(outcome, [("reload", true, true, true, None), ("newer", true, true, false, Some("newer")), ("same", true, true, false, None)]);
    }

    /// A missing daemon is started once, from the bundled skiffd, and the
    /// connect loop keeps asking its socket. Here the stand-in only leaves a
    /// mark and never listens, so the loop ends with the "cannot reach" error.
    #[test]
    fn a_missing_daemon_is_spawned() {
        let _env = crate::reload::tests::ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = PathBuf::from("/tmp").join(format!("sd-spawn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mark = dir.join("started");
        let bin = dir.join("skiffd");
        std::fs::write(&bin, format!("#!/bin/sh\ntouch {}\n", mark.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old_daemon = std::env::var_os("SKIFF_DAEMON");
        let old_socket = std::env::var_os("SKIFF_SOCKET");
        let socket = dir.join("none.sock");
        std::env::set_var("SKIFF_DAEMON", &bin);
        std::env::set_var("SKIFF_SOCKET", &socket);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let app = super::App::default();
        app.set_selection(HostSelection::new(socket.clone(), dir.join("local-host-id")));
        let result = rt.block_on(super::ensure_host(&app));
        for (var, old) in [("SKIFF_DAEMON", old_daemon), ("SKIFF_SOCKET", old_socket)] {
            match old {
                Some(v) => std::env::set_var(var, v),
                None => std::env::remove_var(var),
            }
        }
        let started = mark.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(started, "the bundled skiffd was not started");
        assert!(result.err().is_some_and(|e| e.starts_with("cannot reach skiffd")));
    }

    /// #182: the bundled skiffd cannot read the daemon's handover. No reload
    /// is planned, and the warning says why and what a restart does, in the
    /// words `skiff status` uses.
    #[test]
    fn a_blocked_handover_says_why() {
        let _env = crate::reload::tests::ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("skiff-app-blocked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("skiffd");
        std::fs::write(&bin, format!("#!/bin/sh\necho '{{\"version\":\"{}\",\"handover\":[3],\"fingerprint\":\"x\"}}'\n", skiff_core::VERSION)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old_daemon = std::env::var_os("SKIFF_DAEMON");
        std::env::set_var("SKIFF_DAEMON", &bin);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let app = super::App::default();
        rt.block_on(async {
            let c = fake_daemon(&dir, "0.0.1", skiff_core::PROTOCOL, std::process::id(), true).await;
            super::check_daemon(&app, &c, false).await
        });
        match old_daemon {
            Some(v) => std::env::set_var("SKIFF_DAEMON", v),
            None => std::env::remove_var("SKIFF_DAEMON"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(rt.block_on(app.reload.lock()).is_none());
        let w = rt.block_on(app.warning.lock()).clone().unwrap();
        assert_eq!(w.kind, "outdated");
        assert!(w.message.contains("handover 2") && w.message.contains("Restart skiffd stops them"), "{}", w.message);
    }
}
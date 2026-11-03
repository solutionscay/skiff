use super::*;

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

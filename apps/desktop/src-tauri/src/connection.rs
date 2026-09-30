use std::{collections::HashMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use serde::Serialize;
use skiff_client::Client;
use skiff_core::{socket::socket_path, session::SessionState};
use tauri::{async_runtime::JoinHandle, State};
use tokio::sync::Mutex;
use crate::streaming::Flow;

#[derive(Default)]
pub(crate) struct App {
    pub(crate) client: Mutex<Option<Arc<Client>>>,
    /// The same connection, for keystrokes, which must not wait on a lock.
    pub(crate) live: std::sync::Mutex<Option<Arc<Client>>>,
    pub(crate) subs: Mutex<HashMap<String, JoinHandle<()>>>,
    /// Per session: how far the page's xterm is behind its stream.
    pub(crate) flows: std::sync::Mutex<HashMap<String, Arc<Flow>>>,
    /// Set when the daemon speaks another protocol and could not be replaced.
    pub(crate) warning: Mutex<Option<String>>,
}

#[derive(Serialize, Clone)]
pub(crate) struct DaemonStatus {
    connected: bool,
    version: Option<String>,
    socket: String,
    spawned: bool,
    warning: Option<String>,
}

fn daemon_binary() -> PathBuf {
    if let Ok(p) = std::env::var("SKIFF_DAEMON") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        let beside = exe.with_file_name("skiffd");
        if beside.exists() {
            return beside;
        }
    }
    PathBuf::from("skiffd")
}

fn spawn_daemon() -> anyhow::Result<()> {
    let log = socket_path().with_file_name("skiffd.log");
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Keep the last daemon's log: it saw what the new one restores.
    let _ = std::fs::rename(&log, log.with_extension("log.1"));
    let log = std::fs::File::create(&log)?;
    let mut cmd = std::process::Command::new(daemon_binary());
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()?;
    Ok(())
}

/// Reuses a live connection, else connects, else starts the daemon and connects.
/// A live connection costs no round trip: every keystroke comes through here.
pub(crate) async fn ensure_client(app: &App) -> Result<(Arc<Client>, bool), String> {
    let mut guard = app.client.lock().await;
    if let Some(c) = guard.as_ref() {
        if !c.is_closed() {
            return Ok((c.clone(), false));
        }
    }
    *guard = None;
    let path = socket_path();
    let mut spawned = false;
    let mut last = String::new();
    let mut replaced = 0;
    for attempt in 0..40 {
        match Client::connect(&path).await {
            Ok(c) => {
                if replaced < 1 && check_protocol(app, &c).await {
                    // An older daemon with nothing running: replace it.
                    replaced += 1;
                    spawned = false;
                    continue;
                }
                let c = Arc::new(c);
                *guard = Some(c.clone());
                *app.live.lock().unwrap() = Some(c.clone());
                return Ok((c, spawned));
            }
            Err(e) => {
                last = format!("{e:#}");
                if attempt == 0 || (replaced > 0 && !spawned) {
                    spawn_daemon().map_err(|e| format!("start skiffd: {e:#}"))?;
                    spawned = true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(format!("cannot reach skiffd at {}: {last}", path.display()))
}

/// True when the daemon speaks another protocol, runs no live session, and
/// was stopped so a fresh one can start. Otherwise records a warning.
async fn check_protocol(app: &App, c: &Client) -> bool {
    let protocol = c.hello().await.map(|(_, p)| p).unwrap_or(0);
    if protocol == skiff_core::PROTOCOL {
        *app.warning.lock().await = None;
        return false;
    }
    let live = c
        .list_sessions()
        .await
        .map(|v| v.iter().filter(|s| s.state != SessionState::Done).count())
        .unwrap_or(0);
    if let (0, Some(pid)) = (live, c.daemon_pid) {
        if stop_daemon(pid).await {
            return true;
        }
    }
    *app.warning.lock().await = Some(format!(
        "skiffd is older than this app and runs {live} session(s). \
         New features fail until it restarts. Restarting ends those sessions."
    ));
    false
}

/// SIGTERM, then wait up to 3 s for the process to go.
async fn stop_daemon(pid: i32) -> bool {
    let sent = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !sent {
        return false;
    }
    for _ in 0..30 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    true
}

/// Stops the running daemon, which ends its sessions, and starts a new one.
#[tauri::command]
pub(crate) async fn restart_daemon(app: State<'_, App>) -> Result<DaemonStatus, String> {
    {
        let mut guard = app.client.lock().await;
        let pid = match guard.as_ref() {
            Some(c) => c.daemon_pid,
            None => Client::connect(&socket_path()).await.ok().and_then(|c| c.daemon_pid),
        };
        *guard = None;
        *app.live.lock().unwrap() = None;
        if let Some(pid) = pid {
            stop_daemon(pid).await;
        }
        *app.warning.lock().await = None;
    }
    for (_, task) in app.subs.lock().await.drain() {
        task.abort();
    }
    daemon_status(app).await
}

pub(crate) fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
pub(crate) async fn daemon_status(app: State<'_, App>) -> Result<DaemonStatus, String> {
    let socket = socket_path().display().to_string();
    match ensure_client(&app).await {
        Ok((c, spawned)) => Ok(DaemonStatus {
            connected: true,
            version: c.ping().await.ok(),
            socket,
            spawned,
            warning: app.warning.lock().await.clone(),
        }),
        Err(_) => Ok(DaemonStatus {
            connected: false,
            version: None,
            socket,
            spawned: false,
            warning: None,
        }),
    }
}

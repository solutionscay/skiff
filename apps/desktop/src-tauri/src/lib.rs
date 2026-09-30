//! The desktop app is a client of `skiffd`. It owns no PTY.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use skiff_client::Client;
use skiff_core::{
    config::{Appearance, FolderInfo},
    group::Group,
    project::{AgentInfo, Project, Worktree},
    protocol::Event,
    session::{SessionInfo, SessionSpec, SessionState},
    socket::socket_path,
};
use tauri::{
    async_runtime::JoinHandle,
    ipc::{Channel, InvokeResponseBody},
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    AppHandle, Emitter, Manager, State,
};
use tokio::sync::{broadcast, Mutex, Notify};

#[derive(Default)]
struct App {
    client: Mutex<Option<Arc<Client>>>,
    /// The same connection, for keystrokes, which must not wait on a lock.
    live: std::sync::Mutex<Option<Arc<Client>>>,
    subs: Mutex<HashMap<String, JoinHandle<()>>>,
    /// Per session: how far the page's xterm is behind its stream.
    flows: std::sync::Mutex<HashMap<String, Arc<Flow>>>,
    /// Set when the daemon speaks another protocol and could not be replaced.
    warning: Mutex<Option<String>>,
}

#[derive(Serialize, Clone)]
struct DaemonStatus {
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
async fn ensure_client(app: &App) -> Result<(Arc<Client>, bool), String> {
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
async fn restart_daemon(app: State<'_, App>) -> Result<DaemonStatus, String> {
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

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
async fn daemon_status(app: State<'_, App>) -> Result<DaemonStatus, String> {
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

#[tauri::command]
async fn list_sessions(app: State<'_, App>) -> Result<Vec<SessionInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_sessions().await.map_err(err)
}

#[tauri::command]
async fn create_session(app: State<'_, App>, spec: SessionSpec) -> Result<SessionInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    c.create_session(spec).await.map_err(err)
}

/// Not async: it runs on the main thread, one call after another, so
/// keystrokes reach the PTY in order. It waits for no lock and no reply.
#[tauri::command]
fn pty_write(handle: AppHandle, app: State<'_, App>, session: String, data: String) -> Result<(), String> {
    let live = app.live.lock().unwrap().clone().filter(|c| !c.is_closed());
    if let Some(c) = live {
        return c.write_now(&session, data.into_bytes()).map_err(err);
    }
    // No connection yet. Rare: connect, then write.
    tauri::async_runtime::spawn(async move {
        let app = handle.state::<App>();
        if let Ok((c, _)) = ensure_client(&app).await {
            let _ = c.write(&session, data.into_bytes()).await;
        }
    });
    Ok(())
}

#[tauri::command]
async fn pty_resize(
    app: State<'_, App>,
    session: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.resize(&session, cols, rows).await.map_err(err)
}

#[tauri::command]
async fn set_session_theme(
    app: State<'_, App>,
    session: String,
    theme: Option<String>,
) -> Result<SessionInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_session_theme(&session, theme).await.map_err(err)
}

#[tauri::command]
async fn rename_session(
    app: State<'_, App>,
    session: String,
    name: String,
) -> Result<SessionInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    c.rename_session(&session, &name).await.map_err(err)
}

#[tauri::command]
async fn kill_session(app: State<'_, App>, session: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.kill(&session).await.map_err(err)
}

#[tauri::command]
async fn list_projects(app: State<'_, App>) -> Result<Vec<Project>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_projects().await.map_err(err)
}

#[tauri::command]
async fn add_worktree(
    app: State<'_, App>,
    project: String,
    branch: String,
    base: Option<String>,
) -> Result<Worktree, String> {
    let (c, _) = ensure_client(&app).await?;
    c.add_worktree(&project, &branch, base).await.map_err(err)
}

#[tauri::command]
async fn remove_worktree(
    app: State<'_, App>,
    project: String,
    path: PathBuf,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.remove_worktree(&project, path).await.map_err(err)
}

#[tauri::command]
async fn add_project(
    app: State<'_, App>,
    path: PathBuf,
    name: Option<String>,
    short: Option<String>,
    color: Option<String>,
    icon: Option<String>,
    agents: Option<Vec<String>>,
) -> Result<Project, String> {
    let (c, _) = ensure_client(&app).await?;
    c.add_project(path, name, short, color, icon, agents.unwrap_or_default())
        .await
        .map_err(err)
}

/// Where the keystroke latency trace goes: `SKIFF_TRACE=<file>`, or
/// latency.jsonl in the app's log folder.
fn trace_file(handle: &AppHandle) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("SKIFF_TRACE") {
        return Ok(PathBuf::from(p));
    }
    let dir = handle.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("latency.jsonl"))
}

/// The trace file, and whether SKIFF_TRACE asks for tracing from launch.
#[tauri::command]
fn trace_info(handle: AppHandle) -> Result<(String, bool), String> {
    let path = trace_file(&handle)?;
    Ok((path.display().to_string(), std::env::var_os("SKIFF_TRACE").is_some()))
}

/// Appends JSON lines from the page's latency trace.
#[tauri::command]
fn trace_write(handle: AppHandle, lines: Vec<String>) -> Result<(), String> {
    use std::io::Write;
    let path = trace_file(&handle)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    for line in lines {
        writeln!(f, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn config_path() -> String {
    skiff_core::config::config_path().display().to_string()
}

/// Opens projects.toml in the default editor. From Rust, because the opener's
/// `**` scope does not match hidden folders such as ~/.config.
#[tauri::command]
fn open_config(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = skiff_core::config::config_path();
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_keys(app: State<'_, App>) -> Result<std::collections::BTreeMap<String, String>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.keys().await.map_err(err)
}

/// Memory in bytes: this app with its WebKit children, and skiffd without the
/// sessions it runs. Proportional set size, so shared libraries count once.
/// `None` off Linux.
#[tauri::command]
fn memory_usage() -> Option<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        let procs: Vec<(u32, u32, String)> = std::fs::read_dir("/proc")
            .ok()?
            .filter_map(|e| {
                let pid: u32 = e.ok()?.file_name().to_str()?.parse().ok()?;
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                // `pid (comm) state ppid ...`; comm may hold spaces and parens.
                let (head, tail) = stat.rsplit_once(')')?;
                let comm = head.split_once('(')?.1.to_string();
                let ppid = tail.split_whitespace().nth(1)?.parse().ok()?;
                Some((pid, ppid, comm))
            })
            .collect();
        let me = std::process::id();
        let app = std::iter::once(me)
            .chain(procs.iter().filter(|p| p.1 == me).map(|p| p.0))
            .map(pss)
            .sum();
        let daemon = procs.iter().filter(|p| p.2 == "skiffd").map(|p| pss(p.0)).sum();
        Some((app, daemon))
    }
    #[cfg(not(target_os = "linux"))]
    None
}

#[cfg(target_os = "linux")]
fn pss(pid: u32) -> u64 {
    let kb = |file: &str, key: &str| {
        let text = std::fs::read_to_string(format!("/proc/{pid}/{file}")).ok()?;
        let line = text.lines().find(|l| l.starts_with(key))?;
        line[key.len()..].trim().trim_end_matches("kB").trim().parse::<u64>().ok()
    };
    kb("smaps_rollup", "Pss:").or_else(|| kb("status", "VmRSS:")).unwrap_or(0) * 1024
}

#[tauri::command]
async fn get_appearance(app: State<'_, App>) -> Result<Appearance, String> {
    let (c, _) = ensure_client(&app).await?;
    c.appearance(None).await.map_err(err)
}

#[tauri::command]
async fn set_appearance(app: State<'_, App>, theme: Option<String>) -> Result<Appearance, String> {
    let (c, _) = ensure_client(&app).await?;
    c.appearance(Some(theme)).await.map_err(err)
}

#[tauri::command]
async fn set_font_size(app: State<'_, App>, size: Option<u8>) -> Result<Appearance, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_font_size(size).await.map_err(err)
}

#[tauri::command]
async fn list_themes(app: State<'_, App>) -> Result<Vec<skiff_core::theme::TerminalTheme>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_themes().await.map_err(err)
}

#[tauri::command]
async fn read_icon(app: State<'_, App>, path: PathBuf) -> Result<Option<String>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.read_icon(path).await.map_err(err)
}

#[tauri::command]
async fn set_project_icon(
    app: State<'_, App>,
    project: String,
    icon: Option<String>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_icon(project, icon).await.map_err(err)
}

#[tauri::command]
async fn reorder_projects(app: State<'_, App>, order: Vec<String>) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.reorder_projects(order).await.map_err(err)
}

#[tauri::command]
async fn set_project_background(
    app: State<'_, App>,
    project: String,
    background: Option<String>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_background(project, background).await.map_err(err)
}

/// A background image's raw bytes. The page turns them into a blob URL.
#[tauri::command]
async fn read_image(path: PathBuf) -> Result<tauri::ipc::Response, String> {
    let bytes = tokio::fs::read(&path).await.map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// One folder for the sidebar's file tree. Read here, not in the daemon:
/// the tree is a view, and the daemon owns only sessions.
#[tauri::command]
async fn list_dir(path: PathBuf) -> Result<Vec<skiff_core::files::Entry>, String> {
    tokio::task::spawn_blocking(move || skiff_core::files::list_dir(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// The changed files of one worktree, for the sidebar's Changes section.
#[tauri::command]
async fn git_changes(path: PathBuf) -> Result<Vec<skiff_core::git::Change>, String> {
    tokio::task::spawn_blocking(move || skiff_core::git::changes(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// A diff for the peek, printed by the command under `[open] diff`.
#[tauri::command]
async fn git_diff(path: PathBuf, file: Option<String>, cols: u16) -> Result<String, String> {
    let command = skiff_core::config::load().ok().and_then(|c| c.open.diff);
    tokio::task::spawn_blocking(move || skiff_core::git::diff_text(&path, file.as_deref(), command.as_deref(), cols))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// The `[open]` commands, empty for a default, and the default diff command.
#[derive(serde::Serialize)]
struct OpenSettings {
    diff: String,
    text: String,
    markdown: String,
    html: String,
    default_diff: &'static str,
}

fn open_settings_now() -> Result<OpenSettings, String> {
    let o = skiff_core::config::load().map_err(err)?.open;
    Ok(OpenSettings {
        diff: o.diff.unwrap_or_default(),
        text: o.text.unwrap_or_default(),
        markdown: o.markdown.unwrap_or_default(),
        html: o.html.unwrap_or_default(),
        default_diff: skiff_core::git::DEFAULT_DIFF,
    })
}

#[tauri::command]
fn open_settings() -> Result<OpenSettings, String> {
    open_settings_now()
}

/// Sets one `[open]` command. Empty: the default.
#[tauri::command]
fn set_open(key: String, command: String) -> Result<OpenSettings, String> {
    let c = command.trim();
    skiff_core::config::set_open(&key, (!c.is_empty()).then_some(c)).map_err(err)?;
    open_settings_now()
}

/// Opens a file with its `[open]` command, else in its default app. From
/// Rust, like open_config, because the opener's `**` scope does not match
/// hidden folders such as .github.
#[tauri::command]
async fn open_file(app: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    let open = skiff_core::config::load().map(|c| c.open).unwrap_or_default();
    let file = path.clone();
    let cmd = tokio::task::spawn_blocking(move || skiff_core::open::command_for(&file, &open))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(cmd) = cmd {
        return tokio::task::spawn_blocking(move || skiff_core::open::launch(&cmd, &path))
            .await
            .map_err(|e| e.to_string())?
            .map_err(err);
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Projects with `files = true`. Read from projects.toml here, so an older
/// daemon that does not send the key still works.
#[tauri::command]
fn files_projects() -> Result<Vec<String>, String> {
    let cfg = skiff_core::config::load().map_err(err)?;
    Ok(cfg.projects.into_iter().filter(|p| p.files).map(|p| p.name).collect())
}

/// Projects with `changes = false`.
#[tauri::command]
fn hidden_changes_projects() -> Result<Vec<String>, String> {
    let cfg = skiff_core::config::load().map_err(err)?;
    Ok(cfg.projects.into_iter().filter(|p| p.changes == Some(false)).map(|p| p.name).collect())
}

#[tauri::command]
fn set_project_changes(project: String, on: bool) -> Result<(), String> {
    skiff_core::config::set_project_changes(&project, on).map_err(err)
}

/// Each project's own theme id, as `[name, id]`.
#[tauri::command]
fn project_themes() -> Result<Vec<(String, String)>, String> {
    let cfg = skiff_core::config::load().map_err(err)?;
    Ok(cfg.projects.into_iter().filter_map(|p| p.theme.map(|t| (p.name, t))).collect())
}

#[tauri::command]
fn set_project_theme(project: String, theme: Option<String>) -> Result<(), String> {
    skiff_core::config::set_project_theme(&project, theme.as_deref()).map_err(err)
}

#[tauri::command]
fn set_project_files(project: String, on: bool) -> Result<(), String> {
    skiff_core::config::set_project_files(&project, on).map_err(err)
}

#[tauri::command]
fn reveal_file(app: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_project_color(
    app: State<'_, App>,
    project: String,
    color: String,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_color(project, color).await.map_err(err)
}

#[tauri::command]
async fn set_project_closed(app: State<'_, App>, project: String, closed: bool) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_closed(project, closed).await.map_err(err)
}

#[tauri::command]
async fn remove_project(app: State<'_, App>, project: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.remove_project(project).await.map_err(err)
}

#[tauri::command]
async fn inspect_folder(app: State<'_, App>, path: PathBuf) -> Result<FolderInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    // An outdated daemon does not know this request. Say so at once.
    if let Some(w) = app.warning.lock().await.clone() {
        return Err(w);
    }
    c.inspect_folder(path).await.map_err(err)
}

#[tauri::command]
async fn list_agents(app: State<'_, App>) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_agents().await.map_err(err)
}

#[tauri::command]
async fn set_agent_command(
    app: State<'_, App>,
    agent: String,
    command: String,
) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_agent_command(agent, command).await.map_err(err)
}

#[tauri::command]
async fn set_agents(app: State<'_, App>, enabled: Vec<String>) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_agents(enabled).await.map_err(err)
}

#[tauri::command]
async fn list_groups(app: State<'_, App>) -> Result<Vec<Group>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_groups().await.map_err(err)
}

#[tauri::command]
async fn save_group(app: State<'_, App>, group: Group) -> Result<Group, String> {
    let (c, _) = ensure_client(&app).await?;
    c.save_group(group).await.map_err(err)
}

#[tauri::command]
async fn delete_group(app: State<'_, App>, group: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.delete_group(&group).await.map_err(err)
}

/// One write to the page holds at most this much, so xterm can yield to the
/// page between pieces instead of parsing one long chunk.
const PIECE: usize = 64 * 1024;
/// Unparsed bytes past this, and live output is dropped instead of queued.
/// xterm discards writes itself past 50 MB, and a deep queue is stale anyway.
const BEHIND_HIGH: usize = 4 * 1024 * 1024;
/// Unparsed bytes under this, and a pane that dropped output redraws from a
/// fresh snapshot.
const BEHIND_LOW: usize = 1024 * 1024;

/// Bytes sent to one page stream that its xterm has not parsed yet.
struct Flow {
    /// The page's stream number, so an ack from an older stream is ignored.
    stream: u32,
    unparsed: AtomicUsize,
    parsed: Notify,
}

impl Flow {
    fn send(&self, on_output: &Channel<InvokeResponseBody>, data: Vec<u8>) -> tauri::Result<()> {
        if data.len() <= PIECE {
            self.unparsed.fetch_add(data.len(), Ordering::Relaxed);
            return on_output.send(InvokeResponseBody::Raw(data));
        }
        for piece in data.chunks(PIECE) {
            self.unparsed.fetch_add(piece.len(), Ordering::Relaxed);
            on_output.send(InvokeResponseBody::Raw(piece.to_vec()))?;
        }
        Ok(())
    }
}

/// Streams raw PTY bytes for one session, starting with a screen snapshot.
/// Replaces any earlier subscription. Output the page cannot keep up with is
/// dropped; once it catches up, a snapshot redraws the screen.
#[tauri::command]
async fn subscribe_output(
    app: State<'_, App>,
    session: String,
    stream: u32,
    on_output: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    // Listen first: the snapshot can arrive before the subscribe reply.
    let mut rx = c.output(&session);
    c.subscribe(&session).await.map_err(err)?;
    let flow = Arc::new(Flow {
        stream,
        unparsed: AtomicUsize::new(0),
        parsed: Notify::new(),
    });
    app.flows.lock().unwrap().insert(session.clone(), flow.clone());
    let sid = session.clone();
    // Weak, so a replaced client still drops and closes `rx`.
    let client = Arc::downgrade(&c);
    let task = tauri::async_runtime::spawn(async move {
        // Output from an earlier subscription predates the snapshot. Skip it.
        let mut live = false;
        // Output was dropped because the page fell behind.
        let mut behind = false;
        loop {
            let event = tokio::select! {
                event = rx.recv() => event,
                _ = flow.parsed.notified(), if behind => {
                    if flow.unparsed.load(Ordering::Relaxed) > BEHIND_LOW {
                        continue;
                    }
                    behind = false;
                    live = false;
                    rx.clear();
                    let Some(c) = client.upgrade() else { break };
                    if c.subscribe(&sid).await.is_err() {
                        break;
                    }
                    continue;
                }
            };
            match event {
                Ok(Event::Snapshot { session, data, .. }) if session == sid => {
                    live = true;
                    if flow.send(&on_output, data).is_err() {
                        break;
                    }
                }
                Ok(Event::Output { session, data }) if live && !behind && session == sid => {
                    if flow.unparsed.load(Ordering::Relaxed) >= BEHIND_HIGH {
                        behind = true;
                        continue;
                    }
                    if flow.send(&on_output, data).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Output was lost. Ask for a fresh snapshot and skip
                    // everything before it. A pane that is behind asks
                    // once it catches up.
                    live = false;
                    rx.clear();
                    if behind {
                        continue;
                    }
                    let Some(c) = client.upgrade() else { break };
                    if c.subscribe(&sid).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    if let Some(old) = app.subs.lock().await.insert(session, task) {
        old.abort();
    }
    Ok(())
}

/// The page's xterm parsed `bytes` more of stream `stream`.
#[tauri::command]
fn ack_output(app: State<'_, App>, session: String, stream: u32, bytes: usize) {
    let Some(flow) = app.flows.lock().unwrap().get(&session).cloned() else { return };
    if flow.stream != stream {
        return;
    }
    let _ = flow
        .unparsed
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_sub(bytes)));
    flow.parsed.notify_one();
}

#[tauri::command]
async fn unsubscribe_output(app: State<'_, App>, session: String) -> Result<(), String> {
    app.flows.lock().unwrap().remove(&session);
    if let Some(task) = app.subs.lock().await.remove(&session) {
        task.abort();
    }
    let (c, _) = ensure_client(&app).await?;
    c.unsubscribe(&session).await.map_err(err)
}

/// Streams every non-output event: state changes, exits, created, removed.
/// The client routes output elsewhere, so a busy terminal cannot make this lag.
#[tauri::command]
async fn subscribe_events(app: State<'_, App>, on_event: Channel<Event>) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    let mut rx = c.events();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if on_event.send(e).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

#[derive(Deserialize)]
struct MenuSpec {
    title: String,
    items: Vec<MenuItemSpec>,
}

#[derive(Deserialize)]
struct MenuItemSpec {
    /// None draws a separator.
    id: Option<String>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    accel: String,
    #[serde(default)]
    enabled: bool,
}

/// The menu bar. The page owns the command list and sends it whole on every
/// change; a click comes back as `skiff:menu` with the item's id.
/// Closes the app. Sessions live in skiffd and keep running.
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_menu(app: AppHandle, menus: Vec<MenuSpec>) -> Result<(), String> {
    let mut bar = MenuBuilder::new(&app);
    for m in &menus {
        let mut sub = SubmenuBuilder::new(&app, &m.title);
        for it in &m.items {
            let Some(id) = &it.id else {
                sub = sub.separator();
                continue;
            };
            let item = |accel: &str| {
                let b = MenuItemBuilder::with_id(id, &it.label).enabled(it.enabled);
                if accel.is_empty() { b.build(&app) } else { b.accelerator(accel).build(&app) }
            };
            // A key the menu cannot parse stays with the page, which still handles it.
            let item = item(&it.accel).or_else(|_| item("")).map_err(|e| e.to_string())?;
            sub = sub.item(&item);
        }
        bar = bar.item(&sub.build().map_err(|e| e.to_string())?);
    }
    app.set_menu(bar.build().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK paints a blank window on NVIDIA under Wayland when it renders
    // through DMA-BUF. Turn that path off there, unless the user chose
    // otherwise. Intel and AMD keep it: it is the faster path.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
        && std::path::Path::new("/proc/driver/nvidia").exists()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(App::default())
        .on_menu_event(|app, event| {
            let _ = app.emit("skiff:menu", event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            set_menu,
            quit_app,
            daemon_status,
            list_sessions,
            create_session,
            pty_write,
            pty_resize,
            kill_session,
            rename_session,
            set_session_theme,
            list_projects,
            add_worktree,
            remove_worktree,
            add_project,
            restart_daemon,
            inspect_folder,
            set_project_icon,
            set_project_color,
            set_project_closed,
            remove_project,
            set_project_background,
            read_image,
            list_dir,
            git_changes,
            git_diff,
            open_settings,
            set_open,
            open_file,
            reveal_file,
            files_projects,
            hidden_changes_projects,
            set_project_changes,
            set_project_files,
            project_themes,
            set_project_theme,
            reorder_projects,
            read_icon,
            list_themes,
            get_appearance,
            memory_usage,
            get_keys,
            config_path,
            trace_info,
            trace_write,
            open_config,
            set_appearance,
            set_font_size,
            list_agents,
            set_agents,
            set_agent_command,
            list_groups,
            save_group,
            delete_group,
            subscribe_output,
            ack_output,
            unsubscribe_output,
            subscribe_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

//! The desktop app is a client of `skiffd`. It owns no PTY.

use std::{collections::HashMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use skiff_client::Client;
use skiff_core::{
    config::FolderInfo,
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
    AppHandle, Emitter, State,
};
use tokio::sync::{broadcast, Mutex};

#[derive(Default)]
struct App {
    client: Mutex<Option<Arc<Client>>>,
    subs: Mutex<HashMap<String, JoinHandle<()>>>,
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

#[tauri::command]
async fn pty_write(app: State<'_, App>, session: String, data: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.write(&session, data.into_bytes()).await.map_err(err)
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

#[tauri::command]
fn config_path() -> String {
    skiff_core::config::config_path().display().to_string()
}

#[tauri::command]
async fn get_keys(app: State<'_, App>) -> Result<std::collections::BTreeMap<String, String>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.keys().await.map_err(err)
}

#[tauri::command]
async fn get_appearance(app: State<'_, App>) -> Result<Option<String>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.appearance(None).await.map_err(err)
}

#[tauri::command]
async fn set_appearance(app: State<'_, App>, theme: Option<String>) -> Result<Option<String>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.appearance(Some(theme)).await.map_err(err)
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

/// Streams raw PTY bytes for one session, starting with a screen snapshot.
/// Replaces any earlier subscription.
#[tauri::command]
async fn subscribe_output(
    app: State<'_, App>,
    session: String,
    on_output: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    // Listen first: the snapshot arrives before the subscribe reply.
    let mut rx = c.events();
    c.subscribe(&session).await.map_err(err)?;
    let sid = session.clone();
    // Weak, so a replaced client still drops and closes `rx`.
    let client = Arc::downgrade(&c);
    let task = tauri::async_runtime::spawn(async move {
        // Output from an earlier subscription predates the snapshot. Skip it.
        let mut live = false;
        loop {
            match rx.recv().await {
                Ok(Event::Snapshot { session, data, .. }) if session == sid => {
                    live = true;
                    if on_output.send(InvokeResponseBody::Raw(data)).is_err() {
                        break;
                    }
                }
                Ok(Event::Output { session, data }) if live && session == sid => {
                    if on_output.send(InvokeResponseBody::Raw(data)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Output was lost. Ask for a fresh snapshot and skip
                    // everything before it.
                    live = false;
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

#[tauri::command]
async fn unsubscribe_output(app: State<'_, App>, session: String) -> Result<(), String> {
    if let Some(task) = app.subs.lock().await.remove(&session) {
        task.abort();
    }
    let (c, _) = ensure_client(&app).await?;
    c.unsubscribe(&session).await.map_err(err)
}

/// Streams every non-output event: state changes, exits, created, removed.
#[tauri::command]
async fn subscribe_events(app: State<'_, App>, on_event: Channel<Event>) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    let mut rx = c.events();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(Event::Output { .. } | Event::Snapshot { .. }) => {}
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
    // through DMA-BUF. Turn that path off unless the user chose otherwise.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
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
            read_icon,
            list_themes,
            get_appearance,
            get_keys,
            config_path,
            set_appearance,
            list_agents,
            set_agents,
            set_agent_command,
            list_groups,
            save_group,
            delete_group,
            subscribe_output,
            unsubscribe_output,
            subscribe_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

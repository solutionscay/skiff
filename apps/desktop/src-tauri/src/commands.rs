use std::path::PathBuf;
use skiff_core::{group::Group, project::AgentInfo, session::{SessionInfo, SessionSpec}};
use tauri::{AppHandle, Manager, State};
use crate::connection::{App, ensure_client, err};

#[tauri::command]
pub(crate) async fn list_sessions(app: State<'_, App>) -> Result<Vec<SessionInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_sessions().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn create_session(app: State<'_, App>, spec: SessionSpec) -> Result<SessionInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    c.create_session(spec).await.map_err(err)
}

/// Not async: it runs on the main thread, one call after another, so
/// keystrokes reach the PTY in order. It waits for no lock and no reply.
#[tauri::command]
pub(crate) fn pty_write(handle: AppHandle, app: State<'_, App>, session: String, data: String) -> Result<(), String> {
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
pub(crate) async fn pty_resize(
    app: State<'_, App>,
    session: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.resize(&session, cols, rows).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_session_theme(
    app: State<'_, App>,
    session: String,
    theme: Option<String>,
) -> Result<SessionInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_session_theme(&session, theme).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn rename_session(
    app: State<'_, App>,
    session: String,
    name: String,
) -> Result<SessionInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    c.rename_session(&session, &name).await.map_err(err)
}

/// The session's pane has the keys: its unread flag and agent exit go.
#[tauri::command]
pub(crate) async fn session_seen(app: State<'_, App>, session: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.seen(&session).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn kill_session(app: State<'_, App>, session: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.kill(&session).await.map_err(err)
}

#[tauri::command]
pub(crate) fn config_path() -> String {
    skiff_core::config::config_path().display().to_string()
}

/// Opens projects.toml in the default editor. From Rust, because the opener's
/// `**` scope does not match hidden folders such as ~/.config.
#[tauri::command]
pub(crate) fn open_config(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = skiff_core::config::config_path();
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn list_themes() -> Result<Vec<skiff_core::theme::TerminalTheme>, String> {
    // The desktop ships its theme catalogue. A running daemon can be older.
    tokio::task::spawn_blocking(skiff_core::theme::list)
        .await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn import_themes(paths: Vec<PathBuf>) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || skiff_core::theme::import_files(&paths))
        .await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn open_theme_folder(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = skiff_core::theme::theme_dir().ok_or("Cannot find the theme folder")?;
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

/// A background image's raw bytes. The page turns them into a blob URL.
#[tauri::command]
pub(crate) async fn read_image(path: PathBuf) -> Result<tauri::ipc::Response, String> {
    let bytes = tokio::fs::read(&path).await.map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// One folder for the sidebar's file tree. Read here, not in the daemon:
/// the tree is a view, and the daemon owns only sessions.
#[tauri::command]
pub(crate) async fn list_dir(path: PathBuf) -> Result<Vec<skiff_core::files::Entry>, String> {
    tokio::task::spawn_blocking(move || skiff_core::files::list_dir(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// The changed files of one worktree, for the sidebar's Changes section.
#[tauri::command]
pub(crate) async fn git_changes(path: PathBuf) -> Result<Vec<skiff_core::git::Change>, String> {
    tokio::task::spawn_blocking(move || skiff_core::git::changes(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// What happened to an open request.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Opened {
    /// A terminal tool: the page runs `script` in the peek, in `cwd`.
    Peek { script: String, cwd: PathBuf },
    /// An app with its own window started.
    Window,
    /// No command is set, and the OS has no app for the file.
    NoApp { message: String },
}

async fn run_plan(plan: skiff_core::open::Plan, cwd: PathBuf, file: PathBuf) -> Result<Opened, String> {
    use skiff_core::open::Plan;
    tokio::task::spawn_blocking(move || match plan {
        Plan::Peek(script) => Ok(Opened::Peek { script, cwd }),
        Plan::Window(script) => skiff_core::open::start(&script, &cwd).map(|_| Opened::Window).map_err(err),
        Plan::Default => Ok(match skiff_core::open::open_default(&file) {
            Ok(()) => Opened::Window,
            Err(e) => Opened::NoApp { message: format!("{e:#}") },
        }),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Shows a changed file's diff, or the whole worktree's, with `[open] diff`.
#[tauri::command]
pub(crate) async fn open_diff(path: PathBuf, file: Option<String>) -> Result<Opened, String> {
    let open = skiff_core::config::load().map(|c| c.open).unwrap_or_default();
    let dir = path.clone();
    let plan = tokio::task::spawn_blocking(move || skiff_core::open::plan_diff(&dir, file.as_deref(), &open))
        .await
        .map_err(|e| e.to_string())?;
    run_plan(plan, path.clone(), path).await
}

/// The `[open]` commands, empty for a default, and the default diff command.
#[derive(serde::Serialize)]
pub(crate) struct OpenSettings {
    diff: String,
    text: String,
    markdown: String,
    html: String,
    default_diff: &'static str,
    /// The rows whose command runs in the peek.
    peek: Vec<&'static str>,
}

fn open_settings_now() -> Result<OpenSettings, String> {
    let o = skiff_core::config::load().map_err(err)?.open;
    Ok(OpenSettings {
        diff: o.diff.clone().unwrap_or_default(),
        text: o.text.clone().unwrap_or_default(),
        markdown: o.markdown.clone().unwrap_or_default(),
        html: o.html.clone().unwrap_or_default(),
        default_diff: skiff_core::git::DEFAULT_DIFF,
        peek: skiff_core::config::OPEN_KEYS.into_iter().filter(|k| o.in_peek(k)).collect(),
    })
}

#[tauri::command]
pub(crate) fn open_settings() -> Result<OpenSettings, String> {
    open_settings_now()
}

/// Sets one `[open]` command. Empty: the default.
#[tauri::command]
pub(crate) fn set_open(key: String, command: String) -> Result<OpenSettings, String> {
    let c = command.trim();
    skiff_core::config::set_open(&key, (!c.is_empty()).then_some(c)).map_err(err)?;
    open_settings_now()
}

/// Runs the peek or window setting for one `[open]` row.
#[tauri::command]
pub(crate) fn set_open_peek(key: String, peek: bool) -> Result<OpenSettings, String> {
    skiff_core::config::set_open_peek(&key, peek).map_err(err)?;
    open_settings_now()
}

/// Opens a file with its `[open]` command, else in its default app.
#[tauri::command]
pub(crate) async fn open_file(path: PathBuf) -> Result<Opened, String> {
    let open = skiff_core::config::load().map(|c| c.open).unwrap_or_default();
    let file = path.clone();
    let plan = tokio::task::spawn_blocking(move || skiff_core::open::plan_file(&file, &open))
        .await
        .map_err(|e| e.to_string())?;
    let cwd = path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    run_plan(plan, cwd, path).await
}

/// Projects with `files = true`. Read from projects.toml here, so an older
/// daemon that does not send the key still works.
#[tauri::command]
pub(crate) fn files_projects() -> Result<Vec<String>, String> {
    let cfg = skiff_core::config::load().map_err(err)?;
    Ok(cfg.projects.into_iter().filter(|p| p.files).map(|p| p.name).collect())
}

/// Projects with `changes = false`.
#[tauri::command]
pub(crate) fn hidden_changes_projects() -> Result<Vec<String>, String> {
    let cfg = skiff_core::config::load().map_err(err)?;
    Ok(cfg.projects.into_iter().filter(|p| p.changes == Some(false)).map(|p| p.name).collect())
}

#[tauri::command]
pub(crate) fn set_project_changes(project: String, on: bool) -> Result<(), String> {
    skiff_core::config::set_project_changes(&project, on).map_err(err)
}

/// Each project's own theme id, as `[name, id]`.
#[tauri::command]
pub(crate) fn project_themes() -> Result<Vec<(String, String)>, String> {
    let cfg = skiff_core::config::load().map_err(err)?;
    Ok(cfg.projects.into_iter().filter_map(|p| p.theme.map(|t| (p.name, t))).collect())
}

#[tauri::command]
pub(crate) fn set_project_theme(project: String, theme: Option<String>) -> Result<(), String> {
    skiff_core::config::set_project_theme(&project, theme.as_deref()).map_err(err)
}

#[tauri::command]
pub(crate) fn set_project_files(project: String, on: bool) -> Result<(), String> {
    skiff_core::config::set_project_files(&project, on).map_err(err)
}

#[tauri::command]
pub(crate) fn reveal_file(app: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn list_agents(app: State<'_, App>) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_agents().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn list_agent_sessions(app: State<'_, App>, command: Vec<String>, cwd: String) -> Result<String, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_agent_sessions(command, cwd.into()).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn list_groups(app: State<'_, App>) -> Result<Vec<Group>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_groups().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn save_group(app: State<'_, App>, group: Group) -> Result<Group, String> {
    let (c, _) = ensure_client(&app).await?;
    c.save_group(group).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn delete_group(app: State<'_, App>, group: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.delete_group(&group).await.map_err(err)
}

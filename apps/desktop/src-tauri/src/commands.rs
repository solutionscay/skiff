use std::path::PathBuf;
use skiff_core::{config::{Appearance, FolderInfo}, group::Group, project::{AgentInfo, Project, Worktree}, session::{SessionInfo, SessionSpec}};
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
pub(crate) async fn list_projects(app: State<'_, App>) -> Result<Vec<Project>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.list_projects().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn add_worktree(
    app: State<'_, App>,
    project: String,
    branch: String,
    base: Option<String>,
) -> Result<Worktree, String> {
    let (c, _) = ensure_client(&app).await?;
    c.add_worktree(&project, &branch, base).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn remove_worktree(
    app: State<'_, App>,
    project: String,
    path: PathBuf,
    force: bool,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.remove_worktree(&project, path, force).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn add_project(
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
pub(crate) async fn get_keys(app: State<'_, App>) -> Result<std::collections::BTreeMap<String, String>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.keys().await.map_err(err)
}

/// Memory in bytes: this app with its WebKit children, and skiffd without the
/// sessions it runs. Linux reports proportional set size, so shared libraries
/// count once; macOS reports resident set size.
#[tauri::command]
pub(crate) async fn get_appearance(app: State<'_, App>) -> Result<Appearance, String> {
    let (c, _) = ensure_client(&app).await?;
    c.appearance(None).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_appearance(app: State<'_, App>, theme: Option<String>) -> Result<Appearance, String> {
    let (c, _) = ensure_client(&app).await?;
    c.appearance(Some(theme)).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_font_size(app: State<'_, App>, size: Option<u8>) -> Result<Appearance, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_font_size(size).await.map_err(err)
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

#[tauri::command]
pub(crate) async fn set_project_icon(
    app: State<'_, App>,
    project: String,
    icon: Option<String>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_icon(project, icon).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn reorder_projects(app: State<'_, App>, order: Vec<String>) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.reorder_projects(order).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_project_background(
    app: State<'_, App>,
    project: String,
    background: Option<String>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_background(project, background).await.map_err(err)
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

/// A diff for the peek, printed by the command under `[open] diff`.
#[tauri::command]
pub(crate) async fn git_diff(path: PathBuf, file: Option<String>, cols: u16) -> Result<String, String> {
    let command = skiff_core::config::load().ok().and_then(|c| c.open.diff);
    tokio::task::spawn_blocking(move || skiff_core::git::diff_text(&path, file.as_deref(), command.as_deref(), cols))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// The `[open]` commands, empty for a default, and the default diff command.
#[derive(serde::Serialize)]
pub(crate) struct OpenSettings {
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

/// Opens a file with its `[open]` command, else in its default app. From
/// Rust, like open_config, because the opener's `**` scope does not match
/// hidden folders such as .github.
#[tauri::command]
pub(crate) async fn open_file(app: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
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
pub(crate) async fn set_project_color(
    app: State<'_, App>,
    project: String,
    color: String,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_color(project, color).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_project_closed(app: State<'_, App>, project: String, closed: bool) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_project_closed(project, closed).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn remove_project(app: State<'_, App>, project: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.remove_project(project).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn inspect_folder(app: State<'_, App>, path: PathBuf) -> Result<FolderInfo, String> {
    let (c, _) = ensure_client(&app).await?;
    // A daemon on another protocol may not know this request. Say so at once.
    if let Some(w) = app.warning.lock().await.as_ref().filter(|w| w.kind == "protocol") {
        return Err(w.message.clone());
    }
    c.inspect_folder(path).await.map_err(err)
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
pub(crate) async fn set_agent_command(
    app: State<'_, App>,
    agent: String,
    command: String,
) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_agent_command(agent, command).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn rename_agent(app: State<'_, App>, agent: String, name: String) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.rename_agent(agent, name).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn remove_agent(app: State<'_, App>, agent: String) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.remove_agent(agent).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn restore_agents(app: State<'_, App>) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.restore_agents().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_agents(app: State<'_, App>, enabled: Vec<String>) -> Result<Vec<AgentInfo>, String> {
    let (c, _) = ensure_client(&app).await?;
    c.set_agents(enabled).await.map_err(err)
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

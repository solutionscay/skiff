//! Project and worktree requests go to the selected Host's data plane
//! (`skiff-host`): this Mac's for This Mac, a remote Host's own for it. The
//! app writes no `[[project]]` table and runs no worktree change itself. Its
//! own settings (keys, appearance) are this computer's `client.toml`; the
//! agent settings below are still this Mac's `projects.toml` (M6).

use std::path::{Path, PathBuf};

use skiff_core::{
    config::{self, client, Appearance},
    git,
    project::AgentInfo,
};
use tauri::{AppHandle, Emitter, State};

use skiff_client::host::data;
use skiff_host_api::{AgentEdit, HostFolder, HostProject, HostWorktree, ProjectEdit, ProjectFleet};

use crate::connection::{blocking, data_read, data_write, keep_revision, project_write, App};

/// The page reloads projects on this event.
pub(crate) const PROJECTS_CHANGED: &str = "skiff:projects-changed";

/// Tells the page after a change was tried: done, refused or unknown, it
/// reads the projects again.
fn changed<T>(handle: &AppHandle, out: Result<T, String>) -> Result<T, String> {
    let _ = handle.emit(PROJECTS_CHANGED, ());
    out
}

/// Runs a local projects.toml edit (images only), then tells the page.
async fn local_edit(handle: &AppHandle, f: impl FnOnce() -> anyhow::Result<()> + Send + 'static) -> Result<(), String> {
    changed(handle, blocking(f).await)
}

/// One key of one Host project (`project_config@1`).
async fn edit(handle: &AppHandle, app: &App, project: String, edit: ProjectEdit) -> Result<(), String> {
    let out = project_write(app, |c, revision| async move {
        data::project_edit(&c, &revision, &project, edit).await.map(|r| ((), r))
    })
    .await;
    changed(handle, out)
}

/// The selected Host's projects, from its data-plane service (`projects@1`):
/// this Mac's `skiff-host` here, a remote Host's own there. Never read here.
/// Their revision is kept for the next change.
#[tauri::command]
pub(crate) async fn list_projects(app: State<'_, App>) -> Result<Vec<HostProject>, String> {
    read_projects(&app).await
}

async fn read_projects(app: &App) -> Result<Vec<HostProject>, String> {
    data_read(app, |c| async move {
        let (rows, revision) = data::project_rows(&c).await?;
        keep_revision(app, &c, revision);
        Ok(rows)
    })
    .await
}

/// One project read again: git runs in it only, not in every project.
#[tauri::command]
pub(crate) async fn get_project(app: State<'_, App>, name: String) -> Result<HostProject, String> {
    data_read(&app, |c| async move { data::project_get(&c, &name).await }).await
}

/// A worktree on a new branch, or with `checkout` on an existing one, made
/// by the Host's git (`worktrees@1`).
#[tauri::command]
pub(crate) async fn add_worktree(
    handle: AppHandle,
    app: State<'_, App>,
    project: String,
    branch: String,
    base: Option<String>,
    checkout: Option<bool>,
) -> Result<HostWorktree, String> {
    let checkout = checkout.unwrap_or(false);
    let out = data_write(&app, |c| async move { data::worktree_add(&c, &project, &branch, base.as_deref(), checkout).await }).await;
    changed(&handle, out)
}

/// The project's branches for New worktree, from the Host's git.
#[tauri::command]
pub(crate) async fn list_branches(app: State<'_, App>, project: String) -> Result<skiff_host_api::HostBranches, String> {
    data_read(&app, |c| async move { data::branch_list(&c, &project).await }).await
}

/// The Host refuses the main worktree and one its own live sessions run
/// in. It refuses uncommitted changes unless `force`, which deletes them.
#[tauri::command]
pub(crate) async fn remove_worktree(
    handle: AppHandle,
    app: State<'_, App>,
    project: String,
    path: String,
    force: bool,
) -> Result<(), String> {
    let out = data_write(&app, |c| async move { data::worktree_remove(&c, &project, &path, force).await }).await;
    changed(&handle, out)
}

/// Adds the git repository that contains `path`, or the plain folder, a folder on the Host.
#[tauri::command]
pub(crate) async fn add_project(
    handle: AppHandle,
    app: State<'_, App>,
    path: String,
    name: Option<String>,
    short: Option<String>,
    color: Option<String>,
) -> Result<HostProject, String> {
    let out = project_write(&app, |c, revision| async move {
        data::project_add(&c, &revision, &path, name, short, color).await
    })
    .await;
    changed(&handle, out)
}

/// Initializes the selected folder without staging files or creating a
/// commit. This Mac only: no Host offers a Git write but worktrees.
#[tauri::command]
pub(crate) async fn init_repository(path: PathBuf) -> Result<(), String> {
    blocking(move || git::init_repository(&path)).await
}

/// What `add_project` would do for `path`, a folder on the Host. Writes nothing.
#[tauri::command]
pub(crate) async fn inspect_folder(app: State<'_, App>, path: String) -> Result<HostFolder, String> {
    data_read(&app, |c| async move { data::project_inspect(&c, &path).await }).await
}

/// Runs `git init` in `path`, a folder on the Host, through the Host
/// (`project_config@1`). Answers what `add_project` would do there now.
#[tauri::command]
pub(crate) async fn init_host_repository(app: State<'_, App>, path: String) -> Result<HostFolder, String> {
    data_write(&app, |c| async move { data::project_init(&c, &path).await }).await
}

/// A `folders@1` call with the Host's typed error, so the page words each
/// cause itself. A connection failure is `operation_failed`.
async fn folder_call<T, F, Fut>(app: &App, call: F) -> Result<T, skiff_host_api::Error>
where
    F: FnOnce(std::sync::Arc<skiff_client::host::HostConnection>) -> Fut,
    Fut: std::future::Future<Output = Result<T, skiff_host_api::Error>>,
{
    match data_read(app, |c| async move { Ok(call(c).await) }).await {
        Ok(out) => out,
        Err(message) => Err(skiff_host_api::Error::OperationFailed { message }),
    }
}

/// The Host account's home folder, for the folder picker.
#[tauri::command]
pub(crate) async fn folder_home(app: State<'_, App>) -> Result<String, skiff_host_api::Error> {
    folder_call(&app, |c| async move { data::folder_home(&c).await }).await
}

/// The folders in `path` on the Host, for the folder picker. Host paths
/// only: nothing here reads or opens them on this Mac.
#[tauri::command]
pub(crate) async fn folder_list(
    app: State<'_, App>,
    path: String,
    hidden: bool,
    filter: Option<String>,
) -> Result<skiff_host_api::FolderListing, skiff_host_api::Error> {
    folder_call(&app, |c| async move { data::folder_list(&c, &path, hidden, filter).await }).await
}

/// The picker's Recent and Git repos, from the Host.
#[tauri::command]
pub(crate) async fn folder_places(app: State<'_, App>) -> Result<skiff_host_api::FolderPlaces, skiff_host_api::Error> {
    folder_call(&app, |c| async move { data::folder_places(&c).await }).await
}

/// Makes the folder `name` in `parent` on the Host.
#[tauri::command]
pub(crate) async fn folder_create(app: State<'_, App>, parent: String, name: String) -> Result<String, skiff_host_api::Error> {
    folder_call(&app, |c| async move { data::folder_create(&c, &parent, &name).await }).await
}

/// This Mac only: an image is a path here, and no Host takes image bytes yet.
#[tauri::command]
pub(crate) async fn set_project_icon(handle: AppHandle, project: String, icon: Option<String>) -> Result<(), String> {
    local_edit(&handle, move || config::set_project_icon(&project, icon.as_deref())).await
}

/// A Skipper's chosen avatar: `seed` for a generated one, `image` as a data
/// URL. `scope`: "" for the Skiff agent, `fleet:<name>`, or a project's name.
#[derive(serde::Serialize)]
pub(crate) struct SkipperAvatar {
    scope: String,
    seed: Option<u32>,
    image: Option<String>,
}

/// In each Skipper's own folder: `avatar = "gen:N"`, or the file name of the
/// picture copied beside it.
const AVATAR_FILE: &str = "avatar.toml";

/// The size `data_url` still reads, as for project icons.
const AVATAR_MAX: u64 = 512 * 1024;

/// The Skipper's own folder: the Skiff agent's folder, `fleets/<fleet>/` or `projects/<project>/` in it.
fn skipper_dir(cfg: &config::Config, scope: &str) -> anyhow::Result<PathBuf> {
    use skiff_client::skipper;
    match (scope, scope.strip_prefix("fleet:")) {
        ("", _) => skipper::folder(cfg),
        (_, Some(fleet)) => skipper::fleet_folder(cfg, fleet),
        (project, None) => skipper::project_folder(cfg, project),
    }
}

fn read_avatar(scope: String, dir: &Path) -> Option<SkipperAvatar> {
    let table: toml::Table = std::fs::read_to_string(dir.join(AVATAR_FILE)).ok()?.parse().ok()?;
    let avatar = table.get("avatar")?.as_str()?;
    let (seed, image) = match avatar.strip_prefix("gen:") {
        Some(n) => (Some(n.parse().ok()?), None),
        None => (None, Some(skiff_core::project::data_url(&dir.join(Path::new(avatar).file_name()?))?)),
    };
    Some(SkipperAvatar { scope, seed, image })
}

/// A random generated avatar, `gen:1` to `gen:9999`, for a Skipper's first face.
fn random_avatar(scope: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write(scope.as_bytes());
    format!("gen:{}", 1 + h.finish() % 9999)
}

/// This Mac only, as the icon: each Skipper's avatar from its own folder. A
/// folder with none gets a random generated one, so its face stays.
#[tauri::command]
pub(crate) async fn skipper_avatars() -> Result<Vec<SkipperAvatar>, String> {
    blocking(|| {
        let cfg = config::load()?;
        let mut scopes = vec![String::new()];
        for p in &cfg.projects {
            scopes.push(p.name.clone());
            if let Some(f) = &p.fleet {
                let fleet = format!("fleet:{f}");
                if !scopes.contains(&fleet) {
                    scopes.push(fleet);
                }
            }
        }
        Ok(scopes
            .into_iter()
            .filter_map(|s| {
                let dir = skipper_dir(&cfg, &s).ok().filter(|d| d.is_dir())?;
                let file = dir.join(AVATAR_FILE);
                if !file.exists() {
                    std::fs::write(&file, format!("avatar = {}\n", toml::Value::from(random_avatar(&s)))).ok()?;
                }
                read_avatar(s, &dir)
            })
            .collect())
    })
    .await
}

/// This Mac only, as the icon. `avatar`: `gen:N`, or a picture's path, which
/// is copied into the Skipper's folder. `None`: generated from the name again.
#[tauri::command]
pub(crate) async fn set_skipper_avatar(handle: AppHandle, scope: String, avatar: Option<String>) -> Result<(), String> {
    local_edit(&handle, move || {
        use anyhow::{anyhow, Context};
        let dir = skipper_dir(&config::load()?, &scope)?;
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let keep = match avatar.as_deref() {
            None => None,
            Some(a) if a.starts_with("gen:") => {
                a[4..].parse::<u32>().map_err(|_| anyhow!("{a} is not a generated avatar"))?;
                Some(a.to_string())
            }
            Some(path) => {
                let src = Path::new(path);
                let ext = src
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .filter(|e| ["png", "svg", "jpg", "jpeg", "webp", "gif"].contains(&e.as_str()))
                    .ok_or_else(|| anyhow!("{path} is not a PNG, SVG, JPEG, WebP or GIF picture"))?;
                if std::fs::metadata(src).with_context(|| format!("read {path}"))?.len() > AVATAR_MAX {
                    return Err(anyhow!("{path} is larger than 512 KB"));
                }
                let name = format!("avatar.{ext}");
                let to = dir.join(&name);
                // A copy onto itself would empty the file.
                if src.canonicalize().ok() != to.canonicalize().ok() {
                    std::fs::copy(src, &to).with_context(|| format!("copy {path} to {}", to.display()))?;
                }
                Some(name)
            }
        };
        // One picture at most: an older one goes.
        for e in std::fs::read_dir(&dir)?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("avatar.") && name != AVATAR_FILE && Some(&name) != keep.as_ref() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let file = dir.join(AVATAR_FILE);
        match keep {
            Some(v) => std::fs::write(&file, format!("avatar = {}\n", toml::Value::from(v))).with_context(|| format!("write {}", file.display()))?,
            None => match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            },
        }
        Ok(())
    })
    .await
}

/// This Mac only, as the icon.
#[tauri::command]
pub(crate) async fn set_project_background(
    handle: AppHandle,
    project: String,
    background: Option<String>,
) -> Result<(), String> {
    local_edit(&handle, move || config::set_project_background(&project, background.as_deref())).await
}

#[tauri::command]
pub(crate) async fn set_project_fleet(handle: AppHandle, app: State<'_, App>, project: String, fleet: String) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Fleet { fleet }).await
}

#[tauri::command]
pub(crate) async fn set_project_short(handle: AppHandle, app: State<'_, App>, project: String, short: String) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Short { short }).await
}

#[tauri::command]
pub(crate) async fn set_project_background_opacity(
    handle: AppHandle,
    app: State<'_, App>,
    project: String,
    percent: Option<u8>,
) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::BackgroundOpacity { percent }).await
}

#[tauri::command]
pub(crate) async fn set_project_color(handle: AppHandle, app: State<'_, App>, project: String, color: String) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Color { color }).await
}

#[tauri::command]
pub(crate) async fn set_project_changes(handle: AppHandle, app: State<'_, App>, project: String, on: bool) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Changes { on }).await
}

#[tauri::command]
pub(crate) async fn set_project_files(handle: AppHandle, app: State<'_, App>, project: String, on: bool) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Files { on }).await
}

#[tauri::command]
pub(crate) async fn set_project_theme(handle: AppHandle, app: State<'_, App>, project: String, theme: Option<String>) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Theme { theme }).await
}

#[tauri::command]
pub(crate) async fn reorder_projects(
    handle: AppHandle,
    app: State<'_, App>,
    order: Vec<String>,
    fleets: Option<Vec<(String, String)>>,
) -> Result<(), String> {
    let fleets = fleets.unwrap_or_default().into_iter().map(|(project, fleet)| ProjectFleet { project, fleet }).collect();
    let out = project_write(&app, |c, revision| async move {
        data::project_arrange(&c, &revision, order, fleets).await.map(|r| ((), r))
    })
    .await;
    changed(&handle, out)
}

#[tauri::command]
pub(crate) async fn set_project_closed(handle: AppHandle, app: State<'_, App>, project: String, closed: bool) -> Result<(), String> {
    edit(&handle, &app, project, ProjectEdit::Closed { closed }).await
}

/// Drops the project and its settings from the Host's config. The folder stays.
#[tauri::command]
pub(crate) async fn remove_project(handle: AppHandle, app: State<'_, App>, project: String) -> Result<(), String> {
    let out = project_write(&app, |c, revision| async move {
        data::project_remove(&c, &revision, &project).await.map(|r| ((), r))
    })
    .await;
    changed(&handle, out)
}

#[tauri::command]
pub(crate) async fn get_keys() -> Result<std::collections::BTreeMap<String, String>, String> {
    blocking(|| Ok(client::load()?.keys)).await
}

#[tauri::command]
pub(crate) async fn get_appearance() -> Result<Appearance, String> {
    blocking(|| Ok(client::load()?.appearance)).await
}

/// Sets the app theme: a theme id, or `null` for Harbor.
#[tauri::command]
pub(crate) async fn set_appearance(theme: Option<String>) -> Result<Appearance, String> {
    blocking(move || {
        config::set_app_theme(theme.as_deref())?;
        Ok(client::load()?.appearance)
    })
    .await
}

/// Sets the text size: points, or `null` for the default.
#[tauri::command]
pub(crate) async fn set_font_size(size: Option<u8>) -> Result<Appearance, String> {
    blocking(move || {
        config::set_font_size(size)?;
        Ok(client::load()?.appearance)
    })
    .await
}

/// Sets Mission control's filter: only the running sessions, or all.
#[tauri::command]
pub(crate) async fn set_mission_filter(running: bool) -> Result<(), String> {
    blocking(move || config::set_mission_filter(running)).await
}

/// This Mac's agent list, from the projects.toml this app writes, so a change
/// reads back whatever skiffd build runs. Whether an agent is installed
/// depends on the sessions' PATH and aliases, which the app does not have:
/// skiffd answers that for each agent whose command it read the same. An
/// agent it does not know, or no skiffd, falls back to the app's PATH.
pub(crate) async fn local_agents(app: &App) -> Result<Vec<AgentInfo>, String> {
    let daemon = match crate::connection::host_now(app).await {
        Ok(c) => skiff_client::host::local::legacy_agents(&c).await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    blocking(move || {
        let cfg = config::load()?.agents;
        let mut list = skiff_core::project::agents_in(&cfg, |id, command| {
            daemon.iter().find(|a| a.id == id && a.command == command).map(|a| a.installed)
        });
        skiff_core::project::mark_used(&mut list);
        Ok(list)
    })
    .await
}

/// Runs an agent config edit, then answers with the agent list, read back
/// from the config just written ([`local_agents`]). A remote Host makes the
/// change itself (`agent_config@1`), from the revision of its last agent read.
async fn agents_after(
    app: &State<'_, App>,
    edit: AgentEdit,
    f: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
) -> Result<Vec<AgentInfo>, String> {
    if app.remote() {
        return agent_write(app, edit).await;
    }
    blocking(f).await?;
    local_agents(app).await
}

/// Why an agent change was not sent: no agent read of this Host to make it from.
const AGENTS_UNREAD: &str = "Skiff has not read this Host's agents yet. It reads them again: try again.";

/// The agent list for Settings, with the revision of a remote Host's
/// `[agents]` kept for the next change. This Mac's comes from its config.
#[tauri::command]
pub(crate) async fn agent_config(app: State<'_, App>) -> Result<Vec<AgentInfo>, String> {
    if !app.remote() {
        return local_agents(&app).await;
    }
    let app = &*app;
    data_read(app, |c| async move {
        let (agents, revision) = data::agent_config(&c).await?;
        *app.agent_revision.lock().unwrap() = Some((c.hello().host.host_id.clone(), revision));
        Ok(agents.into_iter().map(skiff_client::host::restore_agent).collect())
    })
    .await
}

/// One `[agents]` change on a remote Host, sent once, as a project change
/// is. A conflict forgets the revision, so the next change waits for the page
/// to read the agents again.
async fn agent_write(app: &App, edit: AgentEdit) -> Result<Vec<AgentInfo>, String> {
    use skiff_host_api::Error as E;
    data_write(app, |c| async move {
        let host = c.hello().host.host_id.clone();
        let revision = app.agent_revision.lock().unwrap().clone().filter(|(h, _)| *h == host).map(|(_, r)| r);
        let Some(revision) = revision else {
            return Err(E::Invalid { message: AGENTS_UNREAD.into() });
        };
        match data::agent_edit(&c, &revision, edit).await {
            Ok((agents, revision)) => {
                *app.agent_revision.lock().unwrap() = Some((host, revision));
                Ok(agents.into_iter().map(skiff_client::host::restore_agent).collect())
            }
            Err(E::Conflict { message }) => {
                *app.agent_revision.lock().unwrap() = None;
                Err(E::Conflict { message: format!("{message} Skiff reads them again: try again if it is still needed.") })
            }
            Err(e) => Err(e),
        }
    })
    .await
}

/// Saves which agents the + menu offers.
#[tauri::command]
pub(crate) async fn set_agents(app: State<'_, App>, enabled: Vec<String>) -> Result<Vec<AgentInfo>, String> {
    let edit = AgentEdit::Enabled { agents: enabled.clone() };
    agents_after(&app, edit, move || config::set_enabled_agents(&enabled)).await
}

/// Replaces an agent's command line. Empty restores the default.
#[tauri::command]
pub(crate) async fn set_agent_command(app: State<'_, App>, agent: String, command: String) -> Result<Vec<AgentInfo>, String> {
    let edit = AgentEdit::Command { agent: agent.clone(), command: command.clone() };
    agents_after(&app, edit, move || config::set_agent_command(&agent, &command)).await
}

#[tauri::command]
pub(crate) async fn rename_agent(app: State<'_, App>, agent: String, name: String) -> Result<Vec<AgentInfo>, String> {
    let edit = AgentEdit::Rename { agent: agent.clone(), name: name.clone() };
    agents_after(&app, edit, move || config::rename_agent(&agent, &name)).await
}

#[tauri::command]
pub(crate) async fn remove_agent(app: State<'_, App>, agent: String) -> Result<Vec<AgentInfo>, String> {
    let edit = AgentEdit::Remove { agent: agent.clone() };
    agents_after(&app, edit, move || config::remove_agent(&agent)).await
}

/// Brings back the default agents the user removed.
#[tauri::command]
pub(crate) async fn restore_agents(app: State<'_, App>) -> Result<Vec<AgentInfo>, String> {
    agents_after(&app, AgentEdit::Restore, config::restore_agents).await
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

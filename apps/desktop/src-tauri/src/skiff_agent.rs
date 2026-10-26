//! The Skiff agent's settings, folder and session, for the page. The folder
//! setup and the find-or-start rule live in `skiff_client::skipper`, which
//! `skiff skipper` uses too, so the app and the CLI pick the same session.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use skiff_client::{
    agent_folder::{self, Kind, Mode},
    fleet, skipper,
};
use skiff_core::{
    config::{self, SkiffAgentKey},
    session::SessionInfo,
    tasks::{self, TaskType},
};
use tauri::{AppHandle, Emitter, State};

use crate::{
    config::PROJECTS_CHANGED,
    connection::{ensure_client, err, App},
};

/// Runs at app start, off the main thread. Never prompts. Makes the agent's
/// folder when it is missing, with Skipper on or off, and copies the
/// built-in `skiff-cli` skill out when it changed.
pub fn sync() {
    if let Err(e) = config::load().and_then(|cfg| skipper::make_folder(&cfg)) {
        eprintln!("skiff agent folder: {e:#}");
    }
    if let Err(e) = skipper::sync_skill() {
        eprintln!("skiff-cli skill: {e:#}");
    }
}

/// Makes the agent's folder ready. Returns the folder.
fn ensure() -> anyhow::Result<PathBuf> {
    skipper::ensure(&config::load()?)
}

/// The `skiff` CLI beside the app binary, else the one on `PATH`.
fn cli() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|e| e.with_file_name("skiff"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("skiff"))
}

/// Starts `skiff watch` for the agent's session, unless one runs for this
/// daemon. The page calls it on every start and every restore. True when it
/// started one.
#[tauri::command]
pub async fn start_skiff_watch(app: State<'_, App>, session: String) -> Result<bool, String> {
    ensure_watch(&app, cli(), session).await
}

/// One current watcher for the agent's session. A watcher of an older CLI,
/// as after an update, gets one replacement attempt per app run when it is
/// proven to be this daemon's `skiff watch`. One that is not, or that does
/// not stop, keeps running, and only the log says so: the update is not
/// finished, and nothing asks the user. One check runs at a time.
pub(crate) async fn ensure_watch(app: &App, cli: PathBuf, session: String) -> Result<bool, String> {
    let _one = app.watch_gate.lock().await;
    let mut known = app.watch_pids.lock().unwrap().clone();
    if let Ok((c, _)) = ensure_client(app).await {
        if let Ok(list) = c.list_sessions().await {
            known.extend(list.iter().filter(|s| s.id == session).filter_map(|s| s.watch.as_ref()).flat_map(|w| w.pids.iter().copied()));
        }
    }
    // Only the bundled CLI replaces a watcher: a bare `skiff` from PATH is
    // not known to be this app's build.
    let tried = cli.is_absolute().then(|| app.watch_tried.clone());
    let socket = skiff_core::socket::socket_path();
    let out = blocking(move || skipper::ensure_watch(&socket, &cli, skipper::CLI_VERSION, &session, &known, tried.as_deref())).await?;
    match out {
        skipper::WatchOutcome::Started => Ok(true),
        skipper::WatchOutcome::Current => Ok(false),
        skipper::WatchOutcome::Replaced { pid } => {
            eprintln!("skiff watch: replaced the older watcher (pid {pid})");
            Ok(true)
        }
        skipper::WatchOutcome::Kept(why) => {
            eprintln!("skiff watch not updated: {why}");
            Ok(false)
        }
    }
}

/// The watch log beside the socket, for the row's Open watch log.
#[tauri::command]
pub fn skiff_watch_log() -> String {
    skiff_core::socket::socket_path().with_extension("watch.log").display().to_string()
}

/// The Skiff agent's session, and what the open did to it.
#[derive(Serialize)]
pub struct Opened {
    session: SessionInfo,
    created: bool,
    typed: bool,
    /// Why its watcher did not start, for the row's tooltip.
    watch_error: Option<String>,
}

/// The row and the key: find the Skiff agent's session or start it, through
/// the daemon's guarded request, and start its watcher. With `restore`, the
/// `input_gen` the page read: only continue a restored shell, as Settings
/// says. `null` when there is nothing to open.
#[tauri::command]
pub async fn open_skiff_agent(app: State<'_, App>, restore: Option<u64>) -> Result<Option<Opened>, String> {
    let (c, _) = ensure_client(&app).await?;
    let how = match restore {
        Some(expect) => skipper::Open::Restore { expect },
        None => skipper::Open::Start,
    };
    let Some(found) = skipper::open(&c, how).await.map_err(err)? else { return Ok(None) };
    let watch_error = ensure_watch(&app, cli(), found.session.id.clone()).await.err();
    if let Some(e) = &watch_error {
        eprintln!("skiff watch: {e}");
    }
    Ok(Some(Opened { session: found.session, created: found.created, typed: found.typed, watch_error }))
}

/// `[skiff_agent]` as the page reads it: the folder without `~`.
#[derive(Serialize)]
pub struct SkiffAgent {
    name: Option<String>,
    agent: Option<String>,
    enabled: bool,
    /// `~/skiff-agent`, or the override. Absent when the override is refused.
    folder: Option<String>,
    /// Why the override in projects.toml is refused.
    folder_error: Option<String>,
    /// The folder projects.toml names, refused or not, for the Folder row.
    configured: String,
    /// `ready` (a Skiff folder), `missing` (the app could not make it),
    /// `other` (it holds other files) or `refused`.
    folder_status: &'static str,
    continue_on_restore: bool,
    /// The project menu offers Start project Skipper.
    project_skippers: bool,
    /// The agent project Skippers start. Absent: the Skipper's.
    project_agent: Option<String>,
    /// `[skiff_agent.tasks]` in file order, with the command each row starts.
    tasks: Vec<TaskType>,
}

fn read() -> anyhow::Result<SkiffAgent> {
    let cfg = config::load().context("read projects.toml")?;
    let (folder, mut folder_error) = match skipper::folder(&cfg) {
        Ok(f) => (Some(f.display().to_string()), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let configured = skipper::configured(&cfg)?;
    let folder_status = match (&folder, agent_folder::kind(&configured)) {
        (None, _) => "refused",
        (_, Ok(Kind::Skiff)) => "ready",
        (_, Ok(Kind::New | Kind::Empty)) => {
            folder_error = Some(skipper::no_folder(&configured));
            "missing"
        }
        _ => "other",
    };
    let configured = configured.display().to_string();
    let a = cfg.skiff_agent;
    // The command lines come from projects.toml, so the local list builds the same ones skiffd would.
    let tasks = tasks::resolve(&a.tasks, &skiff_core::project::agents());
    Ok(SkiffAgent {
        name: a.name,
        agent: a.agent,
        enabled: a.enabled,
        folder,
        folder_error,
        configured,
        folder_status,
        continue_on_restore: a.continue_on_restore,
        project_skippers: a.project_skippers,
        project_agent: a.project_agent,
        tasks,
    })
}

/// One edit from Settings › Skiff agent. `null` or empty takes the default.
#[derive(Deserialize)]
#[serde(tag = "key", content = "value", rename_all = "snake_case")]
pub enum Edit {
    Name(Option<String>),
    Agent(Option<String>),
    Enabled(bool),
    ContinueOnRestore(bool),
    ProjectSkippers(bool),
    /// `null`: Same as Skipper.
    ProjectAgent(Option<String>),
    /// A new row at the end of Task types.
    AddTask(TaskEdit),
    /// A row's agent or model.
    Task(TaskEdit),
    RenameTask { from: String, to: String },
    RemoveTask(String),
    /// Auto flush's age in days. `0`: off.
    CacheDays(u32),
}

/// A task type row as Settings sends it. An empty model is the agent's default.
#[derive(Deserialize)]
pub struct TaskEdit {
    #[serde(rename = "type")]
    name: String,
    agent: String,
    model: Option<String>,
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn get_skiff_agent() -> Result<SkiffAgent, String> {
    blocking(read).await
}

/// Saves one setting and answers with all of them. The page reloads its
/// projects on the event, so the row follows at once.
#[tauri::command]
pub async fn set_skiff_agent(app: AppHandle, edit: Edit) -> Result<SkiffAgent, String> {
    let out = blocking(move || {
        match edit {
            Edit::Name(v) => config::set_skiff_agent(SkiffAgentKey::Name(v.as_deref()))?,
            Edit::Agent(v) => config::set_skiff_agent(SkiffAgentKey::Agent(v.as_deref()))?,
            Edit::ContinueOnRestore(on) => config::set_skiff_agent(SkiffAgentKey::ContinueOnRestore(on))?,
            Edit::ProjectSkippers(on) => config::set_skiff_agent(SkiffAgentKey::ProjectSkippers(on))?,
            Edit::ProjectAgent(v) => config::set_skiff_agent(SkiffAgentKey::ProjectAgent(v.as_deref()))?,
            Edit::AddTask(t) => config::add_task_type(&t.name, &t.agent, t.model.as_deref())?,
            Edit::Task(t) => config::set_task_type(&t.name, &t.agent, t.model.as_deref())?,
            Edit::RenameTask { from, to } => config::rename_task_type(&from, &to)?,
            Edit::RemoveTask(name) => config::remove_task_type(&name)?,
            Edit::CacheDays(n) => config::set_skiff_agent(SkiffAgentKey::CacheDays(n))?,
            Edit::Enabled(on) => {
                // On makes the folder ready first: a refused folder stays off.
                if on {
                    ensure()?;
                }
                config::set_skiff_agent(SkiffAgentKey::Enabled(on))?
            }
        }
        read()
    })
    .await?;
    let _ = app.emit(PROJECTS_CHANGED, ());
    Ok(out)
}

/// Project menu › Start project Skipper: finds or starts the project's
/// Skipper with `skiff skipper --project`, which also gives a new one its
/// first message and starts the watcher. Returns its session id. It waits
/// for the agent's input box, so it can take a while.
#[tauri::command]
pub async fn start_project_skipper(project: String) -> Result<String, String> {
    let socket = skiff_core::socket::socket_path();
    blocking(move || {
        let out = std::process::Command::new(cli())
            .arg("--socket")
            .arg(&socket)
            .args(["skipper", "--project", &project, "--no-attach"])
            // The app is a person, not a pane.
            .env_remove("SKIFF_SESSION")
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("run {}", cli().display()))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("{}", msg.trim().trim_start_matches("skiff: ").trim_start_matches("Error: "));
        }
        Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().trim().to_string())
    })
    .await
}

/// Fleet menu › Start fleet Skipper: finds or starts the fleet's Skipper
/// with `skiff skipper --fleet`, which also gives a new one its first
/// message and starts the watcher. The page asked first, so `--yes` takes
/// the fleet's project Skippers over. Returns its session id.
#[tauri::command]
pub async fn start_fleet_skipper(fleet: String) -> Result<String, String> {
    let socket = skiff_core::socket::socket_path();
    blocking(move || {
        let out = std::process::Command::new(cli())
            .arg("--socket")
            .arg(&socket)
            .args(["skipper", "--fleet", &fleet, "--no-attach", "--yes"])
            // The app is a person, not a pane.
            .env_remove("SKIFF_SESSION")
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("run {}", cli().display()))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("{}", msg.trim().trim_start_matches("skiff: ").trim_start_matches("Error: "));
        }
        Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().trim().to_string())
    })
    .await
}

/// The project Skippers a takeover of `fleet` would stop, for the confirm:
/// those of its projects, and of `join` when it joins. Changes nothing.
#[tauri::command]
pub async fn fleet_takeover_preview(app: State<'_, App>, fleet: String, join: Option<String>) -> Result<Vec<fleet::Taken>, String> {
    let (c, _) = ensure_client(&app).await?;
    fleet::preview(&c, &fleet, join.as_deref()).await.map_err(err)
}

/// Add to fleet…, Move to fleet… or a drop, on the person's confirm: the
/// running fleet Skipper of `fleet` takes `join`'s project Skipper over,
/// then `join` joins the fleet.
#[tauri::command]
pub async fn fleet_takeover(app: State<'_, App>, handle: AppHandle, fleet: String, join: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    let socket = skiff_core::socket::socket_path();
    let out = fleet::takeover(&c, &socket, &fleet, Some(&join), true).await.map_err(err);
    let _ = handle.emit(PROJECTS_CHANGED, ());
    out.map(|_| ())
}

/// Rename fleet…: the fleet, its Skipper's folder, rows, session and mark.
#[tauri::command]
pub async fn rename_fleet(app: State<'_, App>, handle: AppHandle, old: String, new: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    let out = fleet::rename(&c, &old, new.trim()).await.map_err(err);
    let _ = handle.emit(PROJECTS_CHANGED, ());
    out
}

/// A preview for the Change… dialog, with Skipper's session when it runs.
#[derive(Serialize)]
pub struct FolderPreview {
    #[serde(flatten)]
    preview: agent_folder::Preview,
    /// Skipper's session runs: the change ends it first.
    skipper_runs: bool,
}

/// The chosen folder, resolved and read. A relative path resolves against
/// the home folder. Changes nothing.
#[tauri::command]
pub async fn preview_skiff_folder(app: State<'_, App>, path: String) -> Result<FolderPreview, String> {
    let (panes, skipper) = live(&app).await;
    blocking(move || {
        let home = skipper::home()?;
        let to = agent_folder::resolve(&path, &home, &home)?;
        let preview = agent_folder::preview(&config::load()?, &to, &panes)?;
        Ok(FolderPreview { preview, skipper_runs: skipper.is_some() })
    })
    .await
}

/// The live panes and Skipper's session. With no daemon, none run.
async fn live(app: &App) -> (agent_folder::Panes, Option<SessionInfo>) {
    match ensure_client(app).await {
        Ok((c, _)) => agent_folder::sessions(&c).await.unwrap_or_default(),
        Err(_) => Default::default(),
    }
}

/// Copy, check and switch, start empty, or switch to a Skiff folder. `path`
/// is the preview's resolved folder. Skipper's session ends first when
/// `end_skipper`, else a running one stops the change. `anyway` lets the
/// change run while tasks still run.
#[tauri::command]
pub async fn change_skiff_folder(
    app: State<'_, App>,
    handle: AppHandle,
    path: PathBuf,
    mode: Mode,
    end_skipper: bool,
    anyway: bool,
) -> Result<agent_folder::Done, String> {
    let (mut panes, skipper) = live(&app).await;
    if let Some(s) = skipper {
        if !end_skipper {
            return Err("Skipper runs. It ends before the change.".into());
        }
        let (c, _) = ensure_client(&app).await?;
        c.kill(&s.id).await.map_err(err)?;
        panes.remove(&s.id);
    }
    let done = blocking(move || agent_folder::change(&config::load()?, &path, mode, &panes, anyway)).await?;
    let _ = handle.emit(PROJECTS_CHANGED, ());
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::Edit;

    /// The shapes `SkiffAgentEdit` in apps/desktop/src/appearance/skiffAgentSettings.ts sends.
    #[test]
    fn reads_the_task_type_edits_the_page_sends() {
        let edit = |v: serde_json::Value| serde_json::from_value::<Edit>(v).unwrap();
        let add = edit(serde_json::json!({ "key": "add_task", "value": { "type": "review", "agent": "claude", "model": "opus" } }));
        assert!(matches!(add, Edit::AddTask(t) if t.name == "review" && t.agent == "claude" && t.model.as_deref() == Some("opus")));
        let set = edit(serde_json::json!({ "key": "task", "value": { "type": "review", "agent": "aider", "model": null } }));
        assert!(matches!(set, Edit::Task(t) if t.model.is_none()));
        let rename = edit(serde_json::json!({ "key": "rename_task", "value": { "from": "review", "to": "code-review" } }));
        assert!(matches!(rename, Edit::RenameTask { from, to } if from == "review" && to == "code-review"));
        assert!(matches!(edit(serde_json::json!({ "key": "remove_task", "value": "review" })), Edit::RemoveTask(t) if t == "review"));
        assert!(matches!(edit(serde_json::json!({ "key": "name", "value": null })), Edit::Name(None)));
        assert!(matches!(edit(serde_json::json!({ "key": "project_skippers", "value": true })), Edit::ProjectSkippers(true)));
        assert!(matches!(edit(serde_json::json!({ "key": "project_agent", "value": "codex" })), Edit::ProjectAgent(Some(a)) if a == "codex"));
    }

    /// The modes `Mode` in apps/desktop/src/appearance/skiffFolderDialog.ts sends.
    #[test]
    fn reads_the_folder_modes_the_page_sends() {
        use skiff_client::agent_folder::Mode;
        for (text, mode) in [("copy", Mode::Copy), ("empty", Mode::Empty), ("switch", Mode::Switch)] {
            assert_eq!(serde_json::from_value::<Mode>(serde_json::json!(text)).unwrap(), mode);
        }
    }
}

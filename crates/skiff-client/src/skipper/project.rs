//! Project and fleet Skipper setup and instructions.

use super::diff;
use super::folder::{check_setup, ensure, folder, lock, setup_in, skill_file, sync_skill, template, AGENTS_FILE};
use super::identity::{
    choose, continue_line, fleet_name, fleet_scope, name, project_name, with_model,
};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use skiff_core::{
    config::{self, Config, ProjectConfig},
    project::AgentInfo,
    session::{Role, SessionInfo, SessionSpec, SessionState},
};

use super::folder::{hash, past_agents, refresh, write_file, written};
use crate::{Client, Coordinated, Coordinator};

/// A project Skipper's `SKIPPER.md`: the project section only, as
/// [`TEMPLATE`] is the base of every Skipper. `{{project}}`, `{{path}}`,
/// `{{dir}}` and `{{folder}}` become the project, its folder, the
/// Skipper's folder and the Skiff agent's folder.
const PROJECT_TEMPLATE: &str = include_str!("../../agent-template/SKIPPER-project.md.tmpl");

/// A fleet Skipper's `SKIPPER.md`: the fleet section only. `{{fleet}}`,
/// `{{dir}}` and `{{folder}}` become the fleet, the Skipper's folder and
/// the Skiff agent's folder. It names no project: the fleet Skipper reads
/// them from `skiff projects --json`, so the file never goes stale.
const FLEET_TEMPLATE: &str = include_str!("../../agent-template/SKIPPER-fleet.md.tmpl");

/// The name of a project Skipper's file in its folder.
pub const SKIPPER_FILE: &str = "SKIPPER.md";

/// One instruction file: `AGENTS.md` for the Skipper and the base of every
/// Skipper, or the `SKIPPER.md` of a fleet or a project.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum Scope {
    Skipper,
    Fleet(String),
    Project(String),
}

impl Scope {
    /// Its path in the Skiff agent's folder.
    pub fn file(&self) -> String {
        match self {
            Scope::Skipper => AGENTS_FILE.to_string(),
            Scope::Fleet(f) => format!("fleets/{f}/{SKIPPER_FILE}"),
            Scope::Project(p) => format!("projects/{p}/{SKIPPER_FILE}"),
        }
    }

    /// How `skiff skipper template` names it. Always the full spelling: a
    /// bare name can be both a project and a fleet.
    pub fn arg(&self) -> String {
        match self {
            Scope::Skipper => "agents".to_string(),
            Scope::Fleet(f) => format!("fleet:{f}"),
            Scope::Project(p) => format!("project:{p}"),
        }
    }

    /// The command that shows how this file differs from its template, as a
    /// shell reads it. A project or fleet name can hold a space.
    pub fn diff_command(&self) -> String {
        let arg = self.arg();
        let plain = arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c));
        let arg = if plain { arg } else { format!("'{}'", arg.replace('\'', "'\\''")) };
        format!("skiff skipper template diff {arg}")
    }

    /// The text Skiff writes for it now.
    pub(crate) fn template(&self, cfg: &Config, top: &Path) -> Result<String> {
        Ok(match self {
            Scope::Skipper => template(&name(cfg)),
            Scope::Fleet(f) => {
                if !is_fleet(cfg, f) {
                    bail!("no fleet {f:?} in {}", config::config_path().display());
                }
                fleet_template(cfg, f, &fleet_folder(cfg, f)?, top)
            }
            Scope::Project(p) => project_template(cfg, project(cfg, p)?, &project_folder(cfg, p)?, top),
        })
    }
}

/// What an instruction file is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// No file.
    None,
    /// The file is what Skiff last wrote.
    Template,
    /// The person changed it.
    Edited,
    /// The person changed it, and the template is newer than Skiff's last write.
    EditedTemplateChanged,
}

/// One row of Settings › Skippers › Instructions.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileStatus {
    pub scope: Scope,
    /// Its path in the Skiff agent's folder.
    pub file: String,
    /// Its full path.
    pub path: PathBuf,
    pub status: Status,
    /// Unix time in milliseconds. `None` with no file.
    pub modified: Option<u64>,
    /// In bytes. `None` with no file.
    pub size: Option<u64>,
}


/// `SKIPPER.md` for the project `p`, whose Skipper's folder is `dir`, in
/// the Skiff agent's folder `folder`.
pub fn project_template(cfg: &Config, p: &ProjectConfig, dir: &Path, folder: &Path) -> String {
    PROJECT_TEMPLATE
        .replace("{{main}}", &name(cfg))
        .replace("{{project}}", &p.name)
        .replace("{{path}}", &p.path.display().to_string())
        .replace("{{dir}}", &dir.display().to_string())
        .replace("{{folder}}", &folder.display().to_string())
}

/// The configured project, by name.
pub fn project<'a>(cfg: &'a Config, name: &str) -> Result<&'a ProjectConfig> {
    cfg.projects.iter().find(|p| p.name == name).with_context(|| format!("no project {name:?} in {}", config::config_path().display()))
}

/// A project Skipper's folder: `projects/<project>` in the Skiff agent's
/// folder. Nothing goes into the project itself.
pub fn project_folder(cfg: &Config, project: &str) -> Result<PathBuf> {
    if project.is_empty() || project == "." || project == ".." || project.contains(['/', '\\']) {
        bail!("the project name {project:?} cannot name a folder");
    }
    Ok(folder(cfg)?.join("projects").join(project))
}

/// The agent a project Skipper starts, with its model: the project's
/// `skipper_agent` with the agent's default model, else `[skiff_agent]
/// project_agent`, else the Skipper's agent (see [`choose`]), each with
/// `project_model`. One that is not installed and on falls through to the
/// next.
pub fn project_agent<'a>(cfg: &Config, agents: &'a [AgentInfo], p: &ProjectConfig) -> Option<(&'a AgentInfo, Option<String>)> {
    let usable = |id: Option<&str>| agents.iter().find(|a| Some(a.id.as_str()) == id && a.installed && (a.enabled || a.custom));
    match usable(p.skipper_agent.as_deref()) {
        Some(a) => Some((a, None)),
        None => fleet_agent(cfg, agents),
    }
}

/// Makes a project Skipper's folder ready: the Skiff agent's folder first,
/// then `projects/<project>/` with `SKIPPER.md` and `results/`. Skiff
/// writes `SKIPPER.md` once, and again only while its status is template
/// (see [`refresh`]). Returns the folder. The caller holds [`lock`].
fn setup_project(cfg: &Config, p: &ProjectConfig) -> Result<PathBuf> {
    let top = setup_in(&folder(cfg)?, &name(cfg), &sync_skill()?)?;
    let dir = project_folder(cfg, &p.name)?;
    std::fs::create_dir_all(dir.join("results")).with_context(|| format!("create {}", dir.join("results").display()))?;
    refresh(&top, &Scope::Project(p.name.clone()).file(), &project_template(cfg, p, &dir, &top))?;
    Ok(dir)
}

/// The open projects of `fleet`, in config order.
pub fn fleet_projects<'a>(cfg: &'a Config, fleet: &str) -> Vec<&'a ProjectConfig> {
    cfg.projects.iter().filter(|p| !p.closed && p.fleet.as_deref() == Some(fleet)).collect()
}

/// A fleet Skipper's folder: `fleets/<fleet>` in the Skiff agent's folder.
pub fn fleet_folder(cfg: &Config, fleet: &str) -> Result<PathBuf> {
    if fleet.is_empty() || fleet == "." || fleet == ".." || fleet.contains(['/', '\\']) {
        bail!("the fleet name {fleet:?} cannot name a folder");
    }
    Ok(folder(cfg)?.join("fleets").join(fleet))
}

/// `SKIPPER.md` for the fleet `fleet`, whose Skipper's folder is `dir`, in
/// the Skiff agent's folder `folder`.
pub fn fleet_template(cfg: &Config, fleet: &str, dir: &Path, folder: &Path) -> String {
    FLEET_TEMPLATE
        .replace("{{main}}", &name(cfg))
        .replace("{{fleet}}", fleet)
        .replace("{{dir}}", &dir.display().to_string())
        .replace("{{folder}}", &folder.display().to_string())
}

/// The agent a fleet Skipper starts, with its model: `[skiff_agent]
/// project_agent`, else the Skipper's agent, with `project_model`. A
/// project's `skipper_agent` is for its project Skipper only.
pub fn fleet_agent<'a>(cfg: &Config, agents: &'a [AgentInfo]) -> Option<(&'a AgentInfo, Option<String>)> {
    let a = agents
        .iter()
        .find(|a| Some(a.id.as_str()) == cfg.skiff_agent.project_agent.as_deref() && a.installed && (a.enabled || a.custom))
        .or_else(|| choose(agents, cfg.skiff_agent.agent.as_deref()))?;
    Some((a, cfg.skiff_agent.project_model.clone()))
}

/// Makes a fleet Skipper's folder ready, with `results/` and `SKIPPER.md`.
/// Skiff writes `SKIPPER.md` once, and again only while its status is
/// template (see [`refresh`]). Returns the folder. The app and the watcher
/// call it when the fleet changes under a running fleet Skipper.
pub fn write_fleet(cfg: &Config, fleet: &str) -> Result<PathBuf> {
    let _lock = lock()?;
    write_fleet_in(cfg, fleet)
}

/// [`write_fleet`] for a caller that holds [`lock`]: a write to the marker
/// with no lock can lose another write's hash.
fn write_fleet_in(cfg: &Config, fleet: &str) -> Result<PathBuf> {
    let top = folder(cfg)?;
    let dir = fleet_folder(cfg, fleet)?;
    std::fs::create_dir_all(dir.join("results")).with_context(|| format!("create {}", dir.join("results").display()))?;
    refresh(&top, &Scope::Fleet(fleet.to_string()).file(), &fleet_template(cfg, fleet, &dir, &top))?;
    Ok(dir)
}

/// The first message a new project or fleet Skipper gets, built at each
/// start. It names the skill's file and both instruction files: an agent in a
/// project folder has no link to the skill and does not read the Skiff agent's
/// `AGENTS.md` by itself. It is plain instructions, so an agent with no skills
/// system can do it too, and one line: the send types it into the input box.
/// Each path is in backticks: a path can hold a space. The caller does not
/// hold [`lock`].
pub fn first_message(cfg: &Config, scope: &Scope) -> Result<String> {
    let top = folder(cfg)?;
    let mut text = format!(
        "Read the file `{}` before any skiff command: it is the skiff-cli skill. `skiff skill path` prints this path again. Read `{}`, then `{}`. {SKIPPER_FILE} wins where the two differ. Then do the First steps.",
        skill_file()?.display(),
        top.join(AGENTS_FILE).display(),
        top.join(scope.file()).display()
    );
    let stale: Vec<String> = [Scope::Skipper, scope.clone()]
        .iter()
        .filter_map(|s| file_status(cfg, s).ok())
        .filter(|f| f.status == Status::EditedTemplateChanged)
        .map(|f| format!("{} (`{}`)", f.file, f.scope.diff_command()))
        .collect();
    if !stale.is_empty() {
        text.push_str(&format!(
            " Tell the user that these files are edited and their templates changed: {}. Do not apply a template unless the user says so.",
            stale.join(", ")
        ));
    }
    Ok(text)
}

/// The status of the file of `scope`. Reads only.
pub fn file_status(cfg: &Config, scope: &Scope) -> Result<FileStatus> {
    let top = folder(cfg)?;
    status_in(&top, scope, &scope.template(cfg, &top)?)
}

fn status_in(top: &Path, scope: &Scope, text: &str) -> Result<FileStatus> {
    let file = scope.file();
    let path = top.join(&file);
    let (status, modified, size) = match std::fs::read(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Status::None, None, None),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        Ok(now) => {
            let m = std::fs::metadata(&path)?;
            let at = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
            // With no hash, the file is the template only when it is the same or a past template.
            let status = match written(top).get(&file) {
                _ if now == text.as_bytes() => Status::Template,
                Some(h) if *h == hash(&now) => Status::Template,
                Some(h) if *h == hash(text.as_bytes()) => Status::Edited,
                None if past_agents(&file, &now) => Status::Template,
                _ => Status::EditedTemplateChanged,
            };
            (status, at, Some(m.len()))
        }
    };
    Ok(FileStatus { scope: scope.clone(), file, path, status, modified, size })
}

/// Every instruction file, in tree order: the Skipper, each fleet, then
/// each open project, the projects in no fleet first.
pub fn instructions(cfg: &Config) -> Result<Vec<FileStatus>> {
    instructions_each(cfg)?.into_iter().collect()
}

/// [`instructions`] with a result for each file, so a file that cannot be
/// read does not hide the others. Fails only when the folder is refused.
pub fn instructions_each(cfg: &Config) -> Result<Vec<Result<FileStatus>>> {
    let top = folder(cfg)?;
    let open: Vec<&ProjectConfig> = cfg.projects.iter().filter(|p| !p.closed).collect();
    let mut fleets: Vec<&str> = Vec::new();
    for f in open.iter().filter_map(|p| p.fleet.as_deref()) {
        if !fleets.contains(&f) {
            fleets.push(f);
        }
    }
    let mut scopes = vec![Scope::Skipper];
    scopes.extend(fleets.iter().map(|f| Scope::Fleet(f.to_string())));
    scopes.extend(open.iter().filter(|p| p.fleet.is_none()).map(|p| Scope::Project(p.name.clone())));
    for f in &fleets {
        scopes.extend(open.iter().filter(|p| p.fleet.as_deref() == Some(f)).map(|p| Scope::Project(p.name.clone())));
    }
    Ok(scopes.iter().map(|s| status_in(&top, s, &s.template(cfg, &top)?)).collect())
}

/// Settings › Edit: the file's full path. A missing file is written from
/// the template first, with its folder.
pub fn edit_file(cfg: &Config, scope: &Scope) -> Result<PathBuf> {
    let _lock = lock()?;
    let top = setup_in(&folder(cfg)?, &name(cfg), &sync_skill()?)?;
    let file = scope.file();
    let text = scope.template(cfg, &top)?;
    if top.join(&file).symlink_metadata().is_err() {
        write_file(&top, &file, &text)?;
    }
    Ok(top.join(file))
}

/// Settings › Reset…: the template again. The old file goes to
/// `<name>.bak`, which replaces the last one. The file moves before setup,
/// because setup upgrades an older `AGENTS.md`, and `.bak` must hold the bytes
/// that were there at the start. So every check that can refuse comes first,
/// and a setup that fails moves the file back.
pub fn reset_file(cfg: &Config, scope: &Scope) -> Result<FileStatus> {
    let _lock = lock()?;
    let dir = folder(cfg)?;
    check_setup(&dir)?;
    scope.template(cfg, &dir)?;
    let skill = sync_skill()?;
    let file = scope.file();
    let path = dir.join(&file);
    let bak = path.with_file_name(format!("{}.bak", path.file_name().unwrap_or_default().to_string_lossy()));
    let moved = path.exists();
    if moved {
        std::fs::rename(&path, &bak).with_context(|| format!("move {} to {}", path.display(), bak.display()))?;
    }
    let top = setup_in(&dir, &name(cfg), &skill).inspect_err(|_| {
        if moved {
            let _ = std::fs::rename(&bak, &path);
        }
    })?;
    let text = scope.template(cfg, &top)?;
    write_file(&top, &file, &text)?;
    status_in(&top, scope, &text)
}

/// How the file of `scope` differs from its template, as a unified diff: `-`
/// lines are the file, `+` lines the template. Empty when they are the same.
/// A missing file counts as empty. Reads only.
pub fn template_diff(cfg: &Config, scope: &Scope) -> Result<String> {
    let top = folder(cfg)?;
    let text = scope.template(cfg, &top)?;
    let file = scope.file();
    let path = top.join(&file);
    let now = match std::fs::read(&path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    Ok(diff::unified(&now, &text, &file, "template"))
}

/// A project or fleet Skipper the CLI found or started.
pub struct ProjectSkipper {
    pub session: SessionInfo,
    /// It started now: it needs [`first_message`].
    pub created: bool,
    /// Its folder.
    pub dir: PathBuf,
}

/// The first skiffd that marks a project Skipper (`SessionSpec::skipper`).
/// An older one starts a plain agent instead, so nothing is started on it.
pub const PROJECT_SKIPPER_SINCE: &str = "0.6.25";

/// The first skiffd that retires a project Skipper (`Request::Retire`). A
/// fleet Skipper's takeover needs it, so an older one starts none.
pub const FLEET_SKIPPER_SINCE: &str = "0.6.28";

/// What [`open`] does with the coordinator it finds.
pub enum Open {
    /// Find it, or start it. A bare shell gets the agent: its last
    /// conversation when Settings says to continue on restore, else a new one.
    Start,
    /// A restore brought it back as a shell: continue its last conversation,
    /// when Settings says so. Never starts a session or a new conversation.
    /// `expect` is the `input_gen` the app read, so a line the person
    /// typed first stays theirs.
    Restore { expect: u64 },
}



/// The live project Skipper of `project` in `all`, if one runs.
pub fn find_project<'a>(all: &'a [SessionInfo], project: &str) -> Option<&'a SessionInfo> {
    all.iter().find(|s| s.skipper.as_deref() == Some(project) && s.state != SessionState::Done)
}

/// The live fleet Skipper of `fleet` in `all`, if one runs.
pub fn find_fleet<'a>(all: &'a [SessionInfo], fleet: &str) -> Option<&'a SessionInfo> {
    find_project(all, &fleet_scope(fleet))
}

/// The live project Skippers of the projects of `fleet`, in config order.
pub fn project_skippers_in<'a>(cfg: &Config, all: &'a [SessionInfo], fleet: &str) -> Vec<(String, &'a SessionInfo)> {
    fleet_projects(cfg, fleet).into_iter().filter_map(|p| find_project(all, &p.name).map(|s| (p.name.clone(), s))).collect()
}

/// The fleet of `project` when that fleet's Skipper runs.
pub fn commanding_fleet<'a>(cfg: &'a Config, all: &[SessionInfo], project: &str) -> Option<&'a str> {
    let fleet = cfg.projects.iter().find(|p| p.name == project)?.fleet.as_deref()?;
    find_fleet(all, fleet).is_some().then_some(fleet)
}

/// The fleet named `fleet` exists: an open project carries it.
pub fn is_fleet(cfg: &Config, fleet: &str) -> bool {
    !fleet_projects(cfg, fleet).is_empty()
}

/// skiffd `version` cannot mark a project Skipper.
pub fn too_old_for_project(version: &str) -> bool {
    skiff_core::compare_versions(version, PROJECT_SKIPPER_SINCE).is_lt()
}

/// Finds the live project Skipper of `project`, or starts one in the
/// project's folder. The setup lock keeps two starts apart, so a project
/// has at most one. Refuses before anything starts when skiffd is too old
/// to mark one.
pub async fn open_project(c: &Client, project_name: &str) -> Result<ProjectSkipper> {
    let cfg = config::load()?;
    let daemon = c.daemon_info().await?;
    if too_old_for_project(&daemon.version) {
        bail!("skiffd {} is older than this Skiff and cannot start a project Skipper. Update skiffd first", daemon.version);
    }
    let p = project(&cfg, project_name)?;
    let _lock = lock()?;
    let all = c.list_sessions().await?;
    if let Some(s) = find_project(&all, &p.name) {
        let want = self::project_name(&cfg, &p.name);
        let session = if s.name.as_deref() == Some(&want) { s.clone() } else { c.rename_session(&s.id, &want).await? };
        crate::fleet::restart_rows(&crate::ledger::path()?, &p.name, &session, &all)?;
        return Ok(ProjectSkipper { session, created: false, dir: setup_project(&cfg, p)? });
    }
    // One Skipper per project: a running fleet Skipper commands this one.
    if let Some(fleet) = commanding_fleet(&cfg, &all, &p.name) {
        bail!("{} is in {fleet}, which has a fleet Skipper. Run: skiff skipper --fleet {fleet}", p.name);
    }
    let dir = setup_project(&cfg, p)?;
    let agents = c.list_agents().await?;
    let Some((agent, model)) = project_agent(&cfg, &agents, p) else { bail!("No agent is on. Turn one on in Settings › Agents") };
    let mut s = spec(&with_model(agent, model.as_deref()), &self::project_name(&cfg, &p.name), p.path.clone());
    crate::cache::add_flags(&cfg, std::slice::from_ref(agent), &mut s);
    s.skipper = Some(p.name.clone());
    s.cols = 120;
    s.rows = 40;
    let info = c.create_session(s).await?;
    if info.skipper.as_deref() != Some(p.name.as_str()) {
        // Checked above by version; a daemon that still drops the mark
        // leaves nothing behind.
        let _ = c.kill(&info.id).await;
        bail!("skiffd did not mark the project Skipper. Nothing started. Update skiffd first");
    }
    crate::fleet::restart_rows(&crate::ledger::path()?, &p.name, &info, &all)?;
    Ok(ProjectSkipper { session: info, created: true, dir })
}

/// Runs `skiff skipper --project P --no-attach` with `cli` on `socket`: it
/// finds or starts the project Skipper, sends a new one its first message,
/// and starts the watcher. The app and `skiff-host` share it, so both start
/// one the same way. Returns the session id. It waits for a new agent's
/// input box, so it can take a while.
pub fn start_project(socket: &Path, cli: &Path, project: &str) -> Result<String> {
    let out = std::process::Command::new(cli)
        .arg("--socket")
        .arg(socket)
        .args(["skipper", "--project", project, "--no-attach"])
        // The caller is the person, not a pane.
        .env_remove("SKIFF_SESSION")
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("run {}", cli.display()))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        bail!("{}", msg.trim().trim_start_matches("skiff: ").trim_start_matches("Error: "));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().trim().to_string())
}

/// Runs `skiff skipper --fleet F --no-attach --yes` with `cli` on `socket`:
/// it finds or starts the fleet Skipper, takes the fleet's project Skippers
/// over (the person confirmed it first), sends a new one its first message,
/// and starts the watcher. The app and `skiff-host` share it. Returns the
/// session id.
pub fn start_fleet(socket: &Path, cli: &Path, fleet: &str) -> Result<String> {
    let out = std::process::Command::new(cli)
        .arg("--socket")
        .arg(socket)
        .args(["skipper", "--fleet", fleet, "--no-attach", "--yes"])
        // The caller is the person, not a pane.
        .env_remove("SKIFF_SESSION")
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("run {}", cli.display()))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        bail!("{}", msg.trim().trim_start_matches("skiff: ").trim_start_matches("Error: "));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().trim().to_string())
}

/// skiffd `version` cannot run a fleet Skipper's takeover.
pub fn too_old_for_fleet(version: &str) -> bool {
    skiff_core::compare_versions(version, FLEET_SKIPPER_SINCE).is_lt()
}

/// Finds the live fleet Skipper of `fleet`, or starts one in its folder.
/// It refuses before anything starts when skiffd is too old for a
/// takeover. It does not look at project Skippers in the fleet: the caller
/// takes them over first (see [`crate::fleet::takeover`]).
pub async fn open_fleet(c: &Client, fleet: &str) -> Result<ProjectSkipper> {
    let cfg = config::load()?;
    let daemon = c.daemon_info().await?;
    if too_old_for_fleet(&daemon.version) {
        bail!("skiffd {} is older than this Skiff and cannot start a fleet Skipper. Update skiffd first", daemon.version);
    }
    if !is_fleet(&cfg, fleet) {
        bail!("no fleet {fleet:?} in {}", config::config_path().display());
    }
    let _lock = lock()?;
    setup_in(&folder(&cfg)?, &name(&cfg), &sync_skill()?)?;
    let dir = write_fleet_in(&cfg, fleet)?;
    let all = c.list_sessions().await?;
    if let Some(s) = find_fleet(&all, fleet) {
        let want = fleet_name(&cfg, fleet);
        let session = if s.name.as_deref() == Some(&want) { s.clone() } else { c.rename_session(&s.id, &want).await? };
        crate::fleet::restart_rows(&crate::ledger::path()?, &fleet_scope(fleet), &session, &all)?;
        return Ok(ProjectSkipper { session, created: false, dir });
    }
    let agents = c.list_agents().await?;
    let Some((agent, model)) = fleet_agent(&cfg, &agents) else { bail!("No agent is on. Turn one on in Settings › Agents") };
    let mut s = spec(&with_model(agent, model.as_deref()), &fleet_name(&cfg, fleet), dir.clone());
    crate::cache::add_flags(&cfg, std::slice::from_ref(agent), &mut s);
    s.skipper = Some(fleet_scope(fleet));
    s.cols = 120;
    s.rows = 40;
    let info = c.create_session(s).await?;
    if info.skipper.as_deref() != Some(fleet_scope(fleet).as_str()) {
        bail!("skiffd is too old to mark a fleet Skipper. Run `skiff daemon reload`. Session {} runs as a plain agent", info.id);
    }
    crate::fleet::restart_rows(&crate::ledger::path()?, &fleet_scope(fleet), &info, &all)?;
    Ok(ProjectSkipper { session: info, created: true, dir })
}

/// Finds the live Skiff agent session, or starts it, through the daemon's
/// guarded request. `None` only for [`Open::Restore`] with no coordinator.
pub async fn open(c: &Client, how: Open) -> Result<Option<Coordinated>> {
    let cfg = config::load()?;
    let a = &cfg.skiff_agent;
    // Before any request: another socket must not resume the live agent's conversation either.
    folder(&cfg)?;
    let agents = c.list_agents().await?;
    let agent = choose(&agents, a.agent.as_deref());
    // Each line ends with the image cache's read flag, as the + menu adds it.
    let resume = agent
        .filter(|_| a.continue_on_restore)
        .and_then(|g| continue_line(&g.id, &with_model(g, a.model.as_deref())).map(|l| l + &crate::cache::flags_typed(&cfg, g)));
    let ask = match how {
        Open::Restore { expect } => Coordinator { resume, expect: Some(expect), ..Default::default() },
        Open::Start => {
            let Some(agent) = agent else { bail!("No agent is on. Turn one on in Settings › Agents") };
            // The folder is made ready first: files the user deleted come back.
            let dir = ensure(&cfg)?;
            let command = with_model(agent, a.model.as_deref());
            let mut s = spec(&command, &name(&cfg), dir);
            crate::cache::add_flags(&cfg, std::slice::from_ref(agent), &mut s);
            let start = format!("{command}{}", crate::cache::flags_typed(&cfg, agent));
            Coordinator { spec: Some(s), start: Some(start), resume, expect: None }
        }
    };
    c.coordinator(ask).await
}

/// A new session that runs `command` in `dir`, as the app's + menu starts one.
pub(super) fn spec(command: &str, name: &str, dir: PathBuf) -> SessionSpec {
    let mut words = command.split_whitespace().map(str::to_string);
    SessionSpec {
        label: name.to_string(),
        name: Some(name.to_string()),
        role: Role::Agent,
        cwd: Some(dir),
        command: words.next(),
        args: words.collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project Skipper needs a skiffd that marks it. The check is by
    /// version, before anything starts.
    #[test]
    fn an_older_skiffd_cannot_start_a_project_skipper() {
        assert!(too_old_for_project("0.6.24"));
        assert!(too_old_for_project("0.6.21"));
        assert!(!too_old_for_project(PROJECT_SKIPPER_SINCE));
        assert!(!too_old_for_project(skiff_core::VERSION), "this skiffd marks project Skippers");
        assert!(!too_old_for_project("0.7.0"));
    }
}

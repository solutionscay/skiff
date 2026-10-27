//! Projects from `projects.toml`, joined with their git worktrees.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::{config::ProjectConfig, git};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Short branch name. `None` when detached.
    pub branch: Option<String>,
    /// Commit sha.
    pub head: String,
    pub is_main: bool,
    pub locked: bool,
    pub prunable: bool,
}

/// A project in a git repository, or a plain folder with no git.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    #[default]
    Git,
    /// One worktree, the folder itself, with no branch and no changes.
    Folder,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub short: String,
    pub path: PathBuf,
    pub color: Option<String>,
    pub agents: Vec<String>,
    /// The rail image as a data URL, from `icon` or found in the repo.
    #[serde(default)]
    pub icon: Option<String>,
    /// The background image's absolute path, when the file exists.
    #[serde(default)]
    pub background: Option<String>,
    /// Pane opacity over the background image, in percent. Absent: the app default.
    #[serde(default)]
    pub background_opacity: Option<u8>,
    pub worktrees: Vec<Worktree>,
    /// The branch the main checkout normally holds: origin's HEAD, else `main` or `master`.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// Read again each time: a folder that has a `.git` then is a git project.
    #[serde(default)]
    pub kind: ProjectKind,
    /// Why `worktrees` is empty, e.g. the path is missing.
    pub error: Option<String>,
    /// Out of the rail until it opens again. Its worktrees are not read.
    #[serde(default)]
    pub closed: bool,
    /// The fleet it sits in on the rail. Absent: on the rail by itself.
    #[serde(default)]
    pub fleet: Option<String>,
}

impl Project {
    pub fn from_config(p: &ProjectConfig) -> Self {
        let mut kind = ProjectKind::Git;
        let (worktrees, error) = if p.closed {
            (Vec::new(), None)
        } else if !p.path.is_dir() {
            (Vec::new(), Some(format!("{} does not exist", p.path.display())))
        } else {
            match git::list_worktrees(&p.path) {
                Ok(w) => (w, None),
                // Git says it is in no repository: a plain folder, with the
                // folder as its only worktree. A missing or failing git shows its error.
                Err(_) if matches!(git::in_repository(&p.path), Ok(false)) => {
                    kind = ProjectKind::Folder;
                    (vec![folder_worktree(&p.path)], None)
                }
                Err(e) => (Vec::new(), Some(format!("{e:#}"))),
            }
        };
        Self {
            name: p.name.clone(),
            short: p.short(),
            path: p.path.clone(),
            color: p.color.clone(),
            agents: p.agents.clone(),
            icon: icon_file(&p.path, p.icon.as_deref()).and_then(|f| data_url(&f)),
            background: background_file(&p.path, p.background.as_deref()),
            background_opacity: p.background_opacity,
            default_branch: if kind == ProjectKind::Git && !worktrees.is_empty() { git::default_branch(&p.path) } else { None },
            worktrees,
            kind,
            error,
            closed: p.closed,
            fleet: p.fleet.clone().filter(|f| !f.trim().is_empty()),
        }
    }
}

/// The only worktree of a plain folder: the folder, with no branch or commit.
pub fn folder_worktree(path: &Path) -> Worktree {
    Worktree { path: path.to_path_buf(), branch: None, head: String::new(), is_main: true, locked: false, prunable: false }
}

/// Folders that hold a site's or app's icon, searched in order.
const ICON_DIRS: [&str; 12] = [
    "",
    "public",
    "static",
    "src",
    "app",
    "src/app",
    "assets",
    "assets/images",
    "src/assets",
    "src/assets/images",
    "public/images",
    "src-tauri/icons",
];

/// Icon file names, best first: vector, then large bitmaps, then the rest.
const ICON_NAMES: [&str; 11] = [
    "favicon.svg",
    "icon.svg",
    "apple-touch-icon.png",
    "icon.png",
    "favicon-32x32.png",
    "favicon-32.png",
    "favicon.png",
    "logo.svg",
    "logo.png",
    "favicon.ico",
    "icon.ico",
];

/// The configured background image, made absolute. `None` when unset or missing.
pub fn background_file(project: &Path, configured: Option<&str>) -> Option<String> {
    let p = Path::new(configured?.trim());
    let p = if p.is_absolute() { p.to_path_buf() } else { project.join(p) };
    p.is_file().then(|| p.display().to_string())
}

/// The icon file for a project: the configured one, else the first candidate.
pub fn icon_file(project: &Path, configured: Option<&str>) -> Option<PathBuf> {
    match configured.map(str::trim) {
        Some("") => None,
        Some(p) => {
            let p = Path::new(p);
            let p = if p.is_absolute() { p.to_path_buf() } else { project.join(p) };
            p.is_file().then_some(p)
        }
        None => detect_icon(project),
    }
}

/// The root first, then one level down and inside `apps/*` and `packages/*`,
/// where monorepos keep their front ends.
fn detect_icon(project: &Path) -> Option<PathBuf> {
    let found = |dir: &Path| {
        ICON_NAMES.iter().find_map(|n| {
            ICON_DIRS
                .iter()
                .map(|d| dir.join(d).join(n))
                .find(|p| p.is_file())
        })
    };
    if let Some(f) = found(project) {
        return Some(f);
    }
    let subdirs = |dir: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
                    .filter(|p| !matches!(p.file_name().and_then(|n| n.to_str()), Some("node_modules" | "target" | "dist" | "build")))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    let mut dirs = subdirs(project);
    for nest in ["apps", "packages"] {
        dirs.extend(subdirs(&project.join(nest)));
    }
    dirs.iter().find_map(|d| found(d))
}

/// Small images only: the rail shows them at 20px.
const MAX_ICON_BYTES: u64 = 512 * 1024;

/// A small image file as a data URL. Large or unknown files give `None`.
pub fn data_url(file: &Path) -> Option<String> {
    use base64::Engine as _;
    let mime = match file.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    if std::fs::metadata(file).ok()?.len() > MAX_ICON_BYTES {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

/// Reads `projects.toml` again on every call. Projects load in parallel:
/// each one runs git and searches its tree for an icon.
pub fn list() -> Result<Vec<Project>> {
    let projects = crate::config::load()?.projects;
    Ok(std::thread::scope(|s| {
        let handles: Vec<_> = projects
            .iter()
            .map(|p| s.spawn(|| Project::from_config(p)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    }))
}

pub fn find(name: &str) -> Result<ProjectConfig> {
    crate::config::load()?
        .projects
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| anyhow!("no such project: {name}"))
}

/// `<parent>/<dir>-worktrees/<branch with '/' as '-'>`.
pub fn worktree_path(project: &Path, branch: &str) -> PathBuf {
    let dir = project
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    project
        .parent()
        .unwrap_or(project)
        .join(format!("{dir}-worktrees"))
        .join(branch.replace('/', "-"))
}

/// Adds a worktree on a new branch and returns it as git reports it.
pub fn add_worktree(name: &str, branch: &str, base: Option<&str>) -> Result<Worktree> {
    let p = find(name)?;
    let path = worktree_path(&p.path, branch);
    git::add_worktree(&p.path, branch, &path, base)?;
    git::list_worktrees(&p.path)?
        .into_iter()
        .find(|w| same_path(&w.path, &path))
        .ok_or_else(|| anyhow!("git added {} but does not list it", path.display()))
}

/// The agents Skiff offers, in menu order, with their default command.
pub const KNOWN_AGENTS: [(&str, &str); 7] = [
    ("claude", "claude"),
    ("codex", "codex"),
    ("gemini", "agy"),
    ("grok", "grok"),
    ("opencode", "opencode"),
    ("hermes", "hermes"),
    ("kimi", "kimi"),
];

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub id: String,
    /// The command line the + menu runs: the user's, else the default.
    pub command: String,
    /// Empty for a custom agent.
    pub default_command: String,
    /// Added by the user, not one of `KNOWN_AGENTS`.
    #[serde(default)]
    pub custom: bool,
    /// Found on the daemon's PATH.
    pub installed: bool,
    /// Offered in the + menu. Only an installed agent can be enabled.
    pub enabled: bool,
    /// What starts this agent, as Settings › Agents shows it: "Used by task
    /// type review". It cannot be removed while this is set. Absent from an
    /// older skiffd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_by: Option<String>,
}

/// What in `cfg` starts agent `id`, in this order: the main Skipper, project and
/// fleet Skippers, a project's own Skipper (`skipper_agent`), a task type.
pub fn agent_uses(cfg: &crate::config::Config, id: &str) -> Vec<String> {
    let a = &cfg.skiff_agent;
    let is = |v: &Option<String>| v.as_deref().map(str::trim) == Some(id);
    let mut uses = Vec::new();
    if is(&a.agent) {
        uses.push(a.name());
    }
    if is(&a.project_agent) {
        uses.push("Project and fleet Skippers".into());
    }
    uses.extend(cfg.projects.iter().filter(|p| is(&p.skipper_agent)).map(|p| format!("project {}", p.name)));
    uses.extend(a.tasks.0.iter().filter(|(_, r)| r.agent.trim() == id).map(|(t, _)| format!("task type {t}")));
    uses
}

/// The one line for an agent in use: "Used by task type review", or with
/// more uses, "Used by Admiral and 2 more". `None`: nothing uses it.
pub fn used_by(cfg: &crate::config::Config, id: &str) -> Option<String> {
    let uses = agent_uses(cfg, id);
    let first = uses.first()?;
    Some(match uses.len() {
        1 => format!("Used by {first}"),
        n => format!("Used by {first} and {} more", n - 1),
    })
}

/// Sets [`AgentInfo::used_by`] from this process's config, for a list from
/// a skiffd that may be older than the config.
pub fn mark_used(list: &mut [AgentInfo]) {
    let cfg = crate::config::load().unwrap_or_default();
    for a in list {
        a.used_by = used_by(&cfg, &a.id);
    }
}

/// Every known agent with its installed and enabled state.
pub fn agents() -> Vec<AgentInfo> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let full = crate::config::load().unwrap_or_default();
    let cfg = &full.agents;
    // Known agents first, then the user's own, in the order the file sorts them.
    let custom: Vec<(&str, &str)> = cfg
        .commands
        .iter()
        .filter(|(id, c)| !c.trim().is_empty() && !KNOWN_AGENTS.iter().any(|&(k, _)| k == id.as_str()))
        .map(|(id, _)| (id.as_str(), ""))
        .collect();
    KNOWN_AGENTS
        .iter()
        .copied()
        .filter(|&(id, _)| !cfg.removed.iter().any(|r| r == id))
        .chain(custom)
        .map(|(id, default)| {
            let command = cfg
                .commands
                .get(id)
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| default.to_string());
            // Installed means the program, the first word, is on PATH.
            let first = command.split_whitespace().next().unwrap_or(default);
            let program = crate::alias::expand(first).map_or(first, |w| w[0].as_str());
            let installed = if program.contains('/') {
                is_executable(Path::new(program))
            } else {
                dirs.iter().any(|d| is_executable(&d.join(program)))
            };
            let on = cfg.enabled.as_ref().is_none_or(|e| e.iter().any(|x| x == id));
            AgentInfo {
                id: id.to_string(),
                command,
                default_command: default.to_string(),
                custom: !KNOWN_AGENTS.iter().any(|&(k, _)| k == id),
                installed,
                enabled: installed && on,
                used_by: used_by(&full, id),
            }
        })
        .collect()
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(x), Ok(y)) if x == y
        )
}

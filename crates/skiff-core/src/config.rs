//! `~/.config/skiff/projects.toml`. Declarative only: no plugins, no hooks.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub keys: Keys,
    #[serde(default)]
    pub agents: Agents,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub open: Open,
    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Appearance {
    /// The theme id the app and every terminal without its own theme use.
    /// Absent: Harbor.
    pub theme: Option<String>,
    /// Terminal text size in points; the app text scales with it. Absent: 13.
    pub font_size: Option<u8>,
}

/// Which apps Skiff hands things to.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Open {
    /// The command that prints a changed file's diff for the peek. `{target}`
    /// becomes what to compare. Absent: `git diff --color=always`.
    pub diff: Option<String>,
    /// Opens text files. `{path}` becomes the file. Absent: the OS default app.
    pub text: Option<String>,
    /// Opens `.md` files. Absent: the OS default app.
    pub markdown: Option<String>,
    /// Opens `.html` files. Absent: the OS default app.
    pub html: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Agents {
    /// Agent ids the + menu offers. Absent: every installed agent.
    pub enabled: Option<Vec<String>>,
    /// Command lines that replace the defaults, by agent id.
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Theme {
    /// A CSS file with custom properties. Relative to the config directory.
    pub file: Option<String>,
}

/// Key overrides by action: `palette = "ctrl+shift+p"`. The app holds the
/// defaults and the action names; unknown actions are ignored.
pub type Keys = std::collections::BTreeMap<String, String>;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    #[default]
    Stacked,
    SideBySide,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub cmd: String,
    pub port_env: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProjectConfig {
    pub name: String,
    /// Rail label, 3 characters. Derived from `name` when absent.
    pub short: Option<String>,
    pub path: PathBuf,
    pub color: Option<String>,
    /// Image for the rail: a path, relative to the project or absolute.
    /// Absent: detect one in the repo. Empty: no icon.
    pub icon: Option<String>,
    /// Image behind the terminals: a path, relative to the project or absolute.
    pub background: Option<String>,
    pub editor: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub layout: Layout,
    pub server: Option<ServerConfig>,
    /// Shows a Files tree under each worktree. The app reads and writes it;
    /// the daemon passes it by.
    #[serde(default)]
    pub files: bool,
    /// `false` hides the Changes section and the +/- counts. Absent: shown.
    pub changes: Option<bool>,
    /// Theme id for this project's chrome and its terminals. Absent: the app theme.
    pub theme: Option<String>,
}

impl ProjectConfig {
    pub fn short(&self) -> String {
        self.short.clone().unwrap_or_else(|| short_name(&self.name))
    }
}

/// The default rail label: the first 3 letters or digits, lower case.
pub fn short_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(3)
        .collect::<String>()
        .to_lowercase()
}

/// `SKIFF_CONFIG` overrides. Otherwise `<config dir>/skiff/projects.toml`.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("SKIFF_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skiff")
        .join("projects.toml")
}

/// Held across each read-modify-write of the file, so two edits at once do
/// not drop one another.
static EDIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn edit_lock() -> std::sync::MutexGuard<'static, ()> {
    EDIT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes a temp file beside the target and renames it over, so a reader
/// never sees a half-written file. Writes through a symlink to its target.
pub fn write_atomic(file: &Path, text: &str) -> Result<()> {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = file.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &file).with_context(|| format!("write {}", file.display()))
}

/// A missing file is an empty config, not an error.
pub fn load() -> Result<Config> {
    load_from(&config_path())
}

pub fn load_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    parse(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn parse(text: &str) -> Result<Config> {
    let mut cfg: Config = toml::from_str(text)?;
    for p in &mut cfg.projects {
        p.path = expand_home(&p.path);
    }
    Ok(cfg)
}

/// Accent colors for new projects, in order. Kept clear of the state colors.
const PALETTE: [&str; 8] = [
    "#b69cff", "#f28fd0", "#7ee0cb", "#e0c07e", "#9ec1ff", "#ff9e7a", "#c3e88d", "#d0c2ff",
];

/// What Skiff would add for `dir`, before anything is written.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FolderInfo {
    /// The git top level. `None` when `dir` is not in a repository.
    pub root: Option<PathBuf>,
    pub name: String,
    pub short: String,
    pub color: String,
    pub branch: Option<String>,
    pub worktrees: usize,
    /// The icon Skiff found in the repo, as a data URL, and its path
    /// relative to the root.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub icon_path: Option<String>,
    /// Why the folder cannot be added.
    pub error: Option<String>,
}

pub fn inspect_folder(dir: &Path) -> FolderInfo {
    let cfg = load().unwrap_or_default();
    let color = PALETTE
        .iter()
        .find(|c| !cfg.projects.iter().any(|p| p.color.as_deref() == Some(**c)))
        .unwrap_or(&PALETTE[cfg.projects.len() % PALETTE.len()])
        .to_string();
    let mut info = FolderInfo {
        root: None,
        name: String::new(),
        short: String::new(),
        color,
        branch: None,
        worktrees: 0,
        error: None,
        icon: None,
        icon_path: None,
    };
    let top = match crate::git::toplevel(&expand_home(dir)) {
        Ok(r) => r,
        Err(e) => {
            info.error = Some(format!("{e:#}"));
            return info;
        }
    };
    // Inside a linked worktree, the project is the main worktree.
    let worktrees = crate::git::list_worktrees(&top).unwrap_or_default();
    let root = worktrees
        .iter()
        .find(|w| w.is_main)
        .map(|w| w.path.clone())
        .unwrap_or(top);
    info.name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    info.short = short_name(&info.name);
    info.branch = worktrees.iter().find(|w| w.is_main).and_then(|w| w.branch.clone());
    info.worktrees = worktrees.len();
    if let Some(p) = cfg
        .projects
        .iter()
        .find(|p| crate::project::same_path(&p.path, &root))
    {
        info.error = Some(format!("Already added as the project {}.", p.name));
    }
    if let Some(f) = crate::project::icon_file(&root, None) {
        info.icon = crate::project::data_url(&f);
        info.icon_path = f.strip_prefix(&root).ok().map(|p| p.display().to_string());
    }
    info.root = Some(root);
    info
}

/// Appends a `[[project]]` for the git repository that contains `dir`.
/// Comments and layout in the file are kept.
pub fn add_project(
    dir: &Path,
    name: Option<String>,
    short: Option<String>,
    color: Option<String>,
    icon: Option<String>,
    agents: Vec<String>,
) -> Result<ProjectConfig> {
    let info = inspect_folder(dir);
    if let Some(e) = info.error {
        anyhow::bail!(e);
    }
    let path = info.root.expect("root is set when there is no error");
    let _edit = edit_lock();
    let cfg = load()?;
    if let Some(p) = cfg.projects.iter().find(|p| crate::project::same_path(&p.path, &path)) {
        anyhow::bail!("Already added as the project {}.", p.name);
    }
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or(info.name);
    if name.is_empty() {
        anyhow::bail!("type a project name");
    }
    if cfg.projects.iter().any(|p| p.name == name) {
        anyhow::bail!("a project named {name} already exists");
    }
    let short = short
        .map(|s| s.trim().chars().take(3).collect::<String>())
        .filter(|s| !s.is_empty() && *s != short_name(&name));
    let color = color.filter(|c| !c.is_empty()).unwrap_or(info.color);

    let file = config_path();
    let text = if file.exists() {
        std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?
    } else {
        String::new()
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", file.display()))?;
    let mut table = toml_edit::Table::new();
    table["name"] = toml_edit::value(name.as_str());
    if let Some(short) = &short {
        table["short"] = toml_edit::value(short.as_str());
    }
    table["path"] = toml_edit::value(home_relative(&path));
    table["color"] = toml_edit::value(color.as_str());
    // No key: detect. "" : no icon. A path: that file.
    if let Some(i) = &icon {
        table["icon"] = toml_edit::value(i.as_str());
    }
    // No list means every installed agent is offered.
    if !agents.is_empty() {
        let mut list = toml_edit::Array::new();
        for a in &agents {
            list.push(a.as_str());
        }
        table["agents"] = toml_edit::value(list);
    }
    doc.entry("project")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(Default::default()))
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow::anyhow!("`project` in {} is not [[project]] tables", file.display()))?
        .push(table);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_atomic(&file, &doc.to_string())?;

    Ok(ProjectConfig {
        name,
        short,
        path,
        color: Some(color),
        icon,
        background: None,
        editor: None,
        agents,
        layout: Layout::default(),
        server: None,
        files: false,
        changes: None,
        theme: None,
    })
}

/// Writes `[agents] enabled = [...]`, keeping the rest of the file.
pub fn set_enabled_agents(enabled: &[String]) -> Result<()> {
    let mut list = toml_edit::Array::new();
    for a in enabled {
        list.push(a.as_str());
    }
    edit_agents(|t| {
        t["enabled"] = toml_edit::value(list);
        Ok(())
    })
}

/// Sets `[agents.commands] <id> = "<command>"`. An empty command removes the
/// override, so the default applies again.
pub fn set_agent_command(id: &str, command: &str) -> Result<()> {
    let command = command.trim();
    edit_agents(|t| {
        let cmds = t
            .entry("commands")
            .or_insert_with(|| {
                let mut c = toml_edit::Table::new();
                c.set_implicit(false);
                toml_edit::Item::Table(c)
            })
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("`agents.commands` is not a table"))?;
        if command.is_empty() {
            cmds.remove(id);
        } else {
            cmds[id] = toml_edit::value(command);
        }
        Ok(())
    })
}

/// Sets or removes `icon` on the named project. `None` removes the key, so
/// Skiff detects an icon again; `Some("")` means no icon.
pub fn set_project_icon(project: &str, icon: Option<&str>) -> Result<()> {
    set_project_key(project, "icon", icon)
}

/// Sets or removes `background` on the named project.
pub fn set_project_background(project: &str, background: Option<&str>) -> Result<()> {
    set_project_key(project, "background", background)
}

/// Sets `color` on the named project.
pub fn set_project_color(project: &str, color: &str) -> Result<()> {
    let hex = color.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("not a #rrggbb color: {color}");
    }
    set_project_key(project, "color", Some(color))
}

/// Sets `changes = false` on the named project, or removes the key: shown is the default.
pub fn set_project_changes(project: &str, on: bool) -> Result<()> {
    set_project_key(project, "changes", (!on).then_some(false))
}

/// Sets `theme` on the named project, or removes the key: the app theme applies.
pub fn set_project_theme(project: &str, theme: Option<&str>) -> Result<()> {
    set_project_key(project, "theme", theme)
}

/// Sets `files = true` on the named project, or removes the key.
pub fn set_project_files(project: &str, on: bool) -> Result<()> {
    set_project_key(project, "files", on.then_some(true))
}

/// Rewrites the `[[project]]` order. Comments on each table move with it.
pub fn reorder_projects(order: &[String]) -> Result<()> {
    let _edit = edit_lock();
    let file = config_path();
    let text = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", file.display()))?;
    let tables = doc
        .get_mut("project")
        .and_then(|i| i.as_array_of_tables_mut())
        .ok_or_else(|| anyhow::anyhow!("no [[project]] in {}", file.display()))?;
    let name = |t: &toml_edit::Table| t.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    let mut old: Vec<toml_edit::Table> = tables.iter().cloned().collect();
    let positions: Vec<Option<_>> = old.iter().map(|t| t.position()).collect();
    let mut sorted = Vec::with_capacity(old.len());
    for n in order {
        if let Some(i) = old.iter().position(|t| name(t) == *n) {
            sorted.push(old.remove(i));
        }
    }
    sorted.append(&mut old);
    tables.clear();
    for (mut t, pos) in sorted.into_iter().zip(positions) {
        t.set_position(pos);
        tables.push(t);
    }
    write_atomic(&file, &doc.to_string())
}

fn set_project_key(project: &str, key: &str, value: Option<impl Into<toml_edit::Value>>) -> Result<()> {
    let _edit = edit_lock();
    let file = config_path();
    let text = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", file.display()))?;
    let tables = doc
        .get_mut("project")
        .and_then(|i| i.as_array_of_tables_mut())
        .ok_or_else(|| anyhow::anyhow!("no [[project]] in {}", file.display()))?;
    let table = tables
        .iter_mut()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(project))
        .ok_or_else(|| anyhow::anyhow!("no such project: {project}"))?;
    match value {
        Some(v) => table[key] = toml_edit::value(v),
        None => {
            table.remove(key);
        }
    }
    write_atomic(&file, &doc.to_string())
}

/// Sets `[appearance] theme`, or removes it for the default.
pub fn set_app_theme(theme: Option<&str>) -> Result<()> {
    edit_table("appearance", |t| {
        match theme {
            Some(v) => t["theme"] = toml_edit::value(v),
            None => {
                t.remove("theme");
            }
        }
        Ok(())
    })
}

/// Sets `[appearance] font_size`, or removes it for the default.
pub fn set_font_size(size: Option<u8>) -> Result<()> {
    edit_table("appearance", |t| {
        match size {
            Some(v) => t["font_size"] = toml_edit::value(i64::from(v)),
            None => {
                t.remove("font_size");
            }
        }
        Ok(())
    })
}

/// The keys `[open]` takes.
pub const OPEN_KEYS: [&str; 4] = ["diff", "text", "markdown", "html"];

/// Sets one `[open]` command, or removes it for the default.
pub fn set_open(key: &str, command: Option<&str>) -> Result<()> {
    if !OPEN_KEYS.contains(&key) {
        anyhow::bail!("unknown [open] key: {key}");
    }
    edit_table("open", |t| {
        match command {
            Some(v) => t[key] = toml_edit::value(v),
            None => {
                t.remove(key);
            }
        }
        Ok(())
    })
}

fn edit_agents(f: impl FnOnce(&mut toml_edit::Table) -> Result<()>) -> Result<()> {
    edit_table("agents", f)
}

/// Opens a top-level table in the config file, creating it above the projects.
fn edit_table(name: &str, f: impl FnOnce(&mut toml_edit::Table) -> Result<()>) -> Result<()> {
    let _edit = edit_lock();
    let file = config_path();
    let text = if file.exists() {
        std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?
    } else {
        String::new()
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", file.display()))?;
    let created = !doc.contains_key(name);
    let item = doc
        .entry(name)
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = item
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("`{name}` in {} is not a table", file.display()))?;
    f(table)?;
    if created {
        // A new table goes above the [[project]] list, not after it.
        table.set_position(Some(-1));
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_atomic(&file, &doc.to_string())
}

/// `~/x` for paths under the home directory, so the file moves between machines.
fn home_relative(p: &Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

fn expand_home(p: &Path) -> PathBuf {
    let Some(rest) = p.to_str().and_then(|s| s.strip_prefix("~/")) else {
        return p.to_path_buf();
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest),
        None => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
[theme]
file = "themes/harbor.css"

[keys]
palette      = "ctrl+shift+p"
next-waiting = "ctrl+shift+j"

[[project]]
name   = "skiff"
short  = "skf"
path   = "~/code/skiff"
color  = "#b69cff"
editor = "zed"
agents = ["claude", "codex"]
layout = "stacked"

[[project]]
name   = "storefront"
path   = "~/code/storefront"
color  = "#f28fd0"
agents = ["gemini"]
layout = "side-by-side"
server = { cmd = "npm run dev", port_env = "PORT" }
"##;

    #[test]
    fn parses_sample() {
        let cfg = parse(SAMPLE).unwrap();
        assert_eq!(cfg.projects.len(), 2);
        assert_eq!(cfg.projects[0].short(), "skf");
        assert_eq!(cfg.projects[1].short(), "sto");
        assert_eq!(cfg.projects[1].layout, Layout::SideBySide);
        assert_eq!(cfg.keys.get("palette").map(String::as_str), Some("ctrl+shift+p"));
        assert!(!cfg.projects[0].path.to_string_lossy().starts_with('~'));
        assert_eq!(
            cfg.projects[1].server.as_ref().unwrap().port_env.as_deref(),
            Some("PORT")
        );
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(parse("").unwrap(), Config::default());
    }
}

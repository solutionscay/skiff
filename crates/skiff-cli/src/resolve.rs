//! Finds a session from what the user typed: a full id, a unique id prefix,
//! or an exact name. Groups the same way. `.` is the pane this runs in, from
//! `SKIFF_SESSION`, or the group that holds it.

use std::borrow::Cow;

use anyhow::{anyhow, bail, Result};
use skiff_core::{
    group::Group,
    session::{Role, SessionInfo},
};

/// What the app calls a session's program: its file name, else the role.
pub fn agent_name(s: &SessionInfo) -> String {
    if s.command.is_empty() {
        return crate::sessions::role(s.role).to_string();
    }
    std::path::Path::new(&s.command)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| s.command.clone())
}

/// The name the sidebar shows, as the app's `taskTitle`: the user's name;
/// then for an agent its label before its title, for others the title
/// first; else the program.
pub fn display_name(s: &SessionInfo) -> Cow<'_, str> {
    let title = s.title.as_deref().unwrap_or("");
    let (first, second) = if s.role == Role::Agent { (s.label.as_str(), title) } else { (title, s.label.as_str()) };
    [s.name.as_deref().unwrap_or(""), first, second]
        .into_iter()
        .find(|n| !n.is_empty())
        .map_or_else(|| Cow::Owned(agent_name(s)), Cow::Borrowed)
}

/// One session for `arg`. No match or more than one is an error that names
/// the candidates.
pub fn session<'a>(all: &'a [SessionInfo], arg: &str) -> Result<&'a SessionInfo> {
    if arg.is_empty() {
        bail!("empty session argument");
    }
    if arg == "." {
        let id = here()?;
        return all.iter().find(|s| s.id == id).ok_or_else(|| anyhow!("this pane's session {id} is gone"));
    }
    if let Some(s) = all.iter().find(|s| s.id == arg) {
        return Ok(s);
    }
    let found: Vec<&SessionInfo> = all
        .iter()
        .filter(|s| s.id.starts_with(arg) || display_name(s) == arg)
        .collect();
    match found.as_slice() {
        [s] => Ok(s),
        [] => bail!("no session matches {arg:?}"),
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|s| format!("  {}  {}", crate::print::short_id(&s.id), display_name(s)))
                .collect();
            bail!("{arg:?} matches {} sessions:\n{}", many.len(), list.join("\n"))
        }
    }
}

/// One group for `arg`: a full id, a unique id prefix, or an exact name.
pub fn group<'a>(all: &'a [Group], arg: &str) -> Result<&'a Group> {
    if arg.is_empty() {
        bail!("empty group argument");
    }
    if arg == "." {
        let id = here()?;
        return all
            .iter()
            .find(|g| g.layout.sessions().contains(&id))
            .ok_or_else(|| anyhow!("this pane is in no group"));
    }
    if let Some(g) = all.iter().find(|g| g.id == arg) {
        return Ok(g);
    }
    let found: Vec<&Group> = all
        .iter()
        .filter(|g| g.id.starts_with(arg) || g.name == arg)
        .collect();
    match found.as_slice() {
        [g] => Ok(g),
        [] => bail!("no group matches {arg:?}"),
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|g| format!("  {}  {}", crate::print::short_id(&g.id), g.name))
                .collect();
            bail!("{arg:?} matches {} groups:\n{}", many.len(), list.join("\n"))
        }
    }
}

/// The session this process runs in. skiffd sets `SKIFF_SESSION` in every pane.
pub fn here() -> Result<String> {
    std::env::var("SKIFF_SESSION")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("`.` works only inside a Skiff pane (SKIFF_SESSION is not set)"))
}

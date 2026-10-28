//! `skiff projects add` and `remove`: the app's Add project and Remove, from a shell.

use std::path::Path;

use anyhow::{Result, bail};
use skiff_core::{
    config,
    project::{Project, ProjectKind},
    session::{SessionInfo, SessionState},
};

use crate::{print, resolve};

/// Adds the git repository that holds `path`, or the plain folder, with the
/// app's defaults. A closed project for it opens again with its own settings.
pub fn add(path: &Path, name: Option<String>, fleet: Option<String>, json: bool) -> Result<()> {
    let before = config::load()?.projects;
    let p = config::add_project(path, name, None, None, None, Vec::new(), fleet)?;
    // A new project cannot take a name in use: a known name is a reopen.
    let reopened = before.iter().any(|b| b.name == p.name);
    let shown = Project::from_config(&p);
    if reopened {
        let line = format!("Opened the closed project {} again.", p.name);
        if json { eprintln!("{line}") } else { println!("{line}") }
    }
    if json {
        return print::json(&shown);
    }
    if !reopened {
        println!("Added the project {}: {}, {}.", p.name, kind(&shown), print::tilde(&p.path));
    }
    Ok(())
}

fn kind(p: &Project) -> &'static str {
    match p.kind {
        ProjectKind::Git => "git",
        ProjectKind::Folder => "folder",
    }
}

/// Drops the project's `[[project]]` table. The folder and its worktrees
/// stay. Running sessions in it refuse the remove, unless `end_sessions`
/// ends them first, as the app's Remove does after its confirmation.
pub async fn remove(socket: &Path, name: &str, end_sessions: bool) -> Result<()> {
    let target = skiff_core::project::find(name)?;
    // A closed project has no sessions in the app's tree: the app removes it as it is.
    let mine = if target.closed || !socket.exists() {
        Vec::new()
    } else {
        let c = crate::connect(socket).await?;
        let all = c.list_sessions().await?;
        let ids = sessions_in(name, &all)?;
        let running: Vec<&SessionInfo> = all
            .iter()
            .filter(|s| ids.contains(&s.id) && s.state != SessionState::Done)
            .collect();
        if !running.is_empty() && !end_sessions {
            let names: Vec<String> = running.iter().map(|s| format!("{} ({})", resolve::display_name(s), print::short_id(&s.id))).collect();
            bail!(
                "{name} has {} running: {}. Pass --end-sessions to end {} and remove the project. Nothing changed",
                plural(running.len()),
                names.join(", "),
                if running.len() == 1 { "it" } else { "them" }
            );
        }
        // Ends every session of the project, as the app does. One that will
        // not end keeps the project as it was.
        for id in &ids {
            if let Err(e) = c.kill(id).await {
                bail!("could not end session {}: {e:#}. {name} stays", print::short_id(id));
            }
        }
        ids
    };
    config::remove_project(name)?;
    let ended = if mine.is_empty() { String::new() } else { format!(" Ended {}.", plural(mine.len())) };
    println!("Removed the project {name}. The folder {} stays on disk.{ended}", print::tilde(&target.path));
    Ok(())
}

fn plural(n: usize) -> String {
    format!("{n} session{}", if n == 1 { "" } else { "s" })
}

/// The ids of the sessions the app's tree shows under the project: each
/// session sits in the deepest open worktree that holds its folder.
fn sessions_in(name: &str, all: &[SessionInfo]) -> Result<Vec<String>> {
    let projects: Vec<Project> = skiff_core::project::list()?.into_iter().filter(|p| !p.closed).collect();
    Ok(all
        .iter()
        .filter(|s| home(&projects, &s.cwd) == Some(name))
        .map(|s| s.id.clone())
        .collect())
}

/// The project of the deepest worktree that holds `cwd`.
fn home<'a>(projects: &'a [Project], cwd: &Path) -> Option<&'a str> {
    projects
        .iter()
        .flat_map(|p| p.worktrees.iter().map(move |w| (p, w)))
        .filter(|(_, w)| cwd.starts_with(&w.path))
        .max_by_key(|(_, w)| w.path.as_os_str().len())
        .map(|(p, _)| p.name.as_str())
}

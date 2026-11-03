//! Outcomes and expected evidence (#247): how a done task ended, and the
//! check of the evidence it names. Each check says what it shows and no more.

use std::path::Path;

use anyhow::{bail, Result};
use skiff_client::ledger::{EXPECTS, OUTCOMES, SUCCEEDED, UNOBSERVED};

use super::Row;

pub(super) fn outcome(s: &str) -> Result<&'static str> {
    match OUTCOMES.iter().find(|x| **x == s) {
        Some(x) => Ok(x),
        None => bail!("unknown outcome {s:?}. Use one of: {}", OUTCOMES.join(", ")),
    }
}

pub(super) fn expects(s: &str) -> Result<&'static str> {
    match EXPECTS.iter().find(|x| **x == s) {
        Some(x) => Ok(x),
        None => bail!("unknown evidence {s:?}. Use one of: {}", EXPECTS.join(", ")),
    }
}

/// What the check of a task's expected evidence found.
#[derive(Debug, PartialEq)]
pub(super) enum Evidence {
    /// The evidence is there. The text says what that shows.
    Seen(String),
    /// The evidence is not there.
    Missing(String),
    /// The check could not run from here.
    Unchecked(String),
}

/// Checks the evidence `r` expects, on this Host: the daemon at `here`. A
/// task with no expectation gives `None`.
pub(super) fn check(r: &Row, here: &Path) -> Option<Evidence> {
    let e = r.expects.as_deref()?;
    if let Some(s) = r.socket_other_than(here) {
        return Some(Evidence::Unchecked(format!("it ran on another daemon, {s}: its {e} was not checked")));
    }
    Some(match e {
        "commit" => commit(r),
        _ => result(r),
    })
}

fn result(r: &Row) -> Evidence {
    match r.result_path.as_deref() {
        Some(p) if Path::new(p).is_file() => Evidence::Seen(format!("the result file {p} exists on this Host. That does not show the result is right")),
        Some(p) => Evidence::Missing(format!("the result file {p} does not exist on this Host")),
        None => Evidence::Missing("it has no result path".into()),
    }
}

fn commit(r: &Row) -> Evidence {
    let Some(c) = r.commit_ref.as_deref() else {
        return Evidence::Missing("it names no commit. Give one with --commit REF".into());
    };
    let cfg = skiff_core::config::load().unwrap_or_default();
    let Some(dir) = cfg.projects.iter().find(|p| p.name == r.project).map(|p| p.path.clone()) else {
        return Evidence::Unchecked(format!("no project folder for {:?}: commit {c} was not looked up", r.project));
    };
    commit_in(&dir, c)
}

/// Whether `c` names a commit in the git repository at `dir`.
pub(super) fn commit_in(dir: &Path, c: &str) -> Evidence {
    let found = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--verify", "--quiet", "--end-of-options"])
        .arg(format!("{c}^{{commit}}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let dir = dir.display();
    if found {
        Evidence::Seen(format!("commit {c} is in the repository at {dir}. That does not show it was pushed"))
    } else {
        Evidence::Missing(format!("commit {c} is not in the repository at {dir}"))
    }
}

/// The outcome a task takes when it is set done: `given`, else succeeded,
/// or unobserved when its evidence is missing.
pub(super) fn settle(given: Option<&'static str>, found: Option<&Evidence>) -> &'static str {
    match (given, found) {
        (Some(o), _) => o,
        (None, Some(Evidence::Missing(_))) => UNOBSERVED,
        _ => SUCCEEDED,
    }
}

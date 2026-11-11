//! The read side of schedules (#262): a schedule is a row of the ledger's
//! `schedules` table, and each run is an ordinary `tasks` row with the
//! schedule's uid in `tasks.schedule`. The agent step is a child row of the
//! run. `skiff schedule` writes them; the CLI, `skiff-host` and the app read
//! them with this code.

pub mod cron;
pub mod when;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use skiff_core::session::{SessionInfo, SessionState};

use crate::ledger::{rows_with, session_of, Row, FAILED, INTERRUPTED_OUTCOME, UNOBSERVED};
pub use when::When;

/// The table, as `skiff schedule` makes it. Ledger version 14 made it with a
/// `text` column; version 15 moved each prompt to its file (see [`prompt_file`]).
pub const SCHEDULES: &str = "
CREATE TABLE IF NOT EXISTS schedules (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  uid          TEXT NOT NULL UNIQUE,
  name         TEXT NOT NULL UNIQUE,
  cron         TEXT NOT NULL,
  command      TEXT,
  timeout      INTEGER,
  gate         TEXT CHECK (gate IN ('always', 'on-fail', 'on-output')),
  target       TEXT CHECK (target IN ('type', 'pane', 'skipper')),
  project      TEXT,
  type         TEXT,
  pane         TEXT,
  owner        TEXT,
  paused_at    TEXT,
  created_at   TEXT NOT NULL,
  next_at      TEXT,
  skipped      INTEGER NOT NULL DEFAULT 0,
  last_skip_at TEXT
);
";

/// The command's time limit when none is set, in seconds.
pub const TIMEOUT: i64 = 600;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Schedule {
    pub id: i64,
    pub uid: String,
    pub name: String,
    pub cron: String,
    /// Run by skiffd in the project folder. `None`: prompt only.
    pub command: Option<String>,
    /// Seconds, for the command. `None`: [`TIMEOUT`].
    pub timeout: Option<i64>,
    /// `always`, `on-fail` or `on-output`: whether the agent step runs after
    /// the command. `None`: always.
    pub gate: Option<String>,
    /// `type`, `pane` or `skipper`. `None`: command only.
    pub target: Option<String>,
    pub project: Option<String>,
    #[serde(rename = "type")]
    pub task_type: Option<String>,
    pub pane: Option<String>,
    /// `grand` or a project, for a Skipper target.
    pub owner: Option<String>,
    pub paused_at: Option<String>,
    pub created_at: String,
    /// Set by the scheduler: the next run, UTC.
    pub next_at: Option<String>,
    /// Runs skipped because a run was still open.
    pub skipped: i64,
    pub last_skip_at: Option<String>,
}

const COLUMNS: &str = "id, uid, name, cron, command, timeout, gate, target, project, type, pane, owner, paused_at, created_at, next_at, skipped, last_skip_at";

fn map(r: &rusqlite::Row) -> rusqlite::Result<Schedule> {
    Ok(Schedule {
        id: r.get(0)?,
        uid: r.get(1)?,
        name: r.get(2)?,
        cron: r.get(3)?,
        command: r.get(4)?,
        timeout: r.get(5)?,
        gate: r.get(6)?,
        target: r.get(7)?,
        project: r.get(8)?,
        task_type: r.get(9)?,
        pane: r.get(10)?,
        owner: r.get(11)?,
        paused_at: r.get(12)?,
        created_at: r.get(13)?,
        next_at: r.get(14)?,
        skipped: r.get(15)?,
        last_skip_at: r.get(16)?,
    })
}

fn has_table(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'schedules'", [], |r| r.get::<_, i64>(0)).map(|n| n > 0)
}

/// The folder of the prompt files, in the Skiff agent's folder, beside `types/`.
pub const PROMPTS: &str = "schedules";

/// A schedule's prompt file: `schedules/<uid>.md` in the agent's folder `dir`.
/// The uid never changes, so a rename keeps the file. Skiff names the file.
/// The person writes it.
pub fn prompt_file(dir: &Path, uid: &str) -> Result<PathBuf> {
    if uid.is_empty() || uid.len() > 16 || !uid.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("{uid:?} is not a schedule uid");
    }
    Ok(dir.join(PROMPTS).join(format!("{uid}.md")))
}

/// What Edit writes when the file is missing: a `# <name>` line. A heading
/// line is not part of the prompt.
pub fn stub(name: &str) -> String {
    format!("# {name}
")
}

/// The prompt in `text`: the file's text without a first `# ` heading line.
/// `None`: empty, or the heading alone.
pub fn prompt_of(text: &str) -> Option<String> {
    let t = text.trim_start();
    let body = match t.strip_prefix("# ") {
        Some(rest) => rest.split_once('\n').map(|(_, b)| b).unwrap_or_default(),
        None => t,
    };
    let body = body.trim();
    (!body.is_empty()).then(|| body.to_string())
}

/// The prompt of schedule `uid`, read from its file now. `None`: no file, or
/// no prompt in it.
pub fn prompt(dir: &Path, uid: &str) -> Option<String> {
    let bytes = std::fs::read(prompt_file(dir, uid).ok()?).ok()?;
    prompt_of(&String::from_utf8_lossy(&bytes))
}

/// The prompt's size in bytes, for the page. `None`: no prompt.
pub fn prompt_size(dir: &Path, uid: &str) -> Option<u64> {
    prompt(dir, uid).map(|p| p.len() as u64)
}

/// Makes the file with its stub when it is missing, for Edit. Keeps a file
/// that is there. Returns the file.
pub fn edit_prompt_file(dir: &Path, uid: &str, name: &str) -> Result<PathBuf> {
    let f = prompt_file(dir, uid)?;
    let folder = dir.join(PROMPTS);
    std::fs::create_dir_all(&folder).with_context(|| format!("make {}", folder.display()))?;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&f) {
        Ok(mut out) => std::io::Write::write_all(&mut out, stub(name).as_bytes()).with_context(|| format!("write {}", f.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("make {}", f.display())),
    }
    Ok(f)
}

/// Writes `text` as the prompt of schedule `uid`, under its `# <name>` line.
/// Empty text leaves the stub alone: no prompt.
pub fn write_prompt(dir: &Path, uid: &str, name: &str, text: &str) -> Result<PathBuf> {
    let f = prompt_file(dir, uid)?;
    let folder = dir.join(PROMPTS);
    std::fs::create_dir_all(&folder).with_context(|| format!("make {}", folder.display()))?;
    let body = text.trim();
    let all = if body.is_empty() { stub(name) } else { format!("{}\n{body}\n", stub(name)) };
    std::fs::write(&f, all).with_context(|| format!("write {}", f.display()))?;
    Ok(f)
}

/// Every schedule, by name. A ledger from before schedules has none.
pub fn list(db: &Connection) -> Result<Vec<Schedule>> {
    if !has_table(db)? {
        return Ok(Vec::new());
    }
    let mut st = db.prepare(&format!("SELECT {COLUMNS} FROM schedules ORDER BY name"))?;
    let out = st.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(out)
}

/// The schedule named `name`, or with the uid `name`.
pub fn get(db: &Connection, name: &str) -> Result<Option<Schedule>> {
    if !has_table(db)? {
        return Ok(None);
    }
    Ok(db.query_row(&format!("SELECT {COLUMNS} FROM schedules WHERE name = ?1 OR uid = ?1"), [name], map).optional()?)
}

/// The runs of schedule `uid`, newest first, at most `limit`, each with the
/// rows under it.
pub fn runs(db: &Connection, uid: &str, limit: i64) -> Result<Vec<(Row, Vec<Row>)>> {
    let has: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name = 'schedule'", [], |r| r.get(0))?;
    if has == 0 {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = {
        let mut st = db.prepare("SELECT id FROM tasks WHERE schedule = ?1 ORDER BY id DESC LIMIT ?2")?;
        let out = st.query_map(rusqlite::params![uid, limit], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        out
    };
    let Some(oldest) = ids.last() else { return Ok(Vec::new()) };
    let all = rows_with(db, "WHERE id >= ?1 AND (schedule = ?2 OR parent IN (SELECT uid FROM tasks WHERE schedule = ?2))", &[oldest, &uid])?;
    let mut out: Vec<(Row, Vec<Row>)> = all.iter().filter(|r| r.schedule.is_some()).map(|r| (r.clone(), Vec::new())).collect();
    for k in all.iter().filter(|r| r.schedule.is_none()) {
        if let Some(run) = out.iter_mut().find(|(r, _)| Some(&r.uid) == k.parent.as_ref()) {
            run.1.push(k.clone());
        }
    }
    out.reverse();
    Ok(out)
}

/// What came of a run, for the dot and the Last column.
pub const PENDING: &str = "pending";
pub const RUNNING: &str = "running";
pub const SKIPPED: &str = "skipped";

/// The agent of `k` is still on it: the row is open, and its pane still
/// works or waits for the person. Without the session list, an open row counts.
fn agent_active(k: &Row, all: Option<&[SessionInfo]>) -> bool {
    if k.state == "done" {
        return false;
    }
    if k.state == "pending" {
        return true;
    }
    let Some(all) = all else { return true };
    k.pane_id.as_deref().and_then(|p| session_of(all, p)).is_some_and(|s| matches!(s.state, SessionState::Working | SessionState::Waiting))
}

/// `pending`, `running`, `failed` or `succeeded`.
pub fn run_status(run: &Row, kids: &[Row], all: Option<&[SessionInfo]>) -> &'static str {
    if run.state == "pending" {
        return PENDING;
    }
    if run.state != "done" {
        return RUNNING;
    }
    let bad = |r: &Row| matches!(r.outcome.as_deref(), Some(FAILED | UNOBSERVED | INTERRUPTED_OUTCOME));
    if bad(run) {
        return FAILED;
    }
    if kids.iter().any(|k| agent_active(k, all)) {
        return RUNNING;
    }
    if kids.iter().any(bad) {
        return FAILED;
    }
    crate::ledger::SUCCEEDED
}

/// A run is open while it is pending or running: the next one is skipped.
pub fn is_open(status: &str) -> bool {
    matches!(status, PENDING | RUNNING)
}

/// A schedule as `skiff schedule ls --json` and the app show it.
#[derive(Clone, Serialize)]
pub struct Summary {
    #[serde(flatten)]
    pub schedule: Schedule,
    /// The When sentence in words.
    pub when: String,
    /// No sentence shows the cron: the app shows it after the words.
    pub custom: bool,
    /// The next run in local time, `Nov 11 01:30`. `None` while paused.
    pub next: Option<String>,
    /// What came of the last run, or `skipped` when the last due time was
    /// skipped. `None`: no run yet.
    pub last: Option<&'static str>,
    /// A run is open: Run now is off.
    pub open: bool,
    /// The prompt file, relative to the agent's folder: `schedules/<uid>.md`.
    pub prompt_file: String,
    /// The prompt's size in bytes. `None`: no prompt.
    pub prompt: Option<u64>,
    /// The newest runs, newest first, each with its child rows.
    pub runs: Vec<RunView>,
}

#[derive(Clone, Serialize)]
pub struct RunView {
    #[serde(flatten)]
    pub row: Row,
    /// `pending`, `running`, `failed` or `succeeded`.
    pub status: &'static str,
    /// The run's start in local time.
    pub at: String,
    pub kids: Vec<Row>,
}

/// The local time of a ledger timestamp.
pub fn local_of(utc: &str) -> String {
    cron::parse_utc(utc).map(cron::short_time).unwrap_or_default()
}

/// Every schedule with its words, next run, last outcome and its newest
/// `keep` runs. `all`: the Host's sessions, for running agent steps.
/// `dir`: the agent's folder, for the prompt files.
pub fn summaries(db: &Connection, dir: &Path, all: Option<&[SessionInfo]>, keep: i64) -> Result<Vec<Summary>> {
    list(db)?.into_iter().map(|s| summary(db, dir, s, all, keep)).collect()
}

pub fn summary(db: &Connection, dir: &Path, s: Schedule, all: Option<&[SessionInfo]>, keep: i64) -> Result<Summary> {
    let w = When::from_cron(&s.cron);
    let runs: Vec<RunView> = runs(db, &s.uid, keep.max(1))?
        .into_iter()
        .map(|(row, kids)| RunView { status: run_status(&row, &kids, all), at: local_of(&row.sent_at), row, kids })
        .collect();
    let newest = runs.first();
    let open = newest.is_some_and(|r| is_open(r.status));
    let skipped_last = match (&s.last_skip_at, newest) {
        (Some(skip), Some(r)) => skip.as_str() > r.row.sent_at.as_str(),
        (Some(_), None) => true,
        _ => false,
    };
    let last = if skipped_last && !open { Some(SKIPPED) } else { newest.map(|r| r.status) };
    let next = if s.paused_at.is_some() { None } else { s.next_at.as_deref().map(local_of) };
    let runs = if keep == 0 { Vec::new() } else { runs };
    let prompt_file = format!("{PROMPTS}/{}.md", s.uid);
    let prompt = prompt_size(dir, &s.uid);
    Ok(Summary { when: w.words(), custom: w.is_custom(), next, last, open, runs, prompt_file, prompt, schedule: s })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The heading line is the file's name for the person; the prompt is the rest.
    #[test]
    fn the_prompt_is_the_file_without_its_heading() {
        assert_eq!(prompt_of("# nightly\n"), None);
        assert_eq!(prompt_of("# nightly\n\nFix the tests.\n"), Some("Fix the tests.".into()));
        assert_eq!(prompt_of("Fix the tests."), Some("Fix the tests.".into()));
        assert_eq!(prompt_of("  \n"), None);
        assert!(prompt_file(Path::new("/a"), "../x").is_err());
    }
}

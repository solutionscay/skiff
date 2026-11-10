//! The read side of schedules (#262): a schedule is a row of the ledger's
//! `schedules` table, and each run is an ordinary `tasks` row with the
//! schedule's uid in `tasks.schedule`. The agent step is a child row of the
//! run. `skiff schedule` writes them; the CLI, `skiff-host` and the app read
//! them with this code.

pub mod cron;
pub mod when;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use skiff_core::session::{SessionInfo, SessionState};

use crate::ledger::{rows_with, session_of, Row, FAILED, INTERRUPTED_OUTCOME, UNOBSERVED};
pub use when::When;

/// The table, as `skiff schedule` makes it. Ledger version 14.
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
  text         TEXT,
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
    /// The prompt.
    pub text: Option<String>,
    pub paused_at: Option<String>,
    pub created_at: String,
    /// Set by the scheduler: the next run, UTC.
    pub next_at: Option<String>,
    /// Runs skipped because a run was still open.
    pub skipped: i64,
    pub last_skip_at: Option<String>,
}

const COLUMNS: &str = "id, uid, name, cron, command, timeout, gate, target, project, type, pane, owner, text, paused_at, created_at, next_at, skipped, last_skip_at";

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
        text: r.get(12)?,
        paused_at: r.get(13)?,
        created_at: r.get(14)?,
        next_at: r.get(15)?,
        skipped: r.get(16)?,
        last_skip_at: r.get(17)?,
    })
}

fn has_table(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'schedules'", [], |r| r.get::<_, i64>(0)).map(|n| n > 0)
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
pub fn summaries(db: &Connection, all: Option<&[SessionInfo]>, keep: i64) -> Result<Vec<Summary>> {
    list(db)?.into_iter().map(|s| summary(db, s, all, keep)).collect()
}

pub fn summary(db: &Connection, s: Schedule, all: Option<&[SessionInfo]>, keep: i64) -> Result<Summary> {
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
    Ok(Summary { when: w.words(), custom: w.is_custom(), next, last, open, runs, schedule: s })
}

//! The read side of Skipper's ledger, `ledger.db` in its folder. The `skiff
//! ledger` commands write it; the CLI and the app read it with this code.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use skiff_core::{
    session::{SessionInfo, SessionState},
    state,
};

/// Shown in place of `sent` or `blocked` when the pane restarted since the
/// send. Never stored: the pane generation tells it.
pub const INTERRUPTED: &str = "interrupted";
/// Shown in place of an open state when the pane no longer exists, and in
/// place of `done` on a task that skiff set done for that reason (it has
/// `gone_at`).
pub const GONE: &str = "gone";

/// The schema version, in `user_version`. 4 added `tasks.socket` and
/// `owner`, 5 added `tasks.reported_at`, 6 added `tasks.title`, 7 added
/// `tasks.closed_at`, 8 added `tasks.gone_at`, 9 added `tasks.owner` and the
/// `handoff` kind, 10 added `tasks.reopened_at`, 11 added `tasks.work_dir`
/// and `tasks.flushed_at`, 12 added `tasks.outcome`, `tasks.expects` and
/// `tasks.commit_ref`, 13 added the `deliveries` table, 14 added the `schedules`
/// table and `tasks.schedule`, 15 moved each schedule's prompt to its file; an older file from version 3 on takes each in place,
/// with its rows. The readers take a version 10 file: the version 11 and 12
/// columns read as empty there.
pub const VERSION: i64 = 15;

pub const COLUMNS: &str = "id, uid, kind, parent, pane_id, pane_gen, project, task, sent_at, state, result_path, ref, updated_at, type, socket, reported_at, title, closed_at, gone_at, owner";

/// The version 11 columns, or empty values on an older file.
fn columns_11(db: &Connection) -> rusqlite::Result<&'static str> {
    let n: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name IN ('work_dir', 'flushed_at')", [], |r| r.get(0))?;
    Ok(if n == 2 { "work_dir, flushed_at" } else { "NULL, NULL" })
}

/// The version 12 columns, or empty values on an older file.
fn columns_12(db: &Connection) -> rusqlite::Result<&'static str> {
    let n: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name IN ('outcome', 'expects', 'commit_ref')", [], |r| r.get(0))?;
    Ok(if n == 3 { "outcome, expects, commit_ref" } else { "NULL, NULL, NULL" })
}

/// The version 14 column, or an empty value on an older file.
fn columns_14(db: &Connection) -> rusqlite::Result<&'static str> {
    let n: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name = 'schedule'", [], |r| r.get(0))?;
    Ok(if n == 1 { "schedule" } else { "NULL" })
}

/// How a done task ended, beside its state. Only a done task has one; a
/// task that opens again loses it. `None` on a done task: it was set done
/// before outcomes, or added done.
pub const SUCCEEDED: &str = "succeeded";
pub const FAILED: &str = "failed";
/// The task stopped before it ended, such as when its pane restarted, and
/// its coordinator chose not to go on.
pub const INTERRUPTED_OUTCOME: &str = "interrupted";
/// Nobody saw how it ended: its pane was gone, or its expected evidence was
/// missing when it was set done.
pub const UNOBSERVED: &str = "unobserved";
pub const OUTCOMES: [&str; 4] = [SUCCEEDED, FAILED, INTERRUPTED_OUTCOME, UNOBSERVED];

/// The evidence a task names: its result file, or a commit.
pub const EXPECTS: [&str; 2] = ["result", "commit"];

/// UTC, to the second, set by SQLite.
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";

/// The owner of the grand Skipper's rows. A project Skipper's rows have its
/// project's name.
pub const GRAND: &str = "grand";

/// Where work stands. Never stored: the close stamp, the task states and the
/// panes tell it. An agent is on working work now; open work waits for the
/// person or for Skipper; finished work is closed.
pub const WORKING: &str = "working";
pub const OPEN: &str = "open";
pub const FINISHED: &str = "finished";

#[derive(Serialize, Clone)]
pub struct Row {
    pub id: i64,
    /// 8 hex characters, at least one a letter, so it never reads as an id.
    pub uid: String,
    /// `task`; `plan`: a row with no pane that groups tasks; or `handoff`: a
    /// grand Skipper's task sent to a project Skipper, whose own tasks go
    /// under it.
    pub kind: String,
    /// The plan's or the hand-off's uid.
    pub parent: Option<String>,
    /// The full session id. A plan has none.
    pub pane_id: Option<String>,
    /// The pane's generation at the send. See [`SessionInfo::generation`].
    pub pane_gen: Option<i64>,
    pub project: String,
    pub task: String,
    pub sent_at: String,
    pub state: String,
    pub result_path: Option<String>,
    /// Free text that names the work elsewhere. Skiff never reads it.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub updated_at: String,
    /// The task type from `[skiff_agent.tasks]`. Skiff stores it and never acts on it.
    #[serde(rename = "type")]
    pub task_type: Option<String>,
    /// The daemon socket of the pane. A pane id means nothing on another socket.
    pub socket: Option<String>,
    /// When the person was told the result. `None`: not yet. Open work has none.
    pub reported_at: Option<String>,
    /// A short title, 3 to 6 words, for the app's panel. `None`: the panel
    /// takes one from the task text.
    pub title: Option<String>,
    /// When the person, or Skiff by [`auto_close`], closed the work. Only a
    /// work head has it: a plan, or a task with no plan. A task in a plan is
    /// closed with its plan. `None`: the work is open.
    pub closed_at: Option<String>,
    /// When skiff set the task done because its pane was gone: no session
    /// with its id on its daemon. `None`: the task did not close that way.
    pub gone_at: Option<String>,
    /// The coordinator whose row it is: [`GRAND`], or the project of a
    /// project Skipper. Only it, or the person, changes the row.
    pub owner: String,
    /// The row's work folder, which `skiff ledger add` made: everything for
    /// the task goes in it, the result too. `None`: a row from before work
    /// folders, whose result path stands alone.
    pub work_dir: Option<String>,
    /// When Settings › Disk use flushed the work folder. The row stays; the
    /// panel shows "Result flushed" in place of a missing result. A flush is
    /// the person's word on the row: it needs no report (see [`Row::settled`]).
    /// A state other than done clears it.
    pub flushed_at: Option<String>,
    /// How the task ended: one of [`OUTCOMES`]. Only a done task has one.
    pub outcome: Option<String>,
    /// The evidence the task should leave: one of [`EXPECTS`]. `skiff ledger
    /// update --state done` checks it. `None`: nothing is checked.
    pub expects: Option<String>,
    /// The commit the task names as its evidence, with `--expect commit`.
    pub commit_ref: Option<String>,
    /// A schedule's run: the schedule's uid. `None` for any other row, the
    /// agent step under a run included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// [`WORKING`], [`OPEN`] or [`FINISHED`], for the whole work. Set by
    /// [`buckets`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<&'static str>,
    /// An agent is on this task now. Set by the caller from the sessions:
    /// see [`Row::agent_on_in`].
    #[serde(skip)]
    pub agent_on: bool,
    /// The socket is not this skiff's: no check of the pane was made.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub elsewhere: bool,
    /// The pane still exists. Absent when the daemon did not answer, and on a plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    /// The pane restarted since the send, so the task's process is gone.
    /// Absent when the daemon did not answer, and on a plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<bool>,
}

impl Row {
    pub fn is_plan(&self) -> bool {
        self.kind == "plan"
    }

    pub fn is_handoff(&self) -> bool {
        self.kind == "handoff"
    }

    /// The row holds no work open: it is done, and it was reported or flushed,
    /// or skiff set it done because its session closed. A blocked, sent or
    /// pending row is never settled: when its session closes it becomes done
    /// first (see `skiff ledger`'s sweep).
    pub fn settled(&self) -> bool {
        self.state == "done" && (self.reported_at.is_some() || self.flushed_at.is_some() || self.gone_at.is_some())
    }

    /// It ended, and not well: a person should hear of it.
    pub fn ended_badly(&self) -> bool {
        matches!(self.outcome.as_deref(), Some(FAILED | UNOBSERVED | INTERRUPTED_OUTCOME))
    }

    /// The coordinator that owns the row, as a person reads it.
    pub fn owner_name(&self) -> String {
        owner_name(&self.owner)
    }

    /// The socket of a task sent on another daemon than `here`'s. Its pane
    /// cannot be checked from here: it is neither live nor gone.
    pub fn socket_other_than(&self, here: &Path) -> Option<&str> {
        self.socket.as_deref().filter(|s| !self.is_plan() && !state::same_socket(Path::new(s), here))
    }

    /// The task ran in `s`'s pane, and that pane now runs another program
    /// than at the send: a restore brought it back as a new shell. A row
    /// with no generation (from before version 2) is never interrupted.
    pub fn interrupted_in(&self, s: Option<&SessionInfo>) -> bool {
        matches!(self.state.as_str(), "sent" | "blocked")
            && self.pane_gen.is_some()
            && s.and_then(gen).is_some_and(|g| Some(g) != self.pane_gen)
    }

    /// The uid of the row above it: its plan's or its hand-off's, else its own.
    pub fn work(&self) -> &str {
        self.parent.as_deref().unwrap_or(&self.uid)
    }

    /// A sent task whose pane `s` still runs its program, and works or ended
    /// its turn: the result is on its way. A pane that waits for approval,
    /// exited, restarted or is gone needs the person instead.
    pub fn agent_on_in(&self, s: Option<&SessionInfo>) -> bool {
        self.state == "sent"
            && !self.interrupted_in(s)
            && s.is_some_and(|s| s.agent_exit.is_none() && matches!(s.state, SessionState::Working | SessionState::Idle))
    }

    /// The state as `ls` shows it.
    pub fn shown_state(&self) -> &str {
        if self.interrupted == Some(true) {
            INTERRUPTED
        } else if (self.gone_at.is_some() && self.state == "done") || (self.live == Some(false) && self.state != "done") {
            GONE
        } else {
            &self.state
        }
    }
}

/// "Admiral", "the <project> Skipper", or "the <fleet> Skipper" for
/// the owner `fleet:<fleet>`.
pub fn owner_name(owner: &str) -> String {
    match (owner, crate::skipper::scope_fleet(owner)) {
        (GRAND, _) => main_name(),
        (_, Some(fleet)) => format!("the {fleet} {}", skipper_name()),
        _ => format!("the {owner} {}", skipper_name()),
    }
}

fn main_name() -> String {
    crate::skipper::name(&skiff_core::config::load().unwrap_or_default())
}

fn skipper_name() -> String {
    skiff_core::config::load().unwrap_or_default().skiff_agent.skipper_word()
}

/// Each row's work head: the row at the top of its parents. A hand-off in a
/// plan, and the tasks under it, belong to the plan's work. A parent not in
/// `rows` ends the climb.
pub fn heads(rows: &[Row]) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let up: HashMap<&str, &str> = rows.iter().filter_map(|r| r.parent.as_deref().map(|p| (r.uid.as_str(), p))).collect();
    let known: std::collections::HashSet<&str> = rows.iter().map(|r| r.uid.as_str()).collect();
    rows.iter()
        .map(|r| {
            let mut at = r.uid.as_str();
            // Two steps at most: plan, hand-off, task. The bound stops a loop.
            for _ in 0..4 {
                match up.get(at) {
                    Some(p) if known.contains(p) => at = p,
                    _ => break,
                }
            }
            (r.uid.clone(), at.to_string())
        })
        .collect()
}

/// `SKIFF_LEDGER`, else `ledger.db` in the Skiff agent's folder: the
/// `[skiff_agent] folder` override, else `~/skiff-agent`, or the folder in
/// the socket's private state. A relative `SKIFF_LEDGER` is taken from the
/// working folder, so the ledger always has a folder: results, stamps and
/// rows all follow the one full path.
pub fn path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("SKIFF_LEDGER") {
        return std::path::absolute(PathBuf::from(p)).context("SKIFF_LEDGER is not a usable path");
    }
    Ok(crate::skipper::configured(&skiff_core::config::load()?)?.join("ledger.db"))
}

/// The pane generation as the ledger stores it.
pub fn gen(s: &SessionInfo) -> Option<i64> {
    s.generation().and_then(|g| i64::try_from(g).ok())
}

/// Lines for a Skipper that skiff wrote outside a `skiff ledger` command, such
/// as a takeover: the watcher types each into its owner's Skipper, then
/// deletes it.
pub const NOTES: &str = "
CREATE TABLE IF NOT EXISTS notes (
  id    INTEGER PRIMARY KEY AUTOINCREMENT,
  owner TEXT NOT NULL,
  text  TEXT NOT NULL,
  at    TEXT NOT NULL
);
";

/// Opens the ledger to change it, for a writer outside `skiff ledger`: no
/// migration, no new file. Refuses a file older than [`VERSION`], as
/// [`open_read`] does.
pub fn open_write(file: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("open {}", file.display()))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let n: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name = 'reopened_at'", [], |r| r.get(0))?;
    if n == 0 {
        bail!("the ledger is older than version {VERSION}. Run any `skiff ledger` command to update it");
    }
    db.execute_batch(NOTES)?;
    Ok(db)
}

/// The notes for `owner`, oldest first. No ledger yet gives none.
pub fn notes(file: &Path, owner: &str) -> Result<Vec<(i64, String)>> {
    if !file.exists() {
        return Ok(Vec::new());
    }
    let db = open_write(file)?;
    let mut st = db.prepare("SELECT id, text FROM notes WHERE owner = ?1 ORDER BY id")?;
    let it = st.query_map([owner], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(it.collect::<rusqlite::Result<_>>()?)
}

/// Deletes the notes `ids`: their Skipper got them.
pub fn drop_notes(file: &Path, ids: &[i64]) -> Result<()> {
    let db = open_write(file)?;
    for id in ids {
        db.execute("DELETE FROM notes WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Opens the ledger read-only, for a reader that must not change it: no
/// migration, no new file. A file without `tasks.reopened_at` (older than
/// [`VERSION`]) is refused: any `skiff ledger` command brings it up to date.
/// The columns decide, not `user_version`: an older skiff stamps its own
/// lower version on a file that has every column.
pub fn open_read(file: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("open {}", file.display()))?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    let n: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name = 'reopened_at'", [], |r| r.get(0))?;
    if n == 0 {
        bail!("the ledger is older than version {VERSION}. Run any `skiff ledger` command to update it");
    }
    Ok(db)
}

pub fn rows(db: &Connection, where_: &str, arg: Option<&dyn rusqlite::ToSql>) -> Result<Vec<Row>> {
    match arg {
        Some(a) => rows_with(db, where_, &[a]),
        None => rows_with(db, where_, &[]),
    }
}

/// [`rows`] with any number of parameters.
pub fn rows_with(db: &Connection, where_: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Row>> {
    let sql = format!("SELECT {COLUMNS}, {}, {}, {} FROM tasks {where_} ORDER BY id", columns_11(db)?, columns_12(db)?, columns_14(db)?);
    let mut st = db.prepare(&sql)?;
    let map = |r: &rusqlite::Row| {
        Ok(Row {
            id: r.get(0)?,
            uid: r.get(1)?,
            kind: r.get(2)?,
            parent: r.get(3)?,
            pane_id: r.get(4)?,
            pane_gen: r.get(5)?,
            project: r.get(6)?,
            task: r.get(7)?,
            sent_at: r.get(8)?,
            state: r.get(9)?,
            result_path: r.get(10)?,
            reference: r.get(11)?,
            updated_at: r.get(12)?,
            task_type: r.get(13)?,
            socket: r.get(14)?,
            reported_at: r.get(15)?,
            title: r.get(16)?,
            closed_at: r.get(17)?,
            gone_at: r.get(18)?,
            owner: r.get(19)?,
            work_dir: r.get(20)?,
            flushed_at: r.get(21)?,
            outcome: r.get(22)?,
            expects: r.get(23)?,
            commit_ref: r.get(24)?,
            schedule: r.get(25)?,
            bucket: None,
            agent_on: false,
            elsewhere: false,
            live: None,
            interrupted: None,
        })
    };
    let out = st.query_map(args, map)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(out)
}

/// The one line for the grand Skipper's watcher when `rows` of the work head
/// `head` close with no report.
pub fn untold_note(head: &Row, rows: &[&Row]) -> String {
    let named: Vec<String> = rows
        .iter()
        .map(|r| {
            let kind: &str = if r.is_handoff() { "hand-off" } else { &r.kind };
            let title = r.title.as_deref().map(|t| format!(" ({t})")).unwrap_or_default();
            format!("{kind} {}{title} {}", r.uid, r.outcome.as_deref().unwrap_or_default())
        })
        .collect();
    format!("Closed {} {} with no report: {}. Tell the person. Read a row with skiff ledger show UID.", head.kind, head.uid, named.join(", "))
}

/// Leaves `text` for the grand Skipper's watcher. Run [`NOTES`] first.
pub fn add_note(db: &Connection, text: &str) -> Result<()> {
    db.execute(&format!("INSERT INTO notes (owner, text, at) VALUES (?1, ?2, {NOW})"), params![GRAND, text])?;
    Ok(())
}

/// The hand-offs with a task under them, a plan's in them included. Such a
/// hand-off holds its work open until its Skipper reports it, or until skiff
/// sets it done because the Skipper's session closed. Between its tasks the
/// Skipper may still add more. Then it follows its tasks.
pub fn handoffs_with_tasks(rows: &[Row]) -> std::collections::HashSet<&str> {
    let by: std::collections::HashMap<&str, &Row> = rows.iter().map(|r| (r.uid.as_str(), r)).collect();
    let above = |r: &Row| r.parent.as_deref().and_then(|p| by.get(p).copied());
    let mut out = std::collections::HashSet::new();
    for t in rows.iter().filter(|r| r.kind == "task") {
        let Some(p) = above(t) else { continue };
        let h = if p.is_plan() { above(p) } else { Some(p) };
        if let Some(h) = h.filter(|h| h.is_handoff()) {
            out.insert(h.uid.as_str());
        }
    }
    out
}

/// The row keeps its work open. `with_tasks`: from [`handoffs_with_tasks`].
/// A hand-off with tasks holds it while it is not done; any other row, while
/// it is not settled.
pub fn holds_open(r: &Row, with_tasks: &std::collections::HashSet<&str>) -> bool {
    if r.is_plan() {
        false
    } else if with_tasks.contains(r.uid.as_str()) {
        r.state != "done"
    } else {
        !r.settled()
    }
}

/// Closes each open work item that no row holds open (see [`holds_open`]): a
/// plan's tasks, a hand-off and its tasks, or a lone task. A plan with no task
/// stays open, as does work with `reopened_at`. As `ledger close` does, it
/// stamps `reported_at` on the rows not yet reported. A hand-off that is not
/// done becomes done, `unobserved`: closed work has no open row. A done hand-off
/// nobody reported keeps no stamp: the watcher tells the grand Skipper to read
/// its result. A row that ended badly among those it stamps leaves one note
/// for that watcher, in the same transaction. The stamp keeps a later close
/// from naming it again. Returns the work heads it closed.
pub fn auto_close(db: &mut Connection) -> Result<Vec<Row>> {
    use std::collections::{BTreeMap, HashSet};
    db.execute_batch(NOTES)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let all = rows(&tx, "", None)?;
    let held: HashSet<String> = {
        let mut st = tx.prepare("SELECT uid FROM tasks WHERE reopened_at IS NOT NULL")?;
        let out = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
        out
    };
    let head = heads(&all);
    let with_tasks = handoffs_with_tasks(&all);
    // Each work head with its tasks and hand-offs. A plan row's own state does not count.
    let mut work: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in all.iter().filter(|r| !r.is_plan()) {
        work.entry(head[&r.uid].as_str()).or_default().push(r);
    }
    let mut out = Vec::new();
    for (h, members) in work {
        let Some(r) = all.iter().find(|r| r.uid == h) else { continue };
        if r.closed_at.is_some() || held.contains(h) || members.iter().any(|m| holds_open(m, &with_tasks)) {
            continue;
        }
        let mut untold = Vec::new();
        for m in &members {
            let mut m = (*m).clone();
            if m.state != "done" {
                tx.execute(&format!("UPDATE tasks SET state = 'done', outcome = ?2, updated_at = {NOW} WHERE id = ?1"), params![m.id, UNOBSERVED])?;
                m.state = "done".into();
                m.outcome = Some(UNOBSERVED.into());
            } else if with_tasks.contains(m.uid.as_str()) && !m.settled() {
                continue;
            }
            if m.reported_at.is_none() {
                tx.execute(&format!("UPDATE tasks SET reported_at = {NOW} WHERE id = ?1"), [m.id])?;
                if m.ended_badly() {
                    untold.push(m);
                }
            }
        }
        if !untold.is_empty() {
            add_note(&tx, &untold_note(r, &untold.iter().collect::<Vec<_>>()))?;
        }
        if tx.execute(&format!("UPDATE tasks SET closed_at = {NOW} WHERE id = ?1 AND closed_at IS NULL"), [r.id])? == 1 {
            out.push(r.clone());
        }
    }
    tx.commit()?;
    Ok(out)
}

/// Finished in the app's Ledger panel shows this many closed items, newest
/// first. Keep in step with `KEEP` in `apps/desktop/src/workspace/ledger.ts`.
pub const PANEL_KEEP: i64 = 100;

/// The rows the panel shows: all work not closed, and the last
/// [`PANEL_KEEP`] closed work heads with their tasks, a hand-off's tasks
/// included. Older closed work stays in the file. The head of a hand-off's
/// task is two steps up. A schedule's runs show in the Schedules tab, not
/// here: [`panel`] leaves them out, and the agent steps of closed runs.
const PANEL: &str = "WHERE coalesce((SELECT h.parent FROM tasks h WHERE h.uid = tasks.parent), parent, uid) NOT IN (SELECT uid FROM tasks WHERE closed_at IS NOT NULL) \
     OR coalesce((SELECT h.parent FROM tasks h WHERE h.uid = tasks.parent), parent, uid) IN (SELECT uid FROM tasks WHERE closed_at IS NOT NULL ORDER BY closed_at DESC, id DESC LIMIT ?1)";

/// The panel's rows from `file`, oldest first, plans rolled up. No file
/// gives none; nothing is made or migrated.
pub fn panel(file: &Path) -> Result<Vec<Row>> {
    if !file.exists() {
        return Ok(Vec::new());
    }
    let db = open_read(file)?;
    let filter = if columns_14(&db)? == "schedule" {
        PANEL
            .replacen("WHERE ", "WHERE schedule IS NULL AND (", 1)
            .replace("WHERE closed_at IS NOT NULL ORDER BY", "WHERE closed_at IS NOT NULL AND schedule IS NULL ORDER BY")
            + ")"
    } else {
        PANEL.to_string()
    };
    let mut rows = rows(&db, &filter, Some(&PANEL_KEEP))?;
    roll_up(&mut rows);
    Ok(rows)
}

/// One row by its uid, read-only. `None` when no row has it.
pub fn by_uid(file: &Path, uid: &str) -> Result<Option<Row>> {
    if !file.exists() {
        return Ok(None);
    }
    let db = open_read(file)?;
    Ok(rows(&db, "WHERE uid = ?1", Some(&uid))?.into_iter().next())
}

/// A value that changes when the ledger does: the size, change time and
/// inode of `file` itself and of its SQLite WAL, `<file>-wal`, whatever the
/// file's name, and the file's name. Read from metadata only; the database
/// is not opened.
pub fn stamp(file: &Path) -> String {
    use std::hash::{Hash, Hasher};
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut h);
    let mut wal = file.as_os_str().to_owned();
    wal.push("-wal");
    for f in [file.to_path_buf(), PathBuf::from(wal)] {
        let m = std::fs::metadata(f).ok();
        let modified = m.as_ref().and_then(|m| m.modified().ok());
        (m.as_ref().map(|m| m.len()), modified).hash(&mut h);
        #[cfg(unix)]
        m.map(|m| (m.ino(), m.ctime(), m.ctime_nsec())).hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// A plan's state from its tasks in `rows`: `done` once every task is done,
/// `sent` while one is open. `None` for a plan with no task: its own state
/// stands.
pub fn plan_state(uid: &str, rows: &[Row]) -> Option<&'static str> {
    let mut tasks = rows.iter().filter(|r| r.parent.as_deref() == Some(uid)).peekable();
    tasks.peek()?;
    Some(if tasks.all(|t| t.state == "done") { "done" } else { "sent" })
}

/// Sets each plan in `rows` to the state its tasks give it. Never stored.
pub fn roll_up(rows: &mut [Row]) {
    let states: Vec<Option<&str>> = rows.iter().map(|r| if r.is_plan() { plan_state(&r.uid, rows) } else { None }).collect();
    for (r, st) in rows.iter_mut().zip(states) {
        if let Some(st) = st {
            r.state = st.to_string();
        }
    }
}

/// Sets each row's bucket from its work: [`FINISHED`] once the work head is
/// closed, else [`WORKING`] while an agent is on one of its tasks, else
/// [`OPEN`]. Set `agent_on` first.
pub fn buckets(rows: &mut [Row]) {
    use std::collections::HashSet;
    let head = heads(rows);
    let closed: HashSet<String> = rows.iter().filter(|r| r.closed_at.is_some()).map(|r| r.uid.clone()).collect();
    let working: HashSet<String> = rows.iter().filter(|r| r.agent_on).map(|r| head[&r.uid].clone()).collect();
    for r in rows.iter_mut() {
        let h = &head[&r.uid];
        r.bucket = Some(if closed.contains(h) {
            FINISHED
        } else if working.contains(h) {
            WORKING
        } else {
            OPEN
        });
    }
}

/// The session a stored pane id names in `all`: the same id, else the one
/// session it is a prefix of.
pub fn session_of<'a>(all: &'a [SessionInfo], p: &str) -> Option<&'a SessionInfo> {
    all.iter().find(|s| s.id == p).or_else(|| {
        let mut found = all.iter().filter(|s| s.id.starts_with(p));
        found.next().filter(|_| found.next().is_none())
    })
}

/// The session the stored pane id `p` named has closed: `all`, a full list
/// from its daemon, has none it could name, or each one it could name has
/// exited. skiffd sets `SessionState::Done` only when it reaped the process,
/// and it never leaves it. An idle pane, or a pane restored under the old id,
/// is a live session.
pub fn session_closed(all: &[SessionInfo], p: &str) -> bool {
    all.iter().filter(|s| s.id.starts_with(p)).all(|s| s.state == SessionState::Done)
}

/// Fills in `live`, `interrupted` and `agent_on` from the sessions `all`,
/// when the daemon answered, then each row's bucket. A task of another
/// socket is marked `elsewhere` and not checked. A sent task that cannot be
/// checked counts as worked on. `skiff ledger ls` and the app's panel both
/// file rows by it.
pub fn mark_live(socket: &Path, all: Option<&[SessionInfo]>, rows: &mut [Row]) {
    for r in rows.iter_mut() {
        r.elsewhere = r.socket_other_than(socket).is_some();
        r.agent_on = !r.is_plan() && r.state == "sent";
    }
    if let Some(all) = all {
        for r in rows.iter_mut().filter(|r| !r.is_plan() && !r.elsewhere) {
            let s = r.pane_id.as_deref().and_then(|p| session_of(all, p));
            r.live = Some(s.is_some());
            r.interrupted = Some(r.interrupted_in(s));
            r.agent_on = r.agent_on_in(s);
        }
    }
    buckets(rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_goes_by_the_columns_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("skiff-ledger-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ledger.db");
        // Version 9: no reopened_at. Refused.
        let db = Connection::open(&file).unwrap();
        db.execute_batch("CREATE TABLE tasks (id INTEGER PRIMARY KEY, uid TEXT, socket TEXT, reported_at TEXT, title TEXT, closed_at TEXT, gone_at TEXT); PRAGMA user_version = 8;").unwrap();
        assert!(open_read(&file).unwrap_err().to_string().contains("skiff ledger"));
        drop(db);
        std::fs::remove_file(&file).unwrap();
        // Every column, stamped down to 3 by an older skiff. Read.
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&format!(
            "CREATE TABLE tasks (id INTEGER PRIMARY KEY, uid TEXT, kind TEXT, parent TEXT, pane_id TEXT, pane_gen INTEGER, project TEXT,
               task TEXT, sent_at TEXT, state TEXT, result_path TEXT, ref TEXT, updated_at TEXT, type TEXT, socket TEXT, reported_at TEXT, title TEXT, closed_at TEXT, gone_at TEXT, owner TEXT NOT NULL DEFAULT 'grand', reopened_at TEXT);
             INSERT INTO tasks (uid, kind, project, task, sent_at, state, updated_at) VALUES ('a1b2c3d4', 'plan', '', 'p', 'x', 'sent', 'x');
             INSERT INTO tasks (uid, kind, parent, project, task, sent_at, state, updated_at) VALUES ('e5f6a7b8', 'task', 'a1b2c3d4', '', 't', 'x', 'done', 'x');
             PRAGMA user_version = {};",
            3
        ))
        .unwrap();
        let r = open_read(&file).unwrap();
        let mut list = rows(&r, "", None).unwrap();
        roll_up(&mut list);
        assert_eq!(list.iter().map(|r| r.state.as_str()).collect::<Vec<_>>(), ["done", "done"]);
        assert!(r.execute("DELETE FROM tasks", []).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

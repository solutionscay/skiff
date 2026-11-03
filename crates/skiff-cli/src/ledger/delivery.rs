//! Delivery records (#246): each attempt to type a row's text into a
//! session, written before the text goes in and settled after it. A record
//! says what went in and what came of it; it never claims exactly once.

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::NOW;

/// One attempt, as `ledger show` prints it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Attempt {
    pub id: i64,
    /// `send`: the row's text to its pane. `report`: a child's report typed
    /// into its parent.
    pub kind: String,
    pub session: String,
    /// The session's generation at the attempt. See `SessionInfo::generation`.
    pub session_gen: Option<i64>,
    pub bytes: i64,
    pub sha256: String,
    pub started_at: String,
    /// What came of it: see [`Evidence`]. `None`: nothing settled it.
    pub evidence: Option<String>,
    /// `send`'s exit code, when it gave one.
    pub code: Option<i64>,
    pub failure: Option<String>,
    pub ended_at: Option<String>,
}

impl Attempt {
    /// The evidence as a person reads it: an attempt nothing settled is uncertain.
    pub fn shown_evidence(&self) -> &str {
        self.evidence.as_deref().unwrap_or(UNCERTAIN)
    }
}

/// The evidence when nothing can settle the attempt: the process ended
/// between the two writes, or the send failed after it may have typed.
pub const UNCERTAIN: &str = "uncertain";

/// What came of an attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Evidence {
    /// The agent started a turn after the text and Enter.
    TurnStarted,
    /// The text went in. No turn was asked for, or the program cannot show one.
    Accepted,
    /// The text and Enter went in, but no turn started in time: exit 8.
    NoTurn,
    /// Nothing was typed: a refusal, with `send`'s exit code.
    Refused(i32),
}

impl Evidence {
    /// From `send`'s exit code. `turn`: a known agent got Enter, so exit 0
    /// means a turn started.
    pub fn of(code: i32, turn: bool) -> Self {
        match code {
            0 if turn => Evidence::TurnStarted,
            0 => Evidence::Accepted,
            8 => Evidence::NoTurn,
            c => Evidence::Refused(c),
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Evidence::TurnStarted => "turn started",
            Evidence::Accepted => "text accepted",
            Evidence::NoTurn => "no turn",
            Evidence::Refused(_) => "refused",
        }
    }

    /// Why the attempt did not end in a turn, by `send`'s exit code.
    pub fn failure(self) -> Option<&'static str> {
        Some(match self {
            Evidence::TurnStarted | Evidence::Accepted => return None,
            Evidence::NoTurn => "text sent, but no turn started: it can be in the input box",
            Evidence::Refused(3) => "the session waits for the person",
            Evidence::Refused(4) => "the session is working",
            Evidence::Refused(5) => "a shell or an unknown program is in front",
            Evidence::Refused(6) => "the session's program exited",
            Evidence::Refused(7) => "the session is the sender's own",
            Evidence::Refused(9) => "a draft or the busy line, or input after the check",
            Evidence::Refused(_) => "send refused it",
        })
    }

    /// `send`'s exit code for it.
    pub fn code(self) -> i32 {
        match self {
            Evidence::TurnStarted | Evidence::Accepted => 0,
            Evidence::NoTurn => 8,
            Evidence::Refused(c) => c,
        }
    }
}

/// The payload's size and SHA-256, in hex: the text exactly as it was
/// handed to `send`, before Enter keys and the trailing newlines go.
pub fn identity(text: &str) -> (i64, String) {
    let hash = Sha256::digest(text.as_bytes());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    (i64::try_from(text.len()).unwrap_or(i64::MAX), hex)
}

/// Records an attempt to deliver `text` for row `task` to `session`, before
/// it goes in. Returns the attempt's id.
pub fn start(db: &Connection, task: &str, kind: &str, session: &str, session_gen: Option<i64>, text: &str) -> Result<i64> {
    let (bytes, sha) = identity(text);
    db.execute(
        &format!("INSERT INTO deliveries (task, kind, session, session_gen, bytes, sha256, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, {NOW})"),
        params![task, kind, session, session_gen, bytes, sha],
    )?;
    Ok(db.last_insert_rowid())
}

/// Settles attempt `id` with what came of it.
pub fn end(db: &Connection, id: i64, e: Evidence) -> Result<()> {
    db.execute(
        &format!("UPDATE deliveries SET evidence = ?2, code = ?3, failure = ?4, ended_at = {NOW} WHERE id = ?1"),
        params![id, e.word(), e.code(), e.failure()],
    )?;
    Ok(())
}

/// Settles attempt `id` as uncertain: the send failed, and it may have typed.
pub fn end_uncertain(db: &Connection, id: i64, failure: &str) -> Result<()> {
    db.execute(
        &format!("UPDATE deliveries SET evidence = '{UNCERTAIN}', failure = ?2, ended_at = {NOW} WHERE id = ?1"),
        params![id, failure],
    )?;
    Ok(())
}

/// The attempts for row `task`, oldest first.
pub fn of(db: &Connection, task: &str) -> Result<Vec<Attempt>> {
    let mut st = db.prepare("SELECT id, kind, session, session_gen, bytes, sha256, started_at, evidence, code, failure, ended_at FROM deliveries WHERE task = ?1 ORDER BY id")?;
    let it = st.query_map([task], |r| {
        Ok(Attempt {
            id: r.get(0)?,
            kind: r.get(1)?,
            session: r.get(2)?,
            session_gen: r.get(3)?,
            bytes: r.get(4)?,
            sha256: r.get(5)?,
            started_at: r.get(6)?,
            evidence: r.get(7)?,
            code: r.get(8)?,
            failure: r.get(9)?,
            ended_at: r.get(10)?,
        })
    })?;
    Ok(it.collect::<rusqlite::Result<_>>()?)
}

/// Records a child's report typed into its parent for each row in `tasks`:
/// the watcher's line that names them. The ledger at `file` must exist.
/// Returns the attempt ids.
pub fn start_reports(file: &Path, tasks: &[String], session: &str, session_gen: Option<i64>, text: &str) -> Result<Vec<i64>> {
    let db = super::open_at(file)?;
    tasks.iter().map(|t| start(&db, t, "report", session, session_gen, text)).collect()
}

/// Settles the report attempts `ids`.
pub fn end_reports(file: &Path, ids: &[i64], e: Evidence) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let db = super::open_at(file)?;
    for id in ids {
        end(&db, *id, e)?;
    }
    Ok(())
}

/// Settles the report attempts `ids` as uncertain: the watcher's submit failed.
pub fn end_reports_uncertain(file: &Path, ids: &[i64], failure: &str) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let db = super::open_at(file)?;
    for id in ids {
        end_uncertain(&db, *id, failure)?;
    }
    Ok(())
}

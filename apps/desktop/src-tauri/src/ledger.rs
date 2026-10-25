//! Skipper's ledger for the panel beside its pane. The app reads `ledger.db`
//! read-only with the CLI's read code. It never writes: only Skipper changes
//! rows, through the CLI.

use std::{path::Path, time::Duration};

use serde::Serialize;
use skiff_client::ledger::{self, Row};
use skiff_core::session::SessionInfo;
use tauri::{AppHandle, Emitter, State};

use crate::connection::{err, open_sessions, App};

/// The page reads the ledger again on this event.
const LEDGER_CHANGED: &str = "skiff:ledger-changed";

/// How often the watcher looks at the files.
const TICK: Duration = Duration::from_secs(1);

/// A row as the panel takes it.
#[derive(Serialize)]
pub struct Entry {
    #[serde(flatten)]
    row: Row,
    /// The result file is on disk.
    result_exists: bool,
}

/// Every row, reported ones included, oldest first. Plans carry the state
/// their tasks give them. Each row carries its work's bucket, as `skiff
/// ledger ls` gives it, from the sessions on the open connection. No
/// `ledger.db` yet gives none.
#[tauri::command]
pub async fn ledger_rows(app: State<'_, App>) -> Result<Vec<Entry>, String> {
    let all = open_sessions(&app).await;
    tauri::async_runtime::spawn_blocking(move || read(all.as_deref())).await.map_err(|e| e.to_string())?.map_err(err)
}

/// Finished shows this many closed items, newest first. Keep in step with
/// `KEEP` in `workspace/ledger.ts`.
const KEEP: i64 = 100;

/// The rows the panel shows: all work not closed, and the last [`KEEP`]
/// closed work heads with their tasks, a hand-off's tasks included. Older
/// closed work stays in the file. The head of a hand-off's task is two
/// steps up.
const PANEL: &str = "WHERE coalesce((SELECT h.parent FROM tasks h WHERE h.uid = tasks.parent), parent, uid) NOT IN (SELECT uid FROM tasks WHERE closed_at IS NOT NULL) \
     OR coalesce((SELECT h.parent FROM tasks h WHERE h.uid = tasks.parent), parent, uid) IN (SELECT uid FROM tasks WHERE closed_at IS NOT NULL ORDER BY closed_at DESC, id DESC LIMIT ?1)";

fn read(all: Option<&[SessionInfo]>) -> anyhow::Result<Vec<Entry>> {
    let file = ledger::path()?;
    if !file.exists() {
        return Ok(Vec::new());
    }
    let db = ledger::open_read(&file)?;
    let mut rows = ledger::rows(&db, PANEL, Some(&KEEP))?;
    ledger::roll_up(&mut rows);
    ledger::mark_live(&skiff_core::state::socket(), all, &mut rows);
    Ok(rows
        .into_iter()
        .map(|row| {
            let result_exists = row.result_path.as_deref().is_some_and(|p| Path::new(p).is_file());
            Entry { row, result_exists }
        })
        .collect())
}

/// What the watcher compares: the size and mtime of the file and its WAL.
fn stamp(file: &Path) -> Vec<Option<(u64, std::time::SystemTime)>> {
    ["db", "db-wal"]
        .iter()
        .map(|ext| std::fs::metadata(file.with_extension(ext)).ok().and_then(|m| Some((m.len(), m.modified().ok()?))))
        .collect()
}

/// Runs at app start on its own thread. Emits [`LEDGER_CHANGED`] when
/// `ledger.db` or its WAL changes, or when the ledger moves to another folder.
pub fn watch(app: AppHandle) {
    let mut last = None;
    loop {
        if let Ok(file) = ledger::path() {
            let now = Some((stamp(&file), file));
            if last.is_some() && now != last {
                let _ = app.emit(LEDGER_CHANGED, ());
            }
            last = now;
        }
        std::thread::sleep(TICK);
    }
}

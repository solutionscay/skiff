//! `skiff ledger`: the Skiff agent's task record, a SQLite file in its folder.
//! The agent reads and writes it through these commands only, so a bad edit,
//! a compaction or a second writer cannot break it.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::LedgerCmd;
use skiff_client::ledger::auto_close;
pub use skiff_client::ledger::{path, Row, GRAND, SUCCEEDED};

mod brief;
mod commands;
mod delivery;
mod evidence;
mod paths;
mod read;
mod schema;
mod write;

pub use delivery::{end_reports, end_reports_uncertain, start_reports, Evidence};
pub use paths::results_dir;
use paths::{label, make_work_folder, new_uid, result_in, work_folder};
pub use read::{
    close_gone, gone_unreported, handed_back, handoffs_ready, open_tasks, plan_tasks, unreported, Ready,
};
#[cfg(test)]
use read::{close_gone_in, handoffs_ready_in, open_tasks_at, sweep, unreported_at};
use schema::NOW;
#[cfg(test)]
use schema::{OWNER, TASKS, UIDS};
#[cfg(test)]
use skiff_client::ledger::{OPEN, VERSION, WORKING};
pub use write::claim;
#[cfg(test)]
use write::*;
#[cfg(test)]
use read::{container, keep, one, rank, stale, tree, View};
#[cfg(test)]
use rusqlite::params;
#[cfg(test)]
use skiff_client::ledger::{heads, mark_live, roll_up, rows, GONE};
#[cfg(test)]
use skiff_core::session::SessionInfo;
#[cfg(test)]
use skiff_core::state;
#[cfg(test)]
use std::path::PathBuf;

fn open() -> Result<Connection> {
    schema::open()
}

fn open_at(file: &Path) -> Result<Connection> {
    schema::open_at(file)
}

/// Runs one `skiff ledger` command. After a command that writes, work that no
/// row holds open closes by itself (see [`auto_close`]).
/// The exit code: `send`'s for `send` and `add --send`, else 0.
pub async fn run(socket: &Path, cmd: LedgerCmd, person: bool) -> Result<i32> {
    let writes = !matches!(cmd, LedgerCmd::Ls { .. } | LedgerCmd::Show { .. } | LedgerCmd::Dir { .. } | LedgerCmd::Brief { .. } | LedgerCmd::Stale { .. });
    let code = commands::run_one(socket, cmd, person).await?;
    if writes {
        for r in auto_close(&mut open()?)? {
            eprintln!("skiff: closed {} {}: every task in it is closed", r.kind, r.uid);
        }
    }
    Ok(code)
}

#[cfg(test)]
mod tests;

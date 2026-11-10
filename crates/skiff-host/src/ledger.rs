//! `ledger@1`: the panel's rows and the results they name, and where a result
//! may be read on This Mac and over SSH.
use std::{path::Path, sync::atomic::Ordering};

use skiff_core::{files, git, session::SessionInfo};
use skiff_host_api::{Error, HostLedgerRow};

use super::{failed, unread};

/// Set when the app started this service for This Mac. Then a ledger result
/// may also sit anywhere under the person's home folder. A service reached
/// over SSH keeps the ledger folder only.
static THIS_MAC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Says whether this service is This Mac's own. Off by default.
pub fn set_this_mac(on: bool) {
    THIS_MAC.store(on, Ordering::Relaxed);
}

/// The Ledger panel's rows from this Host's `ledger.db`, read-only. A pane
/// is `elsewhere` when its socket is not this Host's skiffd; a result
/// exists only when [`ledger_result`] would serve it. Each row carries its
/// work's bucket from the sessions `all`, as `skiff ledger ls` gives it.
pub(super) fn ledger_rows(all: Option<&[SessionInfo]>) -> Result<Vec<HostLedgerRow>, Error> {
    let file = skiff_client::ledger::path().map_err(failed)?;
    let mut rows = skiff_client::ledger::panel(&file).map_err(ledger_unread)?;
    skiff_client::ledger::mark_live(&skiff_core::state::socket(), all, &mut rows);
    Ok(rows.into_iter().map(|r| host_row(&file, r)).collect())
}

/// A ledger row as the app takes it. `file`: the ledger, for the result check.
pub(super) fn host_row(file: &Path, r: skiff_client::ledger::Row) -> HostLedgerRow {
    let elsewhere = r.elsewhere;
    let bucket = r.bucket.unwrap_or(skiff_client::ledger::OPEN).to_string();
    let result_exists = r.result_path.as_deref().is_some_and(|p| result_file(file, p).is_ok());
    HostLedgerRow {
        id: r.id,
        uid: r.uid,
        kind: r.kind,
        owner: r.owner,
        parent: r.parent,
        pane_id: r.pane_id,
        pane_gen: r.pane_gen,
        project: r.project,
        task: r.task,
        sent_at: r.sent_at,
        title: r.title,
        state: r.state,
        updated_at: r.updated_at,
        task_type: r.task_type,
        reference: r.reference,
        reported_at: r.reported_at,
        closed_at: r.closed_at,
        gone_at: r.gone_at,
        bucket,
        elsewhere,
        result_exists,
        flushed_at: r.flushed_at,
        outcome: r.outcome,
        schedule: r.schedule,
    }
}

/// A ledger older than the readers take is `invalid`: any `skiff ledger`
/// command on the Host updates it. Anything else failed while it ran.
pub(super) fn ledger_unread(e: anyhow::Error) -> Error {
    let message = format!("{e:#}");
    if message.contains("skiff ledger") {
        return Error::Invalid { message };
    }
    failed(e)
}

/// The result of the row `uid`, read on this Host as a preview. Only the
/// path the row records is read, never one the client names, and only inside
/// the ledger's own folder: see [`result_file`].
pub(super) fn ledger_result(uid: &str) -> Result<files::FileRead, Error> {
    let file = skiff_client::ledger::path().map_err(failed)?;
    let none = || Error::NotFound { message: format!("row {uid} has no result this Host can show") };
    let row = skiff_client::ledger::by_uid(&file, uid).map_err(ledger_unread)?.ok_or_else(none)?;
    let path = row.result_path.ok_or_else(none)?;
    let (root, folder, rel) = result_file(&file, &path)?;
    files::read_in(&root, &folder, &rel, files::Purpose::Preview).map_err(unread)
}

/// Where a result may be read: below the folder that holds `ledger`, where
/// `skiff ledger` puts results, as [`files::hold_beneath`] allows. On This
/// Mac it may also be anywhere under the person's home folder: a Skipper
/// records a repo path when its pane cannot write outside its repo. Answers
/// the held folder, its path and the result relative to it. A ledger with
/// no folder of its own serves no result: never the root in its place.
fn result_file(ledger: &Path, result: &str) -> Result<(git::Root, std::path::PathBuf, String), Error> {
    let folder = ledger
        .parent()
        .filter(|f| f.is_absolute() && f.parent().is_some())
        .ok_or_else(|| Error::NotFound { message: format!("{} has no folder to hold results", ledger.display()) })?
        .to_path_buf();
    let beneath = files::hold_beneath(&folder, Path::new(result));
    let (root, folder, rel) = match beneath {
        Ok((root, rel)) => (root, folder, rel),
        Err(e) => {
            let home = dirs::home_dir().filter(|h| THIS_MAC.load(Ordering::Relaxed) && h.is_absolute() && h.parent().is_some());
            let Some(home) = home else { return Err(unread(e)) };
            let (root, rel) = files::hold_beneath(&home, Path::new(result)).map_err(unread)?;
            (root, home, rel)
        }
    };
    Ok((root, folder, rel))
}

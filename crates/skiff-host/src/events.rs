//! The service's invalidation feed: a poll of the config, the ledger and the
//! worktrees, sent to every connection as `DataChanged`.
use std::collections::{BTreeMap, BTreeSet};

use skiff_core::{config, project};
use tokio::sync::broadcast;

/// The service owns one bounded invalidation feed. It deliberately carries
/// no deltas: file-system changes can coalesce, and clients recover every
/// gap by reading the authoritative Host snapshots again.
pub(super) struct DataEvents {
    tx: broadcast::Sender<skiff_host_api::Event>,
}
impl DataEvents {
    pub(super) fn start() -> Self {
        let (tx, _) = broadcast::channel(256);
        let watch = tx.clone();
        tokio::spawn(async move {
            let mut previous = tokio::task::spawn_blocking(data_snapshot).await.unwrap_or_default();
            let mut sequence = 0u64;
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
            tick.tick().await;
            loop {
                tick.tick().await;
                let next = tokio::task::spawn_blocking(data_snapshot).await.unwrap_or_default();
                let projects = next.projects != previous.projects;
                let ledger = next.ledger != previous.ledger;
                let worktrees = next.worktrees.iter().filter_map(|(path, stamp)| {
                    (previous.worktrees.get(path) != Some(stamp)).then(|| path.clone())
                }).chain(previous.worktrees.keys().filter(|path| !next.worktrees.contains_key(*path)).cloned()).collect::<Vec<_>>();
                if projects || ledger || !worktrees.is_empty() {
                    sequence = sequence.wrapping_add(1);
                    let _ = watch.send(skiff_host_api::Event::DataChanged { sequence, projects, worktrees, tasks: ledger });
                }
                previous = next;
            }
        });
        Self { tx }
    }
    pub(super) fn subscribe(&self) -> broadcast::Receiver<skiff_host_api::Event> { self.tx.subscribe() }
}

#[derive(Default)]
struct DataSnapshot { projects: String, ledger: String, worktrees: BTreeMap<String, u64> }

/// A cheap-enough invalidation stamp, not a content hash. A write that keeps
/// mtimes is outside normal filesystem semantics; a reconnect snapshot still
/// makes that harmless. `.git` is excluded from the tree walk, except for
/// its index so a staged Git-only edit also invalidates its worktree.
fn data_snapshot() -> DataSnapshot {
    let projects = config::file_stamp(&config::config_path());
    let ledger = skiff_client::ledger::path().ok().map(|p| skiff_client::ledger::stamp(&p)).unwrap_or_default();
    let mut worktrees = BTreeMap::new();
    if let Ok(rows) = project::list() {
        for project in rows.into_iter().filter(|p| !p.closed) {
            let mut paths = BTreeSet::new();
            paths.insert(project.path);
            for wt in project.worktrees { paths.insert(wt.path); }
            for path in paths {
                let mut stamp = 0xcbf29ce484222325u64;
                for entry in walkdir::WalkDir::new(&path).into_iter().filter_entry(|e| e.file_name() != ".git").flatten() {
                    if let Ok(meta) = entry.metadata() {
                        for n in [meta.len(), meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0)] {
                            stamp ^= n; stamp = stamp.wrapping_mul(0x100000001b3);
                        }
                        for b in entry.path().as_os_str().as_encoded_bytes() { stamp ^= u64::from(*b); stamp = stamp.wrapping_mul(0x100000001b3); }
                    }
                }
                if let Ok(meta) = std::fs::metadata(path.join(".git/index")) {
                    stamp ^= meta.len(); stamp = stamp.wrapping_mul(0x100000001b3);
                    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0);
                    stamp ^= modified; stamp = stamp.wrapping_mul(0x100000001b3);
                }
                worktrees.insert(path.to_string_lossy().into_owned(), stamp);
            }
        }
    }
    DataSnapshot { projects, ledger, worktrees }
}

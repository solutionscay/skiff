//! Shows edits to the selected Host's `projects.toml` made outside the app:
//! `skiff projects add` and `remove`, `skiff theme project`, a hand edit
//! (#178, #223). As the ledger watcher does, it asks the Host for a stamp
//! (`projects@2`), once a second on This Mac and every 2 s on a remote Host.
//! The Host reads the file's bytes each time, so a write-then-rename is
//! seen as any other write, on Linux and macOS alike. A Host without
//! `projects@2` is not asked: its projects show again only on a new data
//! connection, as before.
//!
//! The app's own edits tell the page themselves ([`OwnEdit`]), from before
//! they are sent until they end, so the watcher keeps still for them: the
//! page reads the projects once per edit.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager};

use crate::connection::{data_now, data_read, App};

/// The page reloads projects on this event.
pub(crate) const PROJECTS_CHANGED: &str = "skiff:projects-changed";

/// How often the watcher asks the Host for the stamp.
const TICK: Duration = Duration::from_secs(1);
/// A remote Host is asked less often: each ask crosses the link.
const REMOTE_TICK: Duration = Duration::from_secs(2);

/// The app's own edits that ended so far. The watcher takes the stamp after
/// one as the page's, without a second reload.
static OWN: AtomicU64 = AtomicU64::new(0);
/// The app's own edits under way. The Host may have written the file before
/// the edit ends, so a stamp asked for meanwhile is not taken.
static BUSY: AtomicU64 = AtomicU64::new(0);

/// One edit of `projects.toml` by the app, from before it is sent until it
/// ends: done, refused, unknown or dropped. When it ends, the page reads the
/// projects again, once, unless it is [`OwnEdit::quiet`]. The watcher does
/// not count it as an edit from outside the app.
pub(crate) struct OwnEdit(Option<AppHandle>);

impl OwnEdit {
    /// An edit the page shows: it reloads the projects when the edit ends.
    pub(crate) fn begin(app: &AppHandle) -> Self {
        BUSY.fetch_add(1, Ordering::SeqCst);
        Self(Some(app.clone()))
    }

    /// An edit of the file that the projects do not show, such as the
    /// agents: no reload, and none from the watcher either.
    pub(crate) fn quiet() -> Self {
        BUSY.fetch_add(1, Ordering::SeqCst);
        Self(None)
    }
}

impl Drop for OwnEdit {
    fn drop(&mut self) {
        // OWN first: a tick that sees BUSY fall has seen OWN rise.
        OWN.fetch_add(1, Ordering::SeqCst);
        BUSY.fetch_sub(1, Ordering::SeqCst);
        if let Some(app) = &self.0 {
            let _ = app.emit(PROJECTS_CHANGED, ());
        }
    }
}

/// The app's own edits at one moment: those ended and those under way.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Own {
    ended: u64,
    busy: u64,
}

fn own_now() -> Own {
    // BUSY first: an edit that ends between the two loads shows as busy.
    let busy = BUSY.load(Ordering::SeqCst);
    Own { ended: OWN.load(Ordering::SeqCst), busy }
}

/// What the watcher saw last: the data connection, the stamp, and the
/// count of the app's own edits it has taken in.
#[derive(Debug, Default)]
struct Seen {
    last: Option<(u64, String)>,
    own: u64,
}

/// Runs at app start for the app's lifetime. Emits [`PROJECTS_CHANGED`]
/// when the selected Host's `projects.toml` changes in a way the app did
/// not make itself. It asks only over a live session plane and never
/// starts skiffd.
pub fn watch(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut seen = Seen::default();
        loop {
            let state = app.state::<App>();
            let tick = if state.remote() { REMOTE_TICK } else { TICK };
            if look(&state, &mut seen).await {
                let _ = app.emit(PROJECTS_CHANGED, ());
            }
            tokio::time::sleep(tick).await;
        }
    });
}

/// One tick. True when the projects changed from outside the app.
async fn look(app: &App, seen: &mut Seen) -> bool {
    if app.live_session().await.is_none() {
        return false;
    }
    // New Hosts push invalidations. Keep the stamp loop only for an older
    // Host that did not advertise the additive event capability.
    if data_now(app).await.is_ok_and(|c| c.hello().require(&skiff_host_api::Capability::data_events()).is_ok()) { return false; }
    let before = own_now();
    let stamp = data_read(app, |c| async move { skiff_client::host::data::projects_stamp(&c).await }).await;
    // No `projects@2`, or no data plane right now: nothing to compare.
    let Ok(stamp) = stamp else { return false };
    note(seen, (app.data_gen.load(Ordering::SeqCst), stamp), before, own_now())
}

/// Notes a stamp read, with the app's own edits before and after it was
/// asked for. True only for a change the page has not been told of:
/// - an own edit under way, or one that ended during the ask: the stamp may
///   be from before or after it, so it is not taken; a later tick takes it;
/// - the first read, or one on a new data connection: taken as the page's,
///   since the page reads the projects again for a new connection itself;
/// - an own edit since the last read: taken as the page's, which that edit
///   already told to read again;
/// - else a different stamp is an edit from outside the app.
///
/// An outside edit in the same tick as an own edit is taken in with it: the
/// page's read for the own edit may or may not show it, until the next
/// change or connection.
fn note(seen: &mut Seen, now: (u64, String), before: Own, after: Own) -> bool {
    if before != after || before.busy > 0 {
        return false;
    }
    let outside = match &seen.last {
        Some((connection, stamp)) => *connection == now.0 && seen.own == before.ended && *stamp != now.1,
        None => false,
    };
    seen.last = Some(now);
    seen.own = before.ended;
    outside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(ended: u64, busy: u64) -> Own {
        Own { ended, busy }
    }

    fn read(seen: &mut Seen, connection: u64, stamp: &str, ended: u64) -> bool {
        note(seen, (connection, stamp.into()), own(ended, 0), own(ended, 0))
    }

    #[test]
    fn an_outside_edit_reloads_once_and_the_apps_own_edit_does_not() {
        let mut seen = Seen::default();
        assert!(!read(&mut seen, 1, "a", 0), "the first read: the page has these projects");
        assert!(!read(&mut seen, 1, "a", 0), "nothing changed");
        assert!(read(&mut seen, 1, "b", 0), "`skiff projects add`, from outside the app");
        assert!(!read(&mut seen, 1, "b", 0), "told once only");
        // The app's own edit already told the page: no second reload.
        assert!(!read(&mut seen, 1, "c", 1), "the app's own edit");
        assert!(!read(&mut seen, 1, "c", 1));
        assert!(read(&mut seen, 1, "d", 1), "a hand edit after it");
    }

    /// The Host writes the file before the app's edit ends: a stamp read
    /// then must not count as an edit from outside the app.
    #[test]
    fn a_stamp_read_while_an_own_edit_runs_is_not_taken() {
        let mut seen = Seen::default();
        assert!(!read(&mut seen, 1, "a", 0));
        // The edit is under way for the whole ask; the Host already wrote.
        assert!(!note(&mut seen, (1, "b".into()), own(0, 1), own(0, 1)), "under way");
        // It began during the ask, or ended during it.
        assert!(!note(&mut seen, (1, "b".into()), own(0, 0), own(0, 1)), "began meanwhile");
        assert!(!note(&mut seen, (1, "b".into()), own(0, 1), own(1, 0)), "ended meanwhile");
        assert!(!note(&mut seen, (1, "b".into()), own(0, 0), own(1, 0)), "began and ended meanwhile");
        // Ended: the next tick takes its stamp as the page's.
        assert!(!read(&mut seen, 1, "b", 1), "the own edit's stamp, without a reload");
        assert!(!read(&mut seen, 1, "b", 1));
        assert!(read(&mut seen, 1, "c", 1), "an outside edit after it still shows");
    }

    #[test]
    fn a_new_data_connection_is_the_pages_own_reload() {
        let mut seen = Seen::default();
        assert!(!read(&mut seen, 1, "a", 0));
        // dataPlane.ts reads the projects again for a new connection.
        assert!(!read(&mut seen, 2, "b", 0), "a reconnect or a Host switch");
        assert!(read(&mut seen, 2, "c", 0));
    }

    #[test]
    fn an_own_edit_counts_from_begin_to_drop() {
        let start = own_now();
        let edit = OwnEdit::quiet();
        let during = own_now();
        assert_eq!((during.busy, during.ended), (start.busy + 1, start.ended));
        drop(edit);
        let after = own_now();
        assert_eq!((after.busy, after.ended), (start.busy, start.ended + 1));
    }
}

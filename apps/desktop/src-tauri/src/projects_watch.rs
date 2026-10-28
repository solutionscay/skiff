//! Watches `projects.toml` for edits from outside the app: `skiff projects
//! add`, `skiff theme project`, a hand edit. Polls once a second, as the
//! ledger watcher does: it reads the path each time, so a write-then-rename
//! is seen as any other write, on Linux and macOS alike.

use std::{sync::Mutex, time::Duration};

use skiff_core::config::{self, ProjectConfig, SkiffAgent};
use tauri::{AppHandle, Emitter};

use crate::config::PROJECTS_CHANGED;

/// How often the watcher reads the file.
const TICK: Duration = Duration::from_secs(1);

/// What the page reads again on [`PROJECTS_CHANGED`]: the projects, the app
/// theme and the Skiff agent. `None`: the file does not parse, and the page
/// shows the error.
type Snapshot = Option<(Vec<ProjectConfig>, Option<String>, SkiffAgent)>;

/// The file as the page last read it, or was told of it. `None` until the
/// first look.
static SEEN: Mutex<Option<Snapshot>> = Mutex::new(None);

fn snapshot() -> Snapshot {
    config::load().ok().map(|c| (c.projects, c.appearance.theme, c.skiff_agent))
}

/// The app wrote the file and the page has the change: the watcher keeps
/// still for it.
pub(crate) fn wrote() {
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    *seen = Some(snapshot());
}

/// The app wrote the file: the page reads it again, once.
pub(crate) fn changed(app: &AppHandle) {
    wrote();
    let _ = app.emit(PROJECTS_CHANGED, ());
}

/// Runs at app start on its own thread. Emits [`PROJECTS_CHANGED`] when the
/// file changes in a way the app did not make itself.
pub fn watch(app: AppHandle) {
    loop {
        {
            // Read under the lock, so an app edit between the read and the
            // compare is not taken for an outside one.
            let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
            let now = snapshot();
            if seen.as_ref().is_some_and(|s| *s != now) {
                let _ = app.emit(PROJECTS_CHANGED, ());
            }
            *seen = Some(now);
        }
        std::thread::sleep(TICK);
    }
}

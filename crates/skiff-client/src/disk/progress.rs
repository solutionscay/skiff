//! The count's running totals, which a page reads while the count runs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;

/// What the count has done so far. The walk and the git checks add to it
/// from many threads; [`Progress::now`] reads it at any time.
#[derive(Default)]
pub struct Progress {
    files: AtomicU64,
    bytes: AtomicU64,
    repos: AtomicU64,
    checked: AtomicU64,
    walked: AtomicBool,
}

/// [`Progress`] at one moment.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    /// Files walked.
    pub files: u64,
    /// Their bytes.
    pub bytes: u64,
    /// Git checkouts found.
    pub repos: u64,
    /// Git checkouts checked.
    pub checked: u64,
    /// The walk is done: only git checks remain.
    pub walked: bool,
}

impl Progress {
    pub fn now(&self) -> Counted {
        Counted {
            files: self.files.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            repos: self.repos.load(Ordering::Relaxed),
            checked: self.checked.load(Ordering::Relaxed),
            walked: self.walked.load(Ordering::Relaxed),
        }
    }

    pub(super) fn file(&self, bytes: u64) {
        self.files.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn repo(&self) {
        self.repos.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn checked(&self) {
        self.checked.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn walked(&self) {
        self.walked.store(true, Ordering::Relaxed);
    }
}

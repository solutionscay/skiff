//! Which app runs are alive, for peek sessions. An app run names itself on each
//! connection. It is alive while one such connection is open, and for
//! [`GRACE`] after the last one closes: a reload of skiffd closes them all, and
//! the app connects again at its next call.
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Longer than the app's 10 s session poll, which opens a new connection.
const GRACE: Duration = Duration::from_secs(30);

pub(super) struct PeekOwners {
    /// A reload forgets the connections: until then, each owner is alive.
    started: Instant,
    open: HashMap<u64, String>,
    /// When an owner's last connection closed. Dropped after [`GRACE`].
    closed: HashMap<String, Instant>,
}

impl PeekOwners {
    pub(super) fn new(started: Instant) -> Self {
        Self { started, open: HashMap::new(), closed: HashMap::new() }
    }

    pub(super) fn connected(&mut self, conn: u64, owner: String) {
        self.open.insert(conn, owner);
    }

    pub(super) fn closed(&mut self, conn: u64, now: Instant) {
        if let Some(owner) = self.open.remove(&conn) {
            self.closed.insert(owner, now);
        }
        self.closed.retain(|_, at| now.duration_since(*at) < GRACE);
    }

    pub(super) fn live(&self, owner: &str, now: Instant) -> bool {
        self.open.values().any(|o| o == owner) || now.duration_since(*self.closed.get(owner).unwrap_or(&self.started)) < GRACE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second app must not see a live owner as gone, and a closed app's
    /// peek must come free once the time to reconnect has passed.
    #[test]
    fn an_owner_lives_while_connected_and_for_the_grace_after() {
        let start = Instant::now();
        let mut owners = PeekOwners::new(start);
        let later = start + GRACE * 2;
        assert!(owners.live("a", start), "a reload forgets connections: the owner may be on its way back");
        assert!(!owners.live("a", later), "nobody came back");
        owners.connected(1, "a".into());
        owners.connected(2, "a".into());
        assert!(owners.live("a", later));
        owners.closed(1, later);
        assert!(owners.live("a", later + GRACE * 2), "another connection stays open");
        owners.closed(2, later);
        assert!(owners.live("a", later + GRACE / 2));
        assert!(!owners.live("a", later + GRACE));
        owners.connected(3, "a".into());
        assert!(owners.live("a", later + GRACE * 3), "it reconnected");
    }
}

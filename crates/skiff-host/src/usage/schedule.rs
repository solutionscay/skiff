//! When to read an agent's usage (decision 16). Pure: the caller gives the
//! time and whether the agent runs, and starts the read this allows.
//!
//! - The page opens: read each agent whose last read is older than 60 s,
//!   running or not. Read now reads every agent, running or not.
//! - A pane of the agent ends a turn: read it, at most once per 60 s. A turn
//!   end inside the 60 s queues nothing.
//! - Poll: read each running agent 15 minutes after its last read. After a
//!   failed read the wait doubles, up to 30 minutes.
//! - An agent with no running pane is read only on page open and Read now.
//! - One read at a time per agent.

/// The page reads an agent again after this long.
pub const STALE_MS: u64 = 60_000;
/// At most one turn-end read per agent in this long.
pub const TURN_GAP_MS: u64 = 60_000;
/// The poll of a running agent.
pub const POLL_MS: u64 = 15 * 60_000;
/// The longest poll wait after failed reads.
pub const POLL_MAX_MS: u64 = 30 * 60_000;

/// What asks for a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Settings › Usage opened.
    PageOpen,
    /// The person pressed Read now.
    ReadNow,
    /// A pane of the agent ended a turn (OSC 7501, `Event::TurnEnded`).
    TurnEnd,
    /// The poll tick.
    Poll,
}

/// One agent's read times.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clock {
    /// Unix ms when the last read started.
    started: Option<u64>,
    /// Failed reads in a row.
    failures: u32,
    /// A read runs now.
    busy: bool,
}

impl Clock {
    /// `trigger` at `now` reads the agent. `running`: it has a pane open.
    pub fn wants(&self, trigger: Trigger, now: u64, running: bool) -> bool {
        if self.busy {
            return false;
        }
        let since = self.started.map_or(u64::MAX, |t| now.saturating_sub(t));
        match trigger {
            Trigger::ReadNow => true,
            Trigger::PageOpen => since >= STALE_MS,
            Trigger::TurnEnd => running && since >= TURN_GAP_MS,
            Trigger::Poll => running && since >= self.poll_wait(),
        }
    }

    /// The poll wait: 15 minutes, doubled after each failed read, up to 30.
    pub fn poll_wait(&self) -> u64 {
        POLL_MS.saturating_mul(1u64 << self.failures.min(16)).min(POLL_MAX_MS)
    }

    /// A read starts at `now`.
    pub fn start(&mut self, now: u64) {
        self.busy = true;
        self.started = Some(now);
    }

    /// The read ended. `ok`: it worked.
    pub fn finish(&mut self, ok: bool) {
        self.busy = false;
        self.failures = if ok { 0 } else { self.failures.saturating_add(1) };
    }

    pub fn busy(&self) -> bool {
        self.busy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    /// A clock whose read at `at` finished.
    fn read_at(at: u64, ok: bool) -> Clock {
        let mut c = Clock::default();
        c.start(at);
        c.finish(ok);
        c
    }

    #[test]
    fn page_open_reads_stale_agents_running_or_not() {
        assert!(Clock::default().wants(Trigger::PageOpen, 0, false));
        let c = read_at(10 * MIN, true);
        assert!(!c.wants(Trigger::PageOpen, 10 * MIN + 59_999, false));
        assert!(c.wants(Trigger::PageOpen, 11 * MIN, false));
        assert!(c.wants(Trigger::PageOpen, 11 * MIN, true));
    }

    #[test]
    fn read_now_reads_every_agent() {
        let c = read_at(10 * MIN, true);
        assert!(c.wants(Trigger::ReadNow, 10 * MIN + 1, false));
        assert!(c.wants(Trigger::ReadNow, 10 * MIN + 1, true));
    }

    #[test]
    fn turn_end_reads_at_most_once_per_minute() {
        let mut c = Clock::default();
        assert!(c.wants(Trigger::TurnEnd, 0, true));
        c.start(0);
        c.finish(true);
        // A second turn end 30 s later reads nothing, and queues nothing.
        assert!(!c.wants(Trigger::TurnEnd, 30_000, true));
        // The next turn end after the 60 s reads again.
        assert!(c.wants(Trigger::TurnEnd, 61_000, true));
        // A page-open read counts too.
        let c = read_at(5 * MIN, true);
        assert!(!c.wants(Trigger::TurnEnd, 5 * MIN + 10_000, true));
    }

    #[test]
    fn poll_every_15_minutes_while_running() {
        let c = read_at(0, true);
        assert!(!c.wants(Trigger::Poll, 14 * MIN, true));
        assert!(c.wants(Trigger::Poll, 15 * MIN, true));
    }

    #[test]
    fn no_read_without_a_running_pane() {
        let c = read_at(0, true);
        assert!(!c.wants(Trigger::Poll, 60 * MIN, false));
        assert!(!c.wants(Trigger::TurnEnd, 60 * MIN, false));
        assert!(!Clock::default().wants(Trigger::Poll, 0, false));
        // Page open and Read now still read it.
        assert!(c.wants(Trigger::PageOpen, 60 * MIN, false));
        assert!(c.wants(Trigger::ReadNow, 60 * MIN, false));
    }

    #[test]
    fn failed_reads_double_the_poll_up_to_30_minutes() {
        let mut c = read_at(0, false);
        assert_eq!(c.poll_wait(), 30 * MIN);
        assert!(!c.wants(Trigger::Poll, 15 * MIN, true));
        assert!(c.wants(Trigger::Poll, 30 * MIN, true));
        c.start(30 * MIN);
        c.finish(false);
        assert_eq!(c.poll_wait(), 30 * MIN);
        // The backoff is the poll's only: a turn end still reads after 60 s.
        assert!(c.wants(Trigger::TurnEnd, 31 * MIN, true));
        // A read that works ends it.
        c.start(31 * MIN);
        c.finish(true);
        assert_eq!(c.poll_wait(), 15 * MIN);
    }

    #[test]
    fn one_read_at_a_time() {
        let mut c = Clock::default();
        c.start(0);
        for t in [Trigger::ReadNow, Trigger::PageOpen, Trigger::TurnEnd, Trigger::Poll] {
            assert!(!c.wants(t, 60 * MIN, true), "{t:?}");
        }
    }
}

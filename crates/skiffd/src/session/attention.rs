//! Fallback attention policy.
use super::output::Scan;
use super::{wait_log, Front, Session, SessionPool};
use super::reducer::{self, Effects, Input};
use crate::program_status::Records;
use crate::screen::{title_state, TitleState};
use skiff_core::protocol::Event;
use skiff_core::session::{now_ms, SessionInfo, SessionState};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

impl SessionPool {
    /// A bell from a shell is ignored; an unclaimed command may ring.
    pub(super) fn bell(&self, session: &Session) {
        self.apply_policy(session, Input::Bell, None);
    }

    pub(super) fn apply_scan(&self, session: &Session, scan: Scan) {
        if !scan.scanned { return; }
        let asks = *session.title_state.lock().unwrap() == Some(TitleState::Action);
        *session.prompt_match.lock().unwrap() = scan.prompt;
        self.apply_policy(session, Input::Scan {
            work: scan.work, busy: scan.busy,
            approval: scan.approval, asks,
        }, None);
    }

    pub(super) fn apply_title(&self, session: &Session, title: Option<&str>) {
        let Front::Agent(agent) = session.front() else { return };
        let new = title.and_then(|t| title_state(agent, t));
        let old = std::mem::replace(&mut *session.title_state.lock().unwrap(), new);
        // `info.title` still holds the title before this one.
        self.apply_policy(session, Input::Title { old, new }, title);
    }

    /// Sets `waiting` and records why.
    #[cfg(test)]
    pub(super) fn wait(&self, session: &Session, cause: &'static str) {
        self.move_state(session, SessionState::Waiting, Some(cause));
    }

    /// Sets any state but `waiting`, which [`Self::wait`] sets with its cause.
    pub(super) fn set_state(&self, session: &Session, state: SessionState) {
        debug_assert_ne!(state, SessionState::Waiting, "waiting needs a cause");
        self.move_state(session, state, None);
    }

    fn move_state(&self, session: &Session, target: SessionState, cause: Option<&'static str>) {
        self.apply_policy(session, Input::Move { target, cause }, None);
    }

    fn apply_policy(&self, session: &Session, input: Input, title: Option<&str>) {
        let now = now_ms();
        let (id, effects) = {
            let mut info = session.info.lock().unwrap();
            let effects = step(session, &mut info, input, now, title);
            if effects.work_started { session.work_started.store(now, Ordering::Relaxed); }
            if effects.last_work { session.last_work.store(now, Ordering::Relaxed); }
            if effects.busy_seen { session.busy_seen.store(true, Ordering::Relaxed); }
            if let Some(value) = effects.belled { session.belled.store(value, Ordering::Relaxed); }
            if let Some(value) = effects.raised { session.raised.store(value, Ordering::Relaxed); }
            if let Some(value) = effects.approval_since { session.approval_since.store(value, Ordering::Relaxed); }
            (info.id.clone(), effects)
        };
        if let Some(state) = effects.state { let _ = self.events.send(Event::State { session: id, state }); }
        if effects.update { let _ = self.events.send(Event::SessionUpdated { session: session.info() }); }
    }

    /// Flips quiet `working` sessions to `idle`. For an agent, the end of a
    /// stretch of work is the end of its turn: a result to read.
    pub fn spawn_idle_watcher(self: &Arc<Self>) {
        let pool = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                {
                    let Some(_quiet) = pool.quiet() else { continue };
                    let sessions: Vec<Arc<Session>> =
                        pool.sessions.read().unwrap().values().cloned().collect();
                    for s in sessions {
                        pool.status_tick(&s);
                        pool.idle_if_quiet(&s);
                    }
                }
                // Out of the guard: a kill takes it again.
                pool.end_retired();
            }
        });
    }

    pub(super) fn idle_if_quiet(&self, s: &Session) {
        self.idle_if_quiet_at(s, now_ms());
    }

    pub(super) fn idle_if_quiet_at(&self, s: &Session, now: u64) {
        let (id, updated, effects) = {
            let mut info = s.info.lock().unwrap();
            let title = *s.title_state.lock().unwrap();
            let effects = step(s, &mut info, Input::Idle { title }, now, None);
            if effects.reset_fresh_busy {
                s.fresh.store(false, Ordering::Relaxed);
                s.busy_seen.store(false, Ordering::Relaxed);
            }
            (info.id.clone(), effects.update.then(|| info.clone()), effects)
        };
        if let Some(state) = effects.state {
            let _ = self.events.send(Event::State { session: id.clone(), state });
        }
        if effects.turn { let _ = self.events.send(Event::TurnEnded { session: id }); }
        if let Some(session) = updated { let _ = self.events.send(Event::SessionUpdated { session }); }
    }
}

/// The causes of `waiting`, as `SessionInfo::waiting_cause` names them.
/// General ones, for any terminal program.
pub(super) const BELL: &str = "bell";
pub(super) const PROMPT: &str = "prompt";
pub(super) const TITLE: &str = "title";
pub(super) const REPORT: &str = "report";

pub(super) fn snapshot(session: &Session, info: &SessionInfo) -> reducer::State {
    reducer::State {
        info: info.clone(), front: session.front(),
        last_work: session.last_work.load(Ordering::Relaxed),
        work_started: session.work_started.load(Ordering::Relaxed),
        fresh: session.fresh.load(Ordering::Relaxed),
        busy: session.busy_seen.load(Ordering::Relaxed),
        belled: session.belled.load(Ordering::Relaxed),
        raised: session.raised.load(Ordering::Relaxed),
        approval_since: session.approval_since.load(Ordering::Relaxed),
        last_output: session.last_output.load(Ordering::Relaxed),
        status_dirty: session.status_dirty.load(Ordering::Relaxed),
        status_sent_at: session.status_sent_at.load(Ordering::Relaxed),
    }
}

/// One step of the reducer that can change the state: applies it to `info`
/// and writes the log's lines for a wait that starts or ends. `title` is the
/// title that came with this step, when `info` does not hold it yet.
pub(super) fn step(session: &Session, info: &mut SessionInfo, input: Input, now: u64, title: Option<&str>) -> Effects {
    let old = info.state;
    let ended_by = input.ended_by();
    let (next, effects) = reducer::reduce(snapshot(session, info), input, now);
    *info = next.info;
    if old != SessionState::Waiting && info.state == SessionState::Waiting {
        session.waiting_since.store(now, Ordering::Relaxed);
    }
    if let Some(cause) = effects.cause_recorded {
        let prompt = session.prompt_match.lock().unwrap();
        let last_report = session.last_report_at.load(Ordering::Relaxed);
        let line = wait_log::waits_line(info, session.front(), cause, prompt.as_ref(), title, last_report, now);
        tracing::info!("{line}");
    }
    if old == SessionState::Waiting && info.state != SessionState::Waiting {
        let since = session.waiting_since.swap(0, Ordering::Relaxed);
        tracing::info!("{}", wait_log::stops_line(&info.id, since, now, ended_by));
    }
    effects
}

/// A live report decides the state: one kept after its program left does not.
pub(super) fn live(info: &SessionInfo) -> bool {
    info.program_status.as_ref().is_some_and(|p| !p.ended)
}

/// The program that reported left: it exited, its shell's prompt came back,
/// or another one came in front. Its records go; a done or error stays to be
/// read, marked ended, until the pane is seen or new work starts. True when
/// `program_status` changed.
pub(super) fn end_reports(records: &mut Records, info: &mut SessionInfo) -> bool {
    let had = records.root().is_some();
    let kept = records.end();
    let new = if had {
        kept
    } else {
        info.program_status.clone().filter(|p| p.ended)
    };
    let changed = info.program_status != new;
    info.program_status = new;
    changed
}

/// The title without the status glyph an agent leads it with: Claude's ◐
/// and ◑, Codex's braille spinner. See `screen::title_state`.
pub(super) fn topic(title: &str) -> &str {
    title.trim_start_matches(|c: char| !c.is_alphanumeric())
}

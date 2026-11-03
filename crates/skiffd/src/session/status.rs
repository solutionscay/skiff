//! OSC 7501 session status policy.
use super::attention::{end_reports, log_cause, snapshot};
use super::reducer::{self, Input};
use super::{Front, Session, SessionPool};
use crate::program_status;
use crate::program_status::Records;
use crate::screen::StatusSignal;
use skiff_core::protocol::Event;
use skiff_core::session::{now_ms, SessionState};
use std::sync::atomic::Ordering;

impl SessionPool {
    /// Applies what the program reported with OSC 7501, in order. While a
    /// root record lives, it decides the state; see
    /// [`program_status::session_state`] and [`program_status::ends_turn`].
    pub(super) fn apply_status(&self, session: &Session, signals: Vec<StatusSignal>) {
        self.apply_status_at(session, signals, now_ms());
    }

    pub(super) fn apply_status_at(&self, session: &Session, signals: Vec<StatusSignal>, now: u64) {
        if signals.is_empty() {
            return;
        }
        // A shell at its prompt reports nothing. A report while it is in
        // front comes from a command that is gone, or from printed text, and
        // no return of the prompt would ever drop it.
        if session.front() == Front::Shell {
            return;
        }
        // Records belong to the process group in front when they came. A
        // report under another group is another program's: the old records
        // went with their program.
        let group = session.foreground_group().unwrap_or(0);
        let mut records = session.records.lock().unwrap();
        if group != 0 && records.group != 0 && records.group != group {
            *records = Records::default();
        }
        let old = records.root().map(|r| r.state);
        for signal in signals {
            match signal {
                StatusSignal::Reset => records.clear(),
                StatusSignal::Report(body) => {
                    if let Some(report) = program_status::parse(&body) {
                        records.apply(report);
                    }
                }
            }
        }
        if !records.is_empty() {
            records.group = group;
        }
        let claim = records.root().map(|r| r.state);
        let (state, ended, id, send) = {
            let mut info = session.info.lock().unwrap();
            // A result kept from a program that left stays while no new
            // records replace it.
            let view = records.view().or_else(|| {
                info.program_status
                    .clone()
                    .filter(|p| p.ended && old.is_none() && records.is_empty())
            });
            // A turn ends with done or error, or for a program that never
            // says them, with idle after a turn the user started.
            let ended = records
                .turns
                .step(old, claim, now, info.input_gen, info.output_bytes);
            drop(records);
            let (next, effects) = reducer::reduce(snapshot(session, &info), Input::Report { claim, view, ended }, now);
            *info = next.info;
            if let Some(value) = effects.stalled { session.stalled.store(value, Ordering::Relaxed); }
            if let Some(value) = effects.approval_since { session.approval_since.store(value, Ordering::Relaxed); }
            if let Some(value) = effects.raised { session.raised.store(value, Ordering::Relaxed); }
            if effects.last_work { session.last_work.store(now, Ordering::Relaxed); }
            if let Some(value) = effects.belled { session.belled.store(value, Ordering::Relaxed); }
            if effects.reset_fresh_busy { session.fresh.store(false, Ordering::Relaxed); session.busy_seen.store(false, Ordering::Relaxed); }
            if effects.work_started { session.work_started.store(now, Ordering::Relaxed); }
            if effects.status_sent { session.status_sent_at.store(now, Ordering::Relaxed); }
            if let Some(value) = effects.status_dirty { session.status_dirty.store(value, Ordering::Relaxed); }
            if let Some(cause) = effects.cause_recorded {
                let detail = info.program_status.as_ref()
                    .map(|p| [p.kind.as_deref(), p.msg.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(": "))
                    .unwrap_or_default();
                log_cause(&info, session.front(), cause, &detail);
            }
            (effects.state, effects.turn, info.id.clone(), effects.update)
        };
        if let Some(state) = state {
            let _ = self.events.send(Event::State {
                session: id.clone(),
                state,
            });
        }
        if ended {
            let _ = self.events.send(Event::TurnEnded {
                session: id.clone(),
            });
        }
        // Read again out of the lock: `info()` adds the time of the last output.
        if send {
            let _ = self.events.send(Event::SessionUpdated {
                session: session.info(),
            });
        }
    }

    /// Another process group came in front of the PTY. Reports made under
    /// the one before are its program's: they stop deciding the state, and
    /// only a done or error stays to be read.
    pub(crate) fn status_group(&self, session: &Session, group: u32) {
        let (state, updated) = {
            let mut records = session.records.lock().unwrap();
            if records.is_empty() || records.group == 0 || records.group == group {
                return;
            }
            let mut info = session.info.lock().unwrap();
            if info.state == SessionState::Done {
                return;
            }
            let changed = end_reports(&mut records, &mut info);
            let (next, effects) = reducer::reduce(snapshot(session, &info), Input::ClaimLost, now_ms());
            *info = next.info;
            if effects.work_started { session.work_started.store(now_ms(), Ordering::Relaxed); }
            let moved = effects.state.map(|state| (info.id.clone(), state));
            (moved, changed.then(|| info.clone()))
        };
        if let Some((session, state)) = state {
            let _ = self.events.send(Event::State { session, state });
        }
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
    }

    /// Once a tick, for a program that reports: its approval prompt that
    /// outstayed the grace, a long silence while it says it works, and an
    /// update held back by [`super::STATUS_EVERY_MS`].
    pub(super) fn status_tick(&self, s: &Session) {
        self.status_tick_at(s, now_ms());
    }

    pub(super) fn status_tick_at(&self, s: &Session, now: u64) {
        let (state, updated) = {
            let mut info = s.info.lock().unwrap();
            let (next, effects) = reducer::reduce(snapshot(s, &info), Input::Tick, now);
            *info = next.info;
            if effects.clear_status_dirty { s.status_dirty.store(false, Ordering::Relaxed); }
            if effects.status_sent { s.status_sent_at.store(now, Ordering::Relaxed); }
            if let Some(value) = effects.stalled { s.stalled.store(value, Ordering::Relaxed); }
            if let Some(value) = effects.raised { s.raised.store(value, Ordering::Relaxed); }
            if let Some(cause) = effects.cause_recorded { log_cause(&info, s.front(), cause, "under a working report"); }
            (effects.state.map(|value| (info.id.clone(), value)), effects.update)
        };
        if let Some((session, state)) = state {
            let _ = self.events.send(Event::State { session, state });
        }
        if updated {
            let _ = self
                .events
                .send(Event::SessionUpdated { session: s.info() });
        }
    }
}

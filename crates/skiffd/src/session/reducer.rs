//! Pure session transitions. The caller supplies observations and time.
use super::attention::{live, BELL, PROMPT, REPORT, TITLE};
use super::{Front, AUTH_HOLD_MS, GRACE_MS, IDLE_AFTER, MIN_TURN_MS, STALL_MS, STATUS_EVERY_MS};
use crate::program_status::{self, State as Claim};
use crate::screen::{has_busy_line, TitleState};
use skiff_core::session::{ProgramStatus, SessionInfo, SessionState};

#[derive(Clone)]
pub(super) struct State {
    pub info: SessionInfo,
    pub front: Front,
    pub last_work: u64,
    pub work_started: u64,
    pub fresh: bool,
    pub busy: bool,
    pub belled: bool,
    pub raised: bool,
    pub approval_since: u64,
    pub last_output: u64,
    pub status_dirty: bool,
    pub status_sent_at: u64,
}

pub(super) enum Input {
    Move {
        target: SessionState,
        cause: Option<&'static str>,
    },
    Report {
        claim: Option<Claim>,
        view: Option<ProgramStatus>,
        ended: bool,
    },
    ClaimLost,
    Shell,
    Exit {
        code: Option<i32>,
    },
    Seen,
    Idle {
        title: Option<TitleState>,
    },
    Bell,
    Tick,
    Scan {
        work: bool,
        busy: bool,
        approval: bool,
        asks: bool,
    },
    Title {
        old: Option<TitleState>,
        new: Option<TitleState>,
    },
}

#[derive(Default)]
pub(super) struct Effects {
    pub state: Option<SessionState>,
    pub update: bool,
    pub turn: bool,
    pub work_started: bool,
    pub reset_fresh_busy: bool,
    pub cause_recorded: Option<&'static str>,
    pub last_work: bool,
    pub busy_seen: bool,
    pub belled: Option<bool>,
    pub raised: Option<bool>,
    pub approval_since: Option<u64>,
    pub stalled: Option<bool>,
    pub status_sent: bool,
    pub status_dirty: Option<bool>,
    pub dirty: bool,
}

fn is_auth_block(info: &SessionInfo) -> bool {
    info.program_status
        .as_ref()
        .is_some_and(|p| p.state == "blocked" && p.kind.as_deref() == Some("auth"))
}

/// Whether the session shows an `auth` block inside the first
/// [`AUTH_HOLD_MS`] after its start.
fn auth_held(info: &SessionInfo, now: u64) -> bool {
    is_auth_block(info) && now.saturating_sub(info.started_at) < AUTH_HOLD_MS
}

/// All time is supplied by the caller. The returned effects describe work to
/// perform after releasing the session lock.
pub(super) fn reduce(mut state: State, input: Input, now: u64) -> (State, Effects) {
    let mut effects = Effects::default();
    match input {
        Input::Move {
            target,
            cause,
        } => {
            if state.info.state == target && cause.is_some() && state.info.waiting_cause.is_none() {
                state.info.waiting_cause = cause.map(str::to_owned);
                effects.cause_recorded = cause;
                effects.update = true;
            } else if state.info.state != target && state.info.state != SessionState::Done {
                effects.work_started =
                    target == SessionState::Working && state.info.state == SessionState::Idle;
                state.info.state = target;
                state.info.waiting_cause = cause
                    .filter(|_| target == SessionState::Waiting)
                    .map(str::to_owned);
                effects.cause_recorded = cause.filter(|_| target == SessionState::Waiting);
                if target == SessionState::Working
                    && state.info.program_status.as_ref().is_some_and(|p| p.ended)
                {
                    state.info.program_status = None;
                    effects.update = true;
                }
                effects.state = Some(target);
            }
        }
        Input::Report { claim, view, ended } => {
            if state.info.state == SessionState::Done || state.info.program_status == view {
                return (state, effects);
            }
            let minor = matches!((&state.info.program_status, &view),
                (Some(a), Some(b)) if program_status::Records::minor_change(a, b));
            state.info.program_status = view;
            effects.stalled = Some(false);
            effects.approval_since = Some(0);
            effects.raised = Some(false);
            effects.last_work = true;
            if claim.is_some() {
                effects.belled = Some(false);
            }
            if ended {
                state.info.unread = true;
                effects.reset_fresh_busy = true;
                effects.turn = true;
            }
            let mut target = program_status::session_state(claim, state.info.state);
            // A program that signs in by itself says so with a short auth
            // block at its start: hold it as working; the tick ends the hold.
            if target == SessionState::Waiting && auth_held(&state.info, now) {
                target = SessionState::Working;
            }
            let changed = state.info.state != target;
            effects.work_started = changed
                && target == SessionState::Working
                && state.info.state == SessionState::Idle;
            state.info.state = target;
            if changed || (target == SessionState::Waiting && state.info.waiting_cause.is_none()) {
                state.info.waiting_cause =
                    (target == SessionState::Waiting).then(|| REPORT.to_string());
                effects.cause_recorded = (target == SessionState::Waiting).then_some(REPORT);
            }
            effects.state = changed.then_some(target);
            effects.update = !minor || now.saturating_sub(state.status_sent_at) >= STATUS_EVERY_MS;
            effects.status_sent = effects.update;
            effects.status_dirty = Some(!effects.update);
        }
        Input::Shell => {
            if state.info.state != SessionState::Done {
                if state.info.state != SessionState::Idle {
                    state.info.state = SessionState::Idle;
                    state.info.waiting_cause = None;
                    effects.state = Some(SessionState::Idle);
                }
                effects.update = !state.info.unread;
                state.info.unread = true;
                effects.turn = true;
            }
        }
        Input::Exit { code } => {
            state.info.exit_code = code;
            state.info.state = SessionState::Done;
            state.info.waiting_cause = None;
        }
        Input::Seen => {
            let kept = state.info.program_status.as_ref().is_some_and(|p| p.ended);
            if kept {
                state.info.program_status = None;
            }
            effects.update = state.info.unread || state.info.agent_exit.is_some() || kept;
            state.info.unread = false;
            effects.dirty = state.info.agent_exit.take().is_some();
        }
        Input::ClaimLost => {
            let target = program_status::session_state(None, state.info.state);
            if target != state.info.state {
                state.info.state = target;
                state.info.waiting_cause = None;
                effects.state = Some(target);
            }
        }
        // An observed prompt can override a claim after its grace; a live
        // claim otherwise takes precedence over inferred screen and title data.
        Input::Tick => {
            if state.info.state == SessionState::Done
                || !live(&state.info)
            {
                return (state, effects);
            }
            // The hold on a start-up auth block ran out and the block stays.
            if state.info.state == SessionState::Working
                && is_auth_block(&state.info)
                && !auth_held(&state.info, now)
            {
                let (next, transition) = reduce(
                    state,
                    Input::Move { target: SessionState::Waiting, cause: Some(REPORT) },
                    now,
                );
                effects.state = transition.state;
                effects.cause_recorded = transition.cause_recorded;
                effects.update = true;
                effects.status_sent = true;
                return (next, effects);
            }
            effects.update = state.status_dirty;
            effects.status_dirty = Some(false);
            let working = state
                .info
                .program_status
                .as_ref()
                .is_some_and(|p| p.state == "working");
            let quiet = now.saturating_sub(state.last_output) >= STALL_MS;
            if let Some(status) = state
                .info
                .program_status
                .as_mut()
                .filter(|p| working && quiet && !p.stalled)
            {
                status.stalled = true;
                effects.stalled = Some(true);
                effects.update = true;
            }
            if working
                && state.approval_since != 0
                && now.saturating_sub(state.approval_since) >= GRACE_MS
                && state.info.state == SessionState::Working
                && !state.raised
            {
                let (next, transition) = reduce(
                    state,
                    Input::Move {
                        target: SessionState::Waiting,
                        cause: Some(PROMPT),
                    },
                    now,
                );
                effects.raised = Some(true);
                effects.state = transition.state;
                effects.cause_recorded = transition.cause_recorded;
                effects.status_sent = effects.update;
                return (next, effects);
            }
            effects.status_sent = effects.update;
        }
        Input::Bell => {
            let claimed = live(&state.info);
            if !claimed && matches!(state.front, Front::Command | Front::Program) {
                let (next, mut result) = reduce(
                    state,
                    Input::Move {
                        target: SessionState::Waiting,
                        cause: Some(BELL),
                    },
                    now,
                );
                result.belled = Some(true);
                return (next, result);
            }
        }
        Input::Scan {
            work,
            busy,
            approval,
            asks,
        } => {
            if state.front == Front::Shell {
                return (state, effects);
            }
            effects.last_work = work;
            effects.busy_seen = busy;
            let claimed = live(&state.info);
            let mut target = None;
            if claimed {
                if approval {
                    if state.approval_since == 0 {
                        effects.approval_since = Some(now);
                    }
                } else {
                    effects.approval_since = Some(0);
                    effects.raised = Some(false);
                    if state.raised
                        && state
                            .info
                            .program_status
                            .as_ref()
                            .is_some_and(|p| p.state == "working")
                    {
                        target = Some((SessionState::Working, None));
                    }
                }
            } else if approval || asks {
                effects.belled = Some(false);
                target = Some((
                    SessionState::Waiting,
                    Some(if approval { PROMPT } else { TITLE }),
                ));
            } else if state.info.state == SessionState::Waiting
                && matches!(state.front, Front::Agent(_))
                && !state.belled
            {
                effects.last_work = true;
                target = Some((SessionState::Working, None));
            } else if work && state.info.state != SessionState::Waiting {
                target = Some((SessionState::Working, None));
            }
            if let Some((target, cause)) = target {
                let (next, result) = reduce(
                    state,
                    Input::Move {
                        target,
                        cause,
                    },
                    now,
                );
                effects.state = result.state;
                effects.update = result.update;
                effects.work_started = result.work_started;
                effects.cause_recorded = result.cause_recorded;
                return (next, effects);
            }
        }
        Input::Title { old, new } => {
            let claimed = live(&state.info);
            if old == new || claimed {
                return (state, effects);
            }
            let target = match new {
                Some(TitleState::Busy) => {
                    effects.busy_seen = true;
                    effects.last_work = true;
                    (state.info.state != SessionState::Waiting)
                        .then_some((SessionState::Working, None))
                }
                Some(TitleState::Action) => {
                    effects.belled = Some(false);
                    Some((SessionState::Waiting, Some(TITLE)))
                }
                Some(TitleState::Ready) | None => {
                    effects.last_work = old == Some(TitleState::Busy);
                    None
                }
            };
            if let Some((target, cause)) = target {
                let (next, result) = reduce(
                    state,
                    Input::Move {
                        target,
                        cause,
                    },
                    now,
                );
                effects.state = result.state;
                effects.update = result.update;
                effects.work_started = result.work_started;
                effects.cause_recorded = result.cause_recorded;
                return (next, effects);
            }
        }
        Input::Idle { title } => {
            let quiet = now.saturating_sub(state.last_work) > IDLE_AFTER.as_millis() as u64;
            if title != Some(TitleState::Busy)
                && quiet
                && state.info.state == SessionState::Working
                && !live(&state.info)
            {
                state.info.state = SessionState::Idle;
                let stretch = state.last_work.saturating_sub(state.work_started);
                effects.turn = match state.front {
                    Front::Agent(agent) if has_busy_line(agent) => state.busy,
                    Front::Agent(_) => !state.fresh && stretch >= MIN_TURN_MS,
                    Front::Program => !state.fresh,
                    Front::Shell | Front::Command => false,
                };
                effects.reset_fresh_busy = true;
                effects.update = effects.turn && !state.info.unread;
                if effects.update {
                    state.info.unread = true;
                }
                effects.state = Some(SessionState::Idle);
            }
        }
    }
    (state, effects)
}


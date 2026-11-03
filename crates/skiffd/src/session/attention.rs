//! Fallback attention policy.

use super::*;

impl SessionPool {
    /// A bell outside the echo of a key. A shell rings for its own reasons.
    /// A supported agent says what it wants on the screen, and its bell does
    /// not say why it rang. For every other program the bell is the call.
    pub(super) fn bell(&self, session: &Session) {
        if session.claimed() {
            return;
        }
        if matches!(session.front(), Front::Command | Front::Program) {
            session.belled.store(true, Ordering::Relaxed);
            self.wait(session, BELL, "");
        }
    }

    /// Work is new text on screen. An echo of typing, a redraw after a resize,
    /// mode codes an idle program re-sends, or a logo that only changes color
    /// are not. Text at a shell prompt is not work either.
    pub(super) fn apply_scan(&self, session: &Session, scan: Scan) {
        if !scan.scanned {
            return;
        }
        let front = session.front();
        if front == Front::Shell {
            return;
        }
        if scan.work {
            session.last_work.store(now_ms(), Ordering::Relaxed);
        }
        if scan.busy {
            session.busy_seen.store(true, Ordering::Relaxed);
        }
        if session.claimed() {
            // The agent's own approval prompt under a report that it works:
            // it may ask without saying so. The tick raises it after the
            // grace; the prompt's end lowers it again.
            if scan.approval {
                let _ = session.approval_since.compare_exchange(
                    0,
                    now_ms(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
            } else {
                session.approval_since.store(0, Ordering::Relaxed);
                let working = session
                    .info
                    .lock()
                    .unwrap()
                    .program_status
                    .as_ref()
                    .is_some_and(|p| p.state == "working");
                if session.raised.swap(false, Ordering::Relaxed) && working {
                    self.set_state(session, SessionState::Working);
                }
            }
            return;
        }
        let waiting = session.info.lock().unwrap().state == SessionState::Waiting;
        let asks = *session.title_state.lock().unwrap() == Some(TitleState::Action);
        if scan.approval || asks {
            session.belled.store(false, Ordering::Relaxed);
            let (cause, detail) = if scan.approval {
                (PROMPT, String::new())
            } else {
                (TITLE, title_detail(session))
            };
            self.wait(session, cause, &detail);
        } else if waiting && matches!(front, Front::Agent(_)) && !session.belled.load(Ordering::Relaxed) {
            // The prompt left the screen: the user answered it.
            session.last_work.store(now_ms(), Ordering::Relaxed);
            self.set_state(session, SessionState::Working);
        } else if scan.work && !waiting {
            self.set_state(session, SessionState::Working);
        }
    }

    /// The window title of a supported agent: busy keeps it working and
    /// proves a turn, an action-required title sets the bell.
    pub(super) fn apply_title(&self, session: &Session, title: Option<&str>) {
        let Front::Agent(agent) = session.front() else { return };
        let state = title.and_then(|t| title_state(agent, t));
        let old = std::mem::replace(&mut *session.title_state.lock().unwrap(), state);
        // A program that reports with OSC 7501 is read by its reports alone.
        // Claude Code drops its spinner from the title once it reports.
        if old == state || session.claimed() {
            return;
        }
        match state {
            Some(TitleState::Busy) => {
                session.busy_seen.store(true, Ordering::Relaxed);
                session.last_work.store(now_ms(), Ordering::Relaxed);
                if session.info.lock().unwrap().state != SessionState::Waiting {
                    self.set_state(session, SessionState::Working);
                }
            }
            Some(TitleState::Action) => {
                session.belled.store(false, Ordering::Relaxed);
                self.wait(session, TITLE, &title_detail(session));
            }
            Some(TitleState::Ready) | None => {
                // The busy title kept the session working. Its end starts the
                // quiet time now, so the turn ends IDLE_AFTER from here.
                if old == Some(TitleState::Busy) {
                    session.last_work.store(now_ms(), Ordering::Relaxed);
                }
            }
        }
    }

    /// Sets `waiting` and records why. `detail` goes to the log only.
    pub(super) fn wait(&self, session: &Session, cause: &'static str, detail: &str) {
        self.move_state(session, SessionState::Waiting, Some((cause, detail)));
    }

    /// Sets any state but `waiting`, which [`Self::wait`] sets with its cause.
    pub(super) fn set_state(&self, session: &Session, state: SessionState) {
        debug_assert_ne!(state, SessionState::Waiting, "waiting needs a cause");
        self.move_state(session, state, None);
    }

    fn move_state(&self, session: &Session, state: SessionState, cause: Option<(&'static str, &str)>) {
        let (id, dropped) = {
            let mut info = session.info.lock().unwrap();
            if info.state == state || info.state == SessionState::Done {
                return;
            }
            if state == SessionState::Working && info.state == SessionState::Idle {
                session.work_started.store(now_ms(), Ordering::Relaxed);
            }
            info.state = state;
            set_cause(&mut info, session.front(), cause);
            // New work: the result a program left is read or stale.
            let dropped = state == SessionState::Working
                && info.program_status.as_ref().is_some_and(|p| p.ended);
            if dropped {
                info.program_status = None;
            }
            (info.id.clone(), dropped)
        };
        let _ = self.events.send(Event::State { session: id, state });
        if dropped {
            let _ = self.events.send(Event::SessionUpdated {
                session: session.info(),
            });
        }
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
        let front = s.front();
        // The title says the agent works: a quiet tool call is still work.
        if *s.title_state.lock().unwrap() == Some(TitleState::Busy) {
            return;
        }
        // Check and flip under one lock, so a bell or output the reader
        // handles meanwhile is not overwritten with `idle`.
        let (id, updated, turn) = {
            let mut info = s.info.lock().unwrap();
            let last = s.last_work.load(Ordering::Relaxed);
            let quiet = now_ms().saturating_sub(last) > IDLE_AFTER.as_millis() as u64;
            if !quiet || info.state != SessionState::Working || live(&info) {
                return;
            }
            info.state = SessionState::Idle;
            let stretch = last.saturating_sub(s.work_started.load(Ordering::Relaxed));
            let fresh = s.fresh.swap(false, Ordering::Relaxed);
            let busy = s.busy_seen.swap(false, Ordering::Relaxed);
            let turn = match front {
                // The busy line proves a turn, even a fast one, or the first
                // after a resume. Without it, text is a recap or a redraw.
                Front::Agent(agent) if crate::screen::has_busy_line(agent) => busy,
                _ => !fresh && match front {
                    // An agent with no known busy line redraws its status
                    // line while it does nothing.
                    Front::Agent(_) => stretch >= MIN_TURN_MS,
                    Front::Program => true,
                    // A quiet command in a shell has not finished. Its end
                    // shows when the shell is in front again.
                    Front::Shell | Front::Command => false,
                },
            };
            let updated = (turn && !info.unread).then(|| {
                info.unread = true;
                info.clone()
            });
            (info.id.clone(), updated, turn)
        };
        let _ = self.events.send(Event::State { session: id.clone(), state: SessionState::Idle });
        if turn {
            let _ = self.events.send(Event::TurnEnded { session: id });
        }
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
    }
}

/// The causes of `waiting`, as `SessionInfo::waiting_cause` names them.
/// General ones, for any terminal program.
pub(super) const BELL: &str = "bell";
pub(super) const PROMPT: &str = "prompt";
pub(super) const TITLE: &str = "title";
pub(super) const REPORT: &str = "report";

/// Records why the session waits, and logs it, when `info.state` just became
/// `waiting`. Any other state clears the cause.
pub(super) fn set_cause(info: &mut SessionInfo, front: Front, cause: Option<(&'static str, &str)>) {
    match cause.filter(|_| info.state == SessionState::Waiting) {
        Some((cause, detail)) => {
            info.waiting_cause = Some(cause.to_string());
            let program = info.foreground_program.as_deref().unwrap_or("?");
            let sep = if detail.is_empty() { "" } else { ": " };
            tracing::info!("session {} waits ({front:?}, {program}): {cause}{sep}{detail}", info.id);
        }
        None => info.waiting_cause = None,
    }
}

/// The title that asked for an action, for the log.
fn title_detail(session: &Session) -> String {
    session.info.lock().unwrap().title.clone().unwrap_or_default()
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

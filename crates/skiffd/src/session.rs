//! The session pool: one PTY per session, one reader thread per PTY.

use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Result};
use polling::Poller;
use skiff_core::{
    group::Group,
    protocol::{EchoTimes, Event},
    session::{now_ms, SessionId, SessionInfo, SessionSpec, SessionState, WatchFailure},
};
use tokio::sync::{broadcast, Notify};

use crate::{
    program_status::Records,
    pty::Pty,
    screen::{PromptMatch, TitleState},
    workspace::{SavedSession, Workspace},
};

mod attention;
mod away;
mod coordinator;
mod foreground;
mod groups;
mod input;
mod launch;
mod lifecycle;
mod output;
mod peek_owners;
mod persistence;
mod registry;
pub(crate) mod reducer;
pub mod reload;
mod status;
mod wait_log;
#[cfg(test)]
use crate::screen::Screen;
#[cfg(test)]
use attention::topic;
use attention::end_reports;
#[cfg(test)]
use attention::live;
pub use coordinator::{Ask, Coordinated};
use input::{start_writer, Input};
use peek_owners::PeekOwners;
use lifecycle::default_shell;
#[cfg(test)]
use output::prints;
use output::ScreenState;

/// A process's command line, for logs. Empty when it is gone.
pub(crate) fn proc_argv(pid: u32) -> Vec<String> {
    away::proc::argv(pid)
}

/// Output for subscribers, as one piece.
pub type Chunk = Arc<Piece>;

pub struct Piece {
    pub data: Vec<u8>,
    /// When the reader read its first byte.
    pub read: Instant,
    /// Set when it holds the first output read after input.
    pub echo: Option<EchoTimes>,
}

impl std::ops::Deref for Piece {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.data
    }
}

/// No output for this long turns `working` into `idle`.
pub const IDLE_AFTER: Duration = Duration::from_secs(3);

/// A stretch of work shorter than this is no turn: a status line that
/// redraws, a notice. Its end leaves nothing to read.
pub(crate) const MIN_TURN_MS: u64 = 1500;

/// A terminal title only the wrapper shell sets, right before it execs the
/// fallback shell in place of an agent that just exited. Seeing it means the
/// session is a plain shell now, not the agent it was launched as. The exit
/// code of the agent follows a colon: `skiff:shell-handoff:1`.
const SHELL_HANDOFF_TITLE: &str = "skiff:shell-handoff";

/// What runs in front of a session. It selects the rules for the bell and
/// the unread flag, so a shell and an agent each get rules that are true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Front {
    /// The shell at its prompt. It is never working and never rings.
    Shell,
    /// A program the shell runs. Its end is a result to read.
    Command,
    /// A supported agent, by the name the foreground watcher gives it. Its
    /// approval prompt is read from the screen.
    Agent(&'static str),
    /// A program the launch wrapper runs that is no supported agent. The bell
    /// is its only way to call the user.
    Program,
}

/// Output waits this long for more, so a burst of reads goes out as one
/// chunk. A read with nothing after it goes out when this passes.
const QUIET: Duration = Duration::from_micros(500);

/// Output waits at most this long from its first byte, however fast the
/// program streams.
const HOLD: Duration = Duration::from_millis(8);

/// A chunk goes out once it holds this much, before the hold ends. A client
/// parses a chunk in one go, so a large one stalls it.
const MAX_CHUNK: usize = 64 * 1024;

/// Output this soon after input or a resize answers it: an echo or a
/// redraw. It does not mark the session working.
const ECHO_MS: u64 = 250;

/// A client's answer to a query counts as one this long after the query.
/// Without a client nothing answers, and keys typed later are keys.
const REPLY_WINDOW_MS: u64 = 5000;

/// Queries kept unanswered at most.
const MAX_QUERIES: u32 = 64;

/// A program that reports a `blocked` / `auth` block in this time after the
/// session started shows `working`: some sign in by themselves at start
/// (Antigravity). A block that stays longer, and every other kind, shows
/// `waiting` at once.
const AUTH_HOLD_MS: u64 = 2000;

/// A program that says it works yields to its own approval prompt once the
/// prompt has stayed on screen this long after its last report: it may ask
/// without saying so.
const GRACE_MS: u64 = 2000;

/// A program that says it works and prints nothing for this long is marked
/// stalled: a hint that it may hang.
const STALL_MS: u64 = 5 * 60 * 1000;

/// A report that changes only its message, title or progress, or a child's,
/// is sent at most this often. Grok changes its message at every phase.
const STATUS_EVERY_MS: u64 = 1000;

/// The screen text is checked at most this often. Each check walks every cell,
/// and what it feeds (idle detection, the approval prompt) works in seconds.
const SCAN: Duration = Duration::from_millis(100);

pub struct Session {
    info: Mutex<SessionInfo>,
    pty: Pty,
    /// Input for the writer thread. A program that stops reading its input
    /// blocks that thread, never the daemon's executor or other sessions.
    input: std::sync::mpsc::Sender<Input>,
    /// Writes accepted and not yet in the PTY. A reload waits for 0.
    queued: Arc<AtomicUsize>,
    /// Unix µs when the writer thread first wrote input to the PTY since the
    /// last output. The reader takes it with the first output after it; 0 once taken.
    wrote: Arc<AtomicU64>,
    /// Wakes the reader thread out of its wait, so it sees a pause request.
    waker: Poller,
    /// Pauses the reader thread for a reload.
    park: Park,
    /// Raw PTY output. Subscribers that lag lose chunks, never block the reader.
    pub output: broadcast::Sender<Chunk>,
    /// Unix ms of the last PTY read. Atomic so the reader takes no lock for it.
    last_output: AtomicU64,
    /// Unix ms of the last output that was work: visible text, not an echo.
    last_work: AtomicU64,
    /// Unix ms of the last input or resize from a client.
    last_input: AtomicU64,
    /// A key went in and no output has answered it yet. The first read
    /// after it goes out at once; the reads after that merge again.
    key_pending: AtomicBool,
    /// Unix ms when the current stretch of work began.
    work_started: AtomicU64,
    /// No stretch of work has ended yet under the program in front. The first
    /// one is the program drawing itself, not a turn.
    fresh: AtomicBool,
    /// The agent's busy line showed in the current stretch of work: it is a
    /// turn, however short.
    busy_seen: AtomicBool,
    /// What the agent in front last said in its window title.
    title_state: Mutex<Option<TitleState>>,
    /// The PTY's root process is a shell: launched as one, or handed off to.
    shell: AtomicBool,
    front: Mutex<Front>,
    /// The foreground process group `front` was read from. 0 before the first read.
    group: AtomicU32,
    /// `waiting` came from a bell, so input answers it.
    belled: AtomicBool,
    /// What the program in front reported with OSC 7501. Locked before `info`
    /// when both are.
    records: Mutex<Records>,
    /// Queries the program sent and no client answered yet, and when the
    /// last one came (Unix ms). See [`replies`].
    queries: AtomicU32,
    queried_at: AtomicU64,
    /// Unix ms since the agent's approval prompt shows under a working
    /// report; 0 while it does not. See [`GRACE_MS`].
    approval_since: AtomicU64,
    /// That prompt made the session wait, over the report.
    raised: AtomicBool,
    /// What the last scan's screen rule matched. Only the log reads it.
    prompt_match: Mutex<Option<PromptMatch>>,
    /// Unix ms of the last OSC 7501 report this daemon read for the session;
    /// 0: none. Only the log reads it, and a reload does not keep it.
    last_report_at: AtomicU64,
    /// Unix ms when the wait began; 0 when it does not wait, or waited across
    /// a reload. Only the log reads it.
    waiting_since: AtomicU64,
    /// The report is marked stalled, so a read clears it without a lock.
    stalled: AtomicBool,
    /// Unix ms of the last update sent for a report, and a minor change
    /// since then that waits to be sent. See [`STATUS_EVERY_MS`].
    status_sent_at: AtomicU64,
    status_dirty: AtomicBool,
    /// Emulator plus unsent bytes. One lock, so a snapshot and the live
    /// stream never overlap or leave a gap.
    screen: Mutex<ScreenState>,
}

/// The handshake that pauses a reader thread. A reader checks it between
/// reads, never with bytes read and not yet fed to the screen.
#[derive(Default)]
struct Park {
    state: Mutex<ParkState>,
    changed: Condvar,
}

#[derive(Default)]
struct ParkState {
    /// A reload wants the reader paused.
    asked: bool,
    /// The reader is paused.
    parked: bool,
    /// The reader has ended: the child is reaped and its exit recorded.
    done: bool,
}

impl Session {
    /// The program took the terminal: raw input, as editors, pagers and
    /// pickers set, or the alternate screen.
    fn takes_terminal(&self) -> bool {
        self.screen.lock().unwrap().screen.alt() || self.pty.raw_input()
    }

    pub fn info(&self) -> SessionInfo {
        let mut info = self.info.lock().unwrap().clone();
        info.last_output_at = self.last_output.load(Ordering::Relaxed);
        info
    }

    /// A snapshot of the screen, and a receiver for every byte after it.
    pub fn attach(&self) -> (Vec<u8>, broadcast::Receiver<Chunk>) {
        let mut st = self.screen.lock().unwrap();
        st.flush(&self.output);
        (st.screen.snapshot(), self.output.subscribe())
    }

    /// The program in front reports its own state with OSC 7501. Its report
    /// decides the state over the screen rules and the idle timer. A result
    /// kept after the program left decides nothing.
    #[cfg(test)]
    fn claimed(&self) -> bool {
        live(&self.info.lock().unwrap())
    }

    pub(crate) fn front(&self) -> Front {
        *self.front.lock().unwrap()
    }

    /// The process group in front of the PTY. One ioctl.
    pub(crate) fn foreground_group(&self) -> Option<u32> {
        self.pty.foreground_group()
    }

    /// Called by the reader thread between reads. Waits while a reload has
    /// it paused.
    fn park_point(&self) {
        let mut st = self.park.state.lock().unwrap();
        if !st.asked {
            return;
        }
        st.parked = true;
        self.park.changed.notify_all();
        while st.asked {
            st = self.park.changed.wait(st).unwrap();
        }
        st.parked = false;
    }

    /// Asks the reader to pause and waits until it has, or has ended.
    fn pause_reader(&self, until: Instant) -> Result<()> {
        let mut st = self.park.state.lock().unwrap();
        st.asked = true;
        let _ = self.waker.notify();
        while !st.parked && !st.done {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                bail!("the reader of session {} did not pause", self.info.lock().unwrap().id);
            }
            st = self.park.changed.wait_timeout(st, left).unwrap().0;
        }
        Ok(())
    }

    fn resume_reader(&self) {
        self.park.state.lock().unwrap().asked = false;
        self.park.changed.notify_all();
    }

    /// The reader ended: the child is reaped, or the session was adopted done.
    fn reader_done(&self) -> bool {
        self.park.state.lock().unwrap().done
    }

    fn set_reader_done(&self) {
        self.park.state.lock().unwrap().done = true;
        self.park.changed.notify_all();
    }

}

pub struct SessionPool {
    sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
    /// Why the start failed its load: it could not read or parse the workspace
    /// file, or no saved session started. A reload hands
    /// it on. See [`skiff_core::protocol::WorkspaceLoad`].
    load_error: Mutex<Option<String>>,
    /// Killed sessions whose reader has not reaped the child yet.
    dying: Mutex<Vec<Arc<Session>>>,
    /// Children of sessions lost to a reload, not reaped yet. A reload
    /// hands them on with the dying sessions' children.
    orphans: Mutex<Vec<u32>>,
    /// Named layouts, in creation order. Locked before `sessions` when both are.
    groups: Mutex<Vec<Group>>,
    /// State changes, exits, creations, removals. Not output.
    pub events: broadcast::Sender<Event>,
    /// Groups or sessions changed since the last save.
    dirty: Notify,
    /// Set while a reload hands the sessions over. Changes are refused and
    /// the watchers skip their turn.
    reloading: AtomicBool,
    /// Held shared by each change to sessions or groups and each process
    /// spawn. A reload takes it alone once, to wait for those in flight.
    changes: RwLock<()>,
    /// One reload at a time.
    reload: Mutex<()>,
    /// The listening socket's fd, which a reload hands over. -1 when unset.
    listener: AtomicI32,
    /// Held while a coordinator request finds or starts one. Holds the
    /// last session a start or resume line went into, and when.
    coordinator: Mutex<Option<(SessionId, Instant)>>,
    /// The daemon process start, retained through reload handover by its pid.
    started_at: AtomicU64,
    /// `skiff watch` registrations by connection number. Not saved and not
    /// handed over: a watcher registers again when it connects again.
    /// By connection and the session watched: one watcher process may
    /// register for several Skippers, each on its own session.
    watchers: Mutex<HashMap<(u64, SessionId), Watcher>>,
    /// Which app runs have a connection here, for peek sessions.
    peek_owners: Mutex<PeekOwners>,
}

/// One connection's [`Request::Watching`] registration.
///
/// [`Request::Watching`]: skiff_core::protocol::Request::Watching
struct Watcher {
    session: SessionId,
    pid: Option<u32>,
    since: u64,
    failure: Option<WatchFailure>,
    follows: Option<Vec<SessionId>>,
}

impl Default for SessionPool {
    fn default() -> Self {
        let (events, _) = broadcast::channel(4096);
        Self {
            load_error: Mutex::new(None),
            sessions: RwLock::new(HashMap::new()),
            dying: Mutex::new(Vec::new()),
            orphans: Mutex::new(Vec::new()),
            groups: Mutex::new(Vec::new()),
            events,
            dirty: Notify::new(),
            reloading: AtomicBool::new(false),
            changes: RwLock::new(()),
            reload: Mutex::new(()),
            listener: AtomicI32::new(-1),
            coordinator: Mutex::new(None),
            started_at: AtomicU64::new(now_ms()),
            watchers: Mutex::new(HashMap::new()),
            peek_owners: Mutex::new(PeekOwners::new(Instant::now())),
        }
    }
}

/// Changes this close together are saved once.
const SAVE_DELAY: Duration = Duration::from_millis(500);

impl SessionPool {
    /// Records the program in front. A program that ends and leaves the shell
    /// in front is a result to read.
    pub(crate) fn set_front(&self, session: &Session, front: Front) {
        let old = std::mem::replace(&mut *session.front.lock().unwrap(), front);
        if old == front {
            return;
        }
        session.belled.store(false, Ordering::Relaxed);
        *session.title_state.lock().unwrap() = None;
        let agent = matches!(front, Front::Agent(_));
        if agent {
            session.fresh.store(true, Ordering::Relaxed);
            // The screen was last read without this agent's prompt rule.
            // The reader is woken, so it sets its scan deadline.
            if let Ok(mut st) = session.screen.try_lock() {
                st.unscanned = true;
            }
            let _ = session.waker.notify();
        }
        let (state, updated, ended) = {
            let mut records = session.records.lock().unwrap();
            let mut info = session.info.lock().unwrap();
            if info.state == SessionState::Done {
                return;
            }
            let mut state = None;
            let mut changed = false;
            let mut ended = None;
            if front == Front::Shell {
                let effects = attention::step(session, &mut info, reducer::Input::Shell, now_ms(), None);
                state = effects.state.map(|_| info.id.clone());
                changed = effects.update;
                ended = effects.turn.then(|| info.id.clone());
                // The shell's prompt is back: the program's reports end with it.
                changed |= end_reports(&mut records, &mut info);
            } else if agent && info.agent_exit.take().is_some() {
                changed = true;
                // The exit code is saved with the workspace.
                self.dirty.notify_one();
            }
            (state, changed.then(|| info.clone()), ended)
        };
        if let Some(session) = state {
            let _ = self.events.send(Event::State { session, state: SessionState::Idle });
        }
        if let Some(session) = ended {
            let _ = self.events.send(Event::TurnEnded { session });
        }
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
    }

    /// The session's last rows of text, without the agent's input box.
    pub fn tail_text(&self, id: &str, rows: usize) -> Result<Vec<String>> {
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        let agent = match session.front() {
            Front::Agent(agent) => Some(agent),
            _ => None,
        };
        let st = session.screen.lock().unwrap();
        Ok(st.screen.tail_text(agent, rows))
    }

    /// A client's pane for the session has the keys: the user has seen it.
    pub fn seen(&self, id: &str) -> Result<()> {
        let _change = self.change()?;
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        let updated = {
            let mut info = session.info.lock().unwrap();
            let (next, effects) = reducer::reduce(attention::snapshot(&session, &info), reducer::Input::Seen, now_ms());
            *info = next.info;
            if effects.dirty { self.dirty.notify_one(); }
            effects.update.then(|| info.clone())
        };
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
        Ok(())
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<SessionInfo> {
        let _change = self.change()?;
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        let name = name.trim();
        session.info.lock().unwrap().name = (!name.is_empty()).then(|| name.to_string());
        let info = session.info();
        let _ = self.events.send(Event::SessionUpdated {
            session: info.clone(),
        });
        self.dirty.notify_one();
        Ok(info)
    }

    /// Takes a project Skipper over: its mark goes now, and the session ends
    /// at its next idle (see [`Self::retired`]). An idle one ends now.
    pub fn retire(&self, id: &str) -> Result<()> {
        let info = {
            let _change = self.change()?;
            let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
            let mut info = session.info.lock().unwrap();
            if info.skipper.is_none() || info.coordinator {
                bail!("session {id} is not a project Skipper");
            }
            info.skipper = None;
            info.retiring = true;
            info.clone()
        };
        tracing::info!("session {id} retires: a fleet Skipper took it over");
        let _ = self.events.send(Event::SessionUpdated { session: info });
        self.dirty.notify_one();
        self.end_retired();
        Ok(())
    }

    /// Moves a Skipper to a new scope and name: a fleet's rename.
    pub fn rescope(&self, id: &str, scope: &str, name: &str) -> Result<SessionInfo> {
        let _change = self.change()?;
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        {
            let mut info = session.info.lock().unwrap();
            if info.skipper.is_none() || info.coordinator || scope.trim().is_empty() {
                bail!("session {id} is not a project or fleet Skipper");
            }
            info.skipper = Some(scope.to_string());
            info.name = (!name.trim().is_empty()).then(|| name.trim().to_string());
        }
        let info = session.info();
        let _ = self.events.send(Event::SessionUpdated { session: info.clone() });
        self.dirty.notify_one();
        Ok(info)
    }

    /// The retiring sessions that are idle or done. A waiting one keeps its
    /// prompt for the person; a working one finishes its turn.
    fn retired(&self) -> Vec<String> {
        let sessions = self.sessions.read().unwrap();
        sessions
            .values()
            .filter_map(|s| {
                let info = s.info.lock().unwrap();
                (info.retiring && matches!(info.state, SessionState::Idle | SessionState::Done)).then(|| info.id.clone())
            })
            .collect()
    }

    fn end_retired(&self) {
        for id in self.retired() {
            match self.kill(&id) {
                Ok(()) => tracing::info!("session {id} retired: it was idle"),
                Err(e) => tracing::warn!("cannot end retiring session {id}: {e:#}"),
            }
        }
    }

    pub fn set_theme(&self, id: &str, theme: Option<String>) -> Result<SessionInfo> {
        let _change = self.change()?;
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        session.info.lock().unwrap().theme = theme.filter(|t| !t.is_empty());
        let info = session.info();
        let _ = self.events.send(Event::SessionUpdated {
            session: info.clone(),
        });
        self.dirty.notify_one();
        Ok(info)
    }

}

#[cfg(test)]
mod tests;

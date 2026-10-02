//! The session pool: one PTY per session, one reader thread per PTY.

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, ChildKiller, MasterPty, PtySize};
use skiff_core::{
    group::Group,
    protocol::Event,
    session::{now_ms, SessionId, SessionInfo, SessionSpec, SessionState},
};
use tokio::sync::{broadcast, Notify};

use crate::{
    screen::{Screen, Signals},
    workspace::{SavedSession, Workspace},
};

mod away;
mod foreground;
mod groups;
mod persistence;
mod launch;
use groups::prune;

pub type Chunk = Arc<Vec<u8>>;

/// No output for this long turns `working` into `idle`.
pub const IDLE_AFTER: Duration = Duration::from_secs(3);

/// A stretch of work shorter than this is no turn: a status line that
/// redraws, a notice. Its end leaves nothing to read.
const MIN_TURN_MS: u64 = 1500;

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

/// Output is coalesced into at most one chunk per frame.
pub const FRAME: Duration = Duration::from_millis(16);

/// A chunk goes out once it holds this much, before the frame ends. A client
/// parses a chunk in one go, so a large one stalls it.
const MAX_CHUNK: usize = 64 * 1024;

/// Output this soon after input or a resize answers it: an echo or a
/// redraw. It skips coalescing and does not mark the session working.
const ECHO_MS: u64 = 250;

/// The screen text is checked at most this often. Each check walks every cell,
/// and what it feeds (idle detection, the approval prompt) works in seconds.
const SCAN: Duration = Duration::from_millis(100);

pub struct Session {
    info: Mutex<SessionInfo>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// Input for the writer thread. A program that stops reading its input
    /// blocks that thread, never the daemon's executor or other sessions.
    input: std::sync::mpsc::Sender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// Raw PTY output. Subscribers that lag lose chunks, never block the reader.
    pub output: broadcast::Sender<Chunk>,
    /// Unix ms of the last PTY read. Atomic so the reader takes no lock for it.
    last_output: AtomicU64,
    /// Unix ms of the last output that was work: visible text, not an echo.
    last_work: AtomicU64,
    /// Unix ms of the last input or resize from a client.
    last_input: AtomicU64,
    /// Unix ms when the current stretch of work began.
    work_started: AtomicU64,
    /// No stretch of work has ended yet under the program in front. The first
    /// one is the program drawing itself, not a turn.
    fresh: AtomicBool,
    /// The PTY's root process is a shell: launched as one, or handed off to.
    shell: AtomicBool,
    front: Mutex<Front>,
    /// The foreground process group `front` was read from. 0 before the first read.
    group: AtomicU32,
    /// `waiting` came from a bell, so input answers it.
    belled: AtomicBool,
    /// Emulator plus unsent bytes. One lock, so a snapshot and the live
    /// stream never overlap or leave a gap.
    screen: Mutex<ScreenState>,
}

struct ScreenState {
    screen: Screen,
    pending: Vec<u8>,
    last_flush: Instant,
    last_scan: Instant,
    /// Output came since the last scan.
    unscanned: bool,
    /// A chunk since the last scan printed text.
    printed: bool,
    /// A chunk since the last scan printed text that was not an echo.
    printed_work: bool,
}

/// What a scan of the screen found.
#[derive(Default)]
struct Scan {
    /// The scan ran. The fields below say nothing when it did not.
    scanned: bool,
    /// New text on screen that is not an echo.
    work: bool,
    /// The agent in front shows its approval prompt.
    approval: bool,
}

impl ScreenState {
    fn flush(&mut self, output: &broadcast::Sender<Chunk>) {
        self.last_flush = Instant::now();
        if !self.pending.is_empty() {
            let _ = output.send(Arc::new(std::mem::take(&mut self.pending)));
        }
    }

    fn scan(&mut self, front: Front) -> Scan {
        let changed = self.printed && self.screen.text_changed();
        let approval = match front {
            Front::Agent(agent) => self.screen.approval_prompt(agent),
            _ => false,
        };
        let work = changed && self.printed_work;
        self.last_scan = Instant::now();
        self.unscanned = false;
        self.printed = false;
        self.printed_work = false;
        Scan { scanned: true, work, approval }
    }
}

impl Session {
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

    /// True shortly after input: output now is the program answering a key.
    fn echoing(&self) -> bool {
        now_ms().saturating_sub(self.last_input.load(Ordering::Relaxed)) < ECHO_MS
    }

    pub(crate) fn front(&self) -> Front {
        *self.front.lock().unwrap()
    }

    /// The process group in front of the PTY. One ioctl.
    #[cfg(unix)]
    pub(crate) fn foreground_group(&self) -> Option<u32> {
        self.master.lock().unwrap().process_group_leader().map(|pid| pid as u32)
    }

    #[cfg(not(unix))]
    pub(crate) fn foreground_group(&self) -> Option<u32> {
        None
    }

    /// Feeds the emulator and queues the bytes. Scans the screen when the last
    /// scan is older than `SCAN`; the flusher scans what is left. The last
    /// value is true when the chunk answers a key.
    fn push_output(&self, chunk: &[u8]) -> (Signals, Scan, bool) {
        let echoing = self.echoing();
        let front = self.front();
        let mut st = self.screen.lock().unwrap();
        let signals = st.screen.feed(chunk);
        st.unscanned = true;
        if prints(chunk) {
            st.printed = true;
            st.printed_work |= !echoing;
        }
        let scan = if st.last_scan.elapsed() >= SCAN {
            st.scan(front)
        } else {
            Scan::default()
        };
        st.pending.extend_from_slice(chunk);
        // A chunk after a quiet frame goes out now, and so does the answer
        // to a keystroke, even while the program streams output.
        if self.echoing() || st.last_flush.elapsed() >= FRAME || st.pending.len() >= MAX_CHUNK {
            st.flush(&self.output);
        }
        (signals, scan, echoing)
    }

    /// Skips a screen that is locked, so one busy session cannot hold up the
    /// flusher for the rest. The reader flushes it, or the next tick does.
    fn flush_output(&self) {
        let Ok(mut st) = self.screen.try_lock() else { return };
        if !st.pending.is_empty() {
            st.flush(&self.output);
        }
    }

    /// Scans output that came after the last scan, so the end of a burst
    /// counts. A locked screen waits for the next tick.
    fn scan_due(&self) -> Scan {
        let front = self.front();
        let Ok(mut st) = self.screen.try_lock() else { return Scan::default() };
        if st.unscanned && st.last_scan.elapsed() >= SCAN {
            st.scan(front)
        } else {
            Scan::default()
        }
    }
}

pub struct SessionPool {
    sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
    /// Named layouts, in creation order. Locked before `sessions` when both are.
    groups: Mutex<Vec<Group>>,
    /// State changes, exits, creations, removals. Not output.
    pub events: broadcast::Sender<Event>,
    /// Groups or sessions changed since the last save.
    dirty: Notify,
}

impl Default for SessionPool {
    fn default() -> Self {
        let (events, _) = broadcast::channel(4096);
        Self {
            sessions: RwLock::new(HashMap::new()),
            groups: Mutex::new(Vec::new()),
            events,
            dirty: Notify::new(),
        }
    }
}

/// Changes this close together are saved once.
const SAVE_DELAY: Duration = Duration::from_millis(500);

impl SessionPool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .map(|s| s.info())
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.read().unwrap().get(id).cloned()
    }

    pub fn create(self: &Arc<Self>, spec: SessionSpec) -> Result<SessionInfo> {
        self.spawn(spec, None)
    }

    /// Starts a session. `id` is the old one when a restore brings it back.
    fn spawn(self: &Arc<Self>, spec: SessionSpec, id: Option<SessionId>) -> Result<SessionInfo> {
        let command = spec
            .command
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(default_shell);
        let cwd = spec
            .cwd
            .clone()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        // Match the realpaths git reports for worktrees.
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        let label = if spec.label.is_empty() {
            PathBuf::from(&command)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| command.clone())
        } else {
            spec.label.clone()
        };

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("openpty: {e}"))?;

        let (cmd, is_shell) = launch::launch_command(&spec, &command, &cwd);
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("spawn {command} in {}", cwd.display()))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| anyhow!("clone reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| anyhow!("take writer: {e}"))?;
        let killer = child.clone_killer();

        let id: SessionId =
            id.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()[..12].to_string());
        let info = SessionInfo {
            id: id.clone(),
            label,
            role: spec.role,
            cwd,
            command,
            foreground_program: None,
            args: spec.args,
            state: SessionState::Working,
            cols: spec.cols,
            rows: spec.rows,
            pid: child.process_id(),
            exit_code: None,
            theme: None,
            name: spec.name.filter(|name| !name.trim().is_empty()),
            title: None,
            started_at: now_ms(),
            last_output_at: now_ms(),
            away: Vec::new(),
            was: None,
            resume: None,
            unread: false,
            agent_exit: None,
        };

        let (input, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name(format!("pty-in-{id}"))
            .spawn(move || {
                let mut writer = writer;
                // Ends when the session is dropped or the PTY is gone.
                for data in input_rx {
                    if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                        break;
                    }
                }
            })
            .context("spawn writer thread")?;

        let (output, _) = broadcast::channel(4096);
        let session = Arc::new(Session {
            info: Mutex::new(info.clone()),
            master: Mutex::new(pair.master),
            input,
            killer: Mutex::new(killer),
            output,
            last_output: AtomicU64::new(info.last_output_at),
            last_input: AtomicU64::new(0),
            last_work: AtomicU64::new(info.last_output_at),
            work_started: AtomicU64::new(info.last_output_at),
            fresh: AtomicBool::new(true),
            shell: AtomicBool::new(is_shell),
            // The foreground watcher names the agent within a second.
            front: Mutex::new(if is_shell { Front::Shell } else { Front::Program }),
            group: AtomicU32::new(0),
            belled: AtomicBool::new(false),
            screen: Mutex::new(ScreenState {
                screen: Screen::new(spec.cols, spec.rows),
                pending: Vec::new(),
                last_flush: Instant::now() - FRAME,
                last_scan: Instant::now() - SCAN,
                unscanned: false,
                printed: false,
                printed_work: false,
            }),
        });
        self.sessions
            .write()
            .unwrap()
            .insert(id.clone(), session.clone());
        let _ = self.events.send(Event::SessionCreated {
            session: info.clone(),
        });
        self.dirty.notify_one();

        let pool = self.clone();
        let sid = id.clone();
        std::thread::Builder::new()
            .name(format!("pty-{sid}"))
            .spawn(move || {
                let mut reader = reader;
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let chunk = &buf[..n];
                    session.last_output.store(now_ms(), Ordering::Relaxed);
                    let (signals, scan, echo) = session.push_output(chunk);
                    // A bell that answers a key is no call: the user is at the
                    // keys. The BEL that ends a title sequence is not a bell.
                    if signals.bell && !echo {
                        pool.bell(&session);
                    }
                    // A program that starts or ends prints. Read the group in
                    // front now, so the state does not wait for the watcher.
                    if scan.scanned {
                        if let Some(group) = session.foreground_group() {
                            if group != session.group.load(Ordering::Relaxed) {
                                pool.classify(&session, &mut None);
                            }
                        }
                    }
                    pool.apply_scan(&session, scan);
                    if let Some(title) = signals.title {
                        let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
                        let handoff = title.as_deref().and_then(|t| t.strip_prefix(SHELL_HANDOFF_TITLE));
                        if let Some(code) = handoff {
                            // The agent that owned this pane just exited and the
                            // wrapper is about to exec the fallback shell in its
                            // place. Forget the agent's label and command so the
                            // icon falls back to a plain shell instead of the
                            // agent that is no longer running. Its flags go
                            // too: a restore must not pass them to the shell.
                            let shell_name = PathBuf::from(default_shell())
                                .file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_else(default_shell);
                            session.shell.store(true, Ordering::Relaxed);
                            let info = {
                                let mut info = session.info.lock().unwrap();
                                info.title = None;
                                info.label = shell_name.clone();
                                info.command = shell_name;
                                info.args.clear();
                                info.agent_exit = code.strip_prefix(':').and_then(|c| c.parse().ok());
                                info.clone()
                            };
                            let _ = pool.events.send(Event::SessionUpdated { session: info });
                            pool.dirty.notify_one();
                        } else {
                            let changed = {
                                let mut info = session.info.lock().unwrap();
                                let changed = info.title != title;
                                info.title = title.clone();
                                changed
                            };
                            if changed {
                                let _ = pool.events.send(Event::Title {
                                    session: sid.clone(),
                                    title,
                                });
                            }
                        }
                    }
                }
                session.flush_output();
                let code = child.wait().ok().map(|s| s.exit_code() as i32);
                {
                    let mut info = session.info.lock().unwrap();
                    info.exit_code = code;
                    info.state = SessionState::Done;
                }
                let _ = pool.events.send(Event::State {
                    session: sid.clone(),
                    state: SessionState::Done,
                });
                let _ = pool.events.send(Event::Exit { session: sid, code });
            })
            .context("spawn reader thread")?;

        Ok(info)
    }

    /// A bell outside the echo of a key. A shell rings for its own reasons.
    /// A supported agent says what it wants on the screen, and its bell does
    /// not say why it rang. For every other program the bell is the call.
    fn bell(&self, session: &Session) {
        if matches!(session.front(), Front::Command | Front::Program) {
            session.belled.store(true, Ordering::Relaxed);
            self.set_state(session, SessionState::Waiting);
        }
    }

    /// Work is new text on screen. An echo of typing, a redraw after a resize,
    /// mode codes an idle program re-sends, or a logo that only changes color
    /// are not. Text at a shell prompt is not work either.
    fn apply_scan(&self, session: &Session, scan: Scan) {
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
        let waiting = session.info.lock().unwrap().state == SessionState::Waiting;
        if scan.approval {
            session.belled.store(false, Ordering::Relaxed);
            self.set_state(session, SessionState::Waiting);
        } else if waiting && matches!(front, Front::Agent(_)) && !session.belled.load(Ordering::Relaxed) {
            // The prompt left the screen: the user answered it.
            session.last_work.store(now_ms(), Ordering::Relaxed);
            self.set_state(session, SessionState::Working);
        } else if scan.work && !waiting {
            self.set_state(session, SessionState::Working);
        }
    }

    fn set_state(&self, session: &Session, state: SessionState) {
        let id = {
            let mut info = session.info.lock().unwrap();
            if info.state == state || info.state == SessionState::Done {
                return;
            }
            if state == SessionState::Working && info.state == SessionState::Idle {
                session.work_started.store(now_ms(), Ordering::Relaxed);
            }
            info.state = state;
            info.id.clone()
        };
        let _ = self.events.send(Event::State { session: id, state });
    }

    /// Records the program in front. A program that ends and leaves the shell
    /// in front is a result to read.
    pub(crate) fn set_front(&self, session: &Session, front: Front) {
        let old = std::mem::replace(&mut *session.front.lock().unwrap(), front);
        if old == front {
            return;
        }
        session.belled.store(false, Ordering::Relaxed);
        let agent = matches!(front, Front::Agent(_));
        if agent {
            session.fresh.store(true, Ordering::Relaxed);
            // The screen was last read without this agent's prompt rule.
            if let Ok(mut st) = session.screen.try_lock() {
                st.unscanned = true;
            }
        }
        let (state, updated) = {
            let mut info = session.info.lock().unwrap();
            if info.state == SessionState::Done {
                return;
            }
            let mut state = None;
            let mut changed = false;
            if front == Front::Shell {
                if info.state != SessionState::Idle {
                    info.state = SessionState::Idle;
                    state = Some(info.id.clone());
                }
                changed = !info.unread;
                info.unread = true;
            } else if agent && info.agent_exit.take().is_some() {
                changed = true;
                // The exit code is saved with the workspace.
                self.dirty.notify_one();
            }
            (state, changed.then(|| info.clone()))
        };
        if let Some(session) = state {
            let _ = self.events.send(Event::State { session, state: SessionState::Idle });
        }
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
    }

    /// A client's pane for the session has the keys: the user has seen it.
    pub fn seen(&self, id: &str) -> Result<()> {
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        let updated = {
            let mut info = session.info.lock().unwrap();
            let changed = info.unread || info.agent_exit.is_some();
            info.unread = false;
            if info.agent_exit.take().is_some() {
                self.dirty.notify_one();
            }
            changed.then(|| info.clone())
        };
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
        Ok(())
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<()> {
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        // Stored before the send: the reader can see the echo before this
        // thread runs again, and it must count as an echo, not as work.
        session.last_input.store(now_ms(), Ordering::Relaxed);
        session
            .input
            .send(data.to_vec())
            .map_err(|_| anyhow!("session {id} no longer takes input"))?;
        // Input answers a bell. An agent's prompt goes when the screen says so.
        if session.belled.swap(false, Ordering::Relaxed)
            && session.info.lock().unwrap().state == SessionState::Waiting
        {
            self.set_state(&session, SessionState::Working);
        }
        Ok(())
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<SessionInfo> {
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

    pub fn set_theme(&self, id: &str, theme: Option<String>) -> Result<SessionInfo> {
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        session.info.lock().unwrap().theme = theme.filter(|t| !t.is_empty());
        let info = session.info();
        let _ = self.events.send(Event::SessionUpdated {
            session: info.clone(),
        });
        self.dirty.notify_one();
        Ok(info)
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        session
            .master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("resize: {e}"))?;
        session.screen.lock().unwrap().screen.resize(cols, rows);
        // The program redraws for the new size. That is not work either.
        session.last_input.store(now_ms(), Ordering::Relaxed);
        let mut info = session.info.lock().unwrap();
        info.cols = cols;
        info.rows = rows;
        Ok(())
    }

    /// Terminates the process and forgets the session. Drops it from every
    /// group; a group left with no session keeps an empty slot.
    pub fn kill(&self, id: &str) -> Result<()> {
        let mut groups = self.groups.lock().unwrap();
        let session = self
            .sessions
            .write()
            .unwrap()
            .remove(id)
            .ok_or_else(|| anyhow!("no such session: {id}"))?;
        let _ = session.killer.lock().unwrap().kill();
        let cwd = session.info.lock().unwrap().cwd.clone();
        let pruned = prune(&mut groups, id, &cwd);
        drop(groups);
        let _ = self.events.send(Event::SessionRemoved {
            session: id.to_string(),
        });
        if pruned {
            let _ = self.events.send(Event::GroupsChanged {});
        }
        self.dirty.notify_one();
        Ok(())
    }

    /// Sends output held back by coalescing, and scans held-back screen
    /// changes, once per frame.
    pub fn spawn_flusher(self: &Arc<Self>) {
        let pool = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(FRAME);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let sessions: Vec<Arc<Session>> =
                    pool.sessions.read().unwrap().values().cloned().collect();
                for s in sessions {
                    s.flush_output();
                    pool.apply_scan(&s, s.scan_due());
                }
            }
        });
    }

    /// Flips quiet `working` sessions to `idle`. For an agent, the end of a
    /// stretch of work is the end of its turn: a result to read.
    pub fn spawn_idle_watcher(self: &Arc<Self>) {
        let pool = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                let sessions: Vec<Arc<Session>> =
                    pool.sessions.read().unwrap().values().cloned().collect();
                for s in sessions {
                    pool.idle_if_quiet(&s);
                }
            }
        });
    }

    fn idle_if_quiet(&self, s: &Session) {
        let front = s.front();
        // Check and flip under one lock, so a bell or output the reader
        // handles meanwhile is not overwritten with `idle`.
        let (id, updated) = {
            let mut info = s.info.lock().unwrap();
            let last = s.last_work.load(Ordering::Relaxed);
            let quiet = now_ms().saturating_sub(last) > IDLE_AFTER.as_millis() as u64;
            if !quiet || info.state != SessionState::Working {
                return;
            }
            info.state = SessionState::Idle;
            let stretch = last.saturating_sub(s.work_started.load(Ordering::Relaxed));
            let fresh = s.fresh.swap(false, Ordering::Relaxed);
            let turn = !fresh
                && match front {
                    // An agent redraws its status line while it does nothing.
                    Front::Agent(_) => stretch >= MIN_TURN_MS,
                    Front::Program => true,
                    // A quiet command in a shell has not finished. Its end
                    // shows when the shell is in front again.
                    Front::Shell | Front::Command => false,
                };
            let updated = (turn && !info.unread).then(|| {
                info.unread = true;
                info.clone()
            });
            (info.id.clone(), updated)
        };
        let _ = self.events.send(Event::State { session: id, state: SessionState::Idle });
        if let Some(session) = updated {
            let _ = self.events.send(Event::SessionUpdated { session });
        }
    }
}

/// True when the bytes put text on the screen. Escape sequences and control
/// characters alone do not. A sequence split across reads may misjudge one chunk.
fn prints(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x1b => {
                i += 1;
                match bytes.get(i) {
                    // CSI: parameters, then one final byte in 0x40..=0x7e.
                    Some(b'[') => {
                        i += 1;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                    }
                    // OSC, DCS, APC, PM, SOS: up to BEL or ST.
                    Some(b']' | b'P' | b'_' | b'^' | b'X') => {
                        while i < bytes.len() && bytes[i] != 0x07 && !(bytes[i] == b'\\' && bytes[i - 1] == 0x1b) {
                            i += 1;
                        }
                    }
                    // Charset designation: one more byte.
                    Some(b'(' | b')' | b'*' | b'+' | b'#' | b'%') => i += 1,
                    _ => {}
                }
            }
            b if b < 0x20 || b == 0x7f => {}
            _ => return true,
        }
        i += 1;
    }
    false
}

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

#[cfg(test)]
mod tests {
    use super::{prints, Front, Screen, ScreenState, SCAN};
    use std::time::Instant;

    fn state() -> ScreenState {
        ScreenState {
            screen: Screen::new(40, 5),
            pending: Vec::new(),
            last_flush: Instant::now(),
            last_scan: Instant::now() - SCAN,
            unscanned: false,
            printed: false,
            printed_work: false,
        }
    }

    fn print(st: &mut ScreenState, bytes: &[u8], echo: bool) {
        st.screen.feed(bytes);
        st.unscanned = true;
        st.printed = true;
        st.printed_work |= !echo;
    }

    #[test]
    fn scan_counts_new_text_once() {
        let mut st = state();
        print(&mut st, b"hello", false);
        assert!(st.scan(Front::Program).work);
        assert!(!st.unscanned);
        print(&mut st, b"", false);
        assert!(!st.scan(Front::Program).work);
    }

    #[test]
    fn echo_updates_the_baseline_without_work() {
        let mut st = state();
        print(&mut st, b"typed", true);
        assert!(!st.scan(Front::Program).work);
        print(&mut st, b"", false);
        assert!(!st.scan(Front::Program).work);
    }

    #[test]
    fn only_text_counts_as_printing() {
        assert!(!prints(b"\x1b(B\x0f\x1b[?2004h\x1b[?1000h\x1b[?1006h"));
        assert!(!prints(b"\x1b]0;\xe2\x9c\xb3 Claude Code\x07\r\n"));
        assert!(!prints(b"\x1b[?2026h\x1b[?25l\x1b[43;16H\x1b[?25h\x1b[?2026l"));
        assert!(prints(b"\x1b[38;2;215;119;87m*\x1b[39m"));
        assert!(prints("\x1b[3G✻".as_bytes()));
    }
}

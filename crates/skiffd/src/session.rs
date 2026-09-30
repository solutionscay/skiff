//! The session pool: one PTY per session, one reader thread per PTY.

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use skiff_core::{
    group::Group,
    protocol::Event,
    session::{now_ms, SessionId, SessionInfo, SessionSpec, SessionState},
};
use tokio::sync::{broadcast, Notify};

use crate::{
    screen::{Screen, Signals},
    workspace::{self, SavedSession, Workspace},
};

pub type Chunk = Arc<Vec<u8>>;

/// No output for this long turns `working` into `idle`.
pub const IDLE_AFTER: Duration = Duration::from_secs(3);

/// A terminal title only the wrapper shell sets, right before it execs the
/// fallback shell in place of an agent that just exited. Seeing it means the
/// session is a plain shell now, not the agent it was launched as.
const SHELL_HANDOFF_TITLE: &str = "skiff:shell-handoff";

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
    /// Emulator plus unsent bytes. One lock, so a snapshot and the live
    /// stream never overlap or leave a gap.
    screen: Mutex<ScreenState>,
}

struct ScreenState {
    screen: Screen,
    /// Whether the current screen has the approval prompt. Keeping the edge
    /// prevents ordinary output after a confirmation from re-alerting on the
    /// prompt still visible in the terminal.
    codex_approval_visible: bool,
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
    /// New text on screen that is not an echo.
    work: bool,
    /// The Codex approval prompt just appeared.
    approval_prompted: bool,
}

impl ScreenState {
    fn flush(&mut self, output: &broadcast::Sender<Chunk>) {
        self.last_flush = Instant::now();
        if !self.pending.is_empty() {
            let _ = output.send(Arc::new(std::mem::take(&mut self.pending)));
        }
    }

    fn scan(&mut self) -> Scan {
        let changed = self.printed && self.screen.text_changed();
        let approval_visible = self.screen.has_codex_approval_prompt();
        let approval_prompted = approval_visible && !self.codex_approval_visible;
        self.codex_approval_visible = approval_visible;
        let work = changed && self.printed_work;
        self.last_scan = Instant::now();
        self.unscanned = false;
        self.printed = false;
        self.printed_work = false;
        Scan { work, approval_prompted }
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

    /// Feeds the emulator and queues the bytes. Scans the screen when the last
    /// scan is older than `SCAN`; the flusher scans what is left.
    fn push_output(&self, chunk: &[u8]) -> (Signals, Scan) {
        let echoing = self.echoing();
        let mut st = self.screen.lock().unwrap();
        let signals = st.screen.feed(chunk);
        st.unscanned = true;
        if prints(chunk) {
            st.printed = true;
            st.printed_work |= !echoing;
        }
        let scan = if st.last_scan.elapsed() >= SCAN {
            st.scan()
        } else {
            Scan::default()
        };
        st.pending.extend_from_slice(chunk);
        // A chunk after a quiet frame goes out now, and so does the answer
        // to a keystroke, even while the program streams output.
        if self.echoing() || st.last_flush.elapsed() >= FRAME || st.pending.len() >= MAX_CHUNK {
            st.flush(&self.output);
        }
        (signals, scan)
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
        let Ok(mut st) = self.screen.try_lock() else { return Scan::default() };
        if st.unscanned && st.last_scan.elapsed() >= SCAN {
            st.scan()
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

        // An alias from the user's rc files stands for a command line.
        let (program, args) = match skiff_core::alias::expand(&command) {
            Some(w) => (w[0].clone(), [&w[1..], &spec.args[..]].concat()),
            None => (command.clone(), spec.args.clone()),
        };
        // A program that is not the shell hands the terminal to a shell when it
        // exits, so Ctrl+C in an agent leaves a prompt instead of a dead pane.
        // Ctrl+C signals the whole foreground group, the wrapper included; the
        // `trap :` keeps the wrapper alive, and unlike `trap ''` the child does
        // not inherit it, so the agent still gets SIGINT as normal.
        // Keep the daemon's prepared environment. A login shell resets PATH;
        // another Ctrl+C during profile loading can leave agent commands missing.
        let shell = default_shell();
        let is_shell = program == shell
            || std::path::Path::new(&program).file_name() == std::path::Path::new(&shell).file_name();
        let (program, args) = if is_shell {
            (program, args)
        } else {
            let mut wrapped = vec![
                "-c".to_string(),
                format!(
                    "trap : INT; \"$@\"; printf '\\033]0;{SHELL_HANDOFF_TITLE}\\007'; exec \"$SKIFF_SHELL\" -i"
                ),
                "skiff".to_string(),
                program,
            ];
            wrapped.extend(args);
            ("/bin/sh".to_string(), wrapped)
        };
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&args);
        cmd.env("SKIFF_SHELL", &shell);
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
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
            screen: Mutex::new(ScreenState {
                screen: Screen::new(spec.cols, spec.rows),
                codex_approval_visible: false,
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
                    let (signals, scan) = session.push_output(chunk);
                    // A bell means "the process wants you". It stays until the user
                    // types. The BEL that ends a title sequence is not a bell.
                    if signals.bell {
                        pool.set_state(&session, SessionState::Waiting);
                    }
                    pool.apply_scan(&session, scan);
                    if let Some(title) = signals.title {
                        let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
                        if title.as_deref() == Some(SHELL_HANDOFF_TITLE) {
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
                            let info = {
                                let mut info = session.info.lock().unwrap();
                                info.title = None;
                                info.label = shell_name.clone();
                                info.command = shell_name;
                                info.args.clear();
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

    /// Work is new text on screen. An echo of typing, a redraw after a resize,
    /// mode codes an idle program re-sends, or a logo that only changes color
    /// are not.
    fn apply_scan(&self, session: &Session, scan: Scan) {
        if scan.work {
            session.last_work.store(now_ms(), Ordering::Relaxed);
        }
        if scan.approval_prompted {
            self.set_state(session, SessionState::Waiting);
        } else if scan.work && session.info.lock().unwrap().state != SessionState::Waiting {
            self.set_state(session, SessionState::Working);
        }
    }

    fn set_state(&self, session: &Session, state: SessionState) {
        let id = {
            let mut info = session.info.lock().unwrap();
            if info.state == state || info.state == SessionState::Done {
                return;
            }
            info.state = state;
            info.id.clone()
        };
        let _ = self.events.send(Event::State { session: id, state });
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
        // Input answers a waiting prompt.
        if session.info.lock().unwrap().state == SessionState::Waiting {
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

    pub fn list_groups(&self) -> Vec<Group> {
        self.groups.lock().unwrap().clone()
    }

    /// Creates the group when `id` is empty, else replaces it.
    pub fn save_group(&self, mut group: Group) -> Result<Group> {
        let mut groups = self.groups.lock().unwrap();
        let ids = group.layout.sessions();
        {
            let sessions = self.sessions.read().unwrap();
            for (i, s) in ids.iter().enumerate() {
                if ids[..i].contains(s) {
                    bail!("session {s} appears twice in the layout");
                }
                if !skiff_core::group::is_slot(s) && !sessions.contains_key(s) {
                    bail!("no such session: {s}");
                }
            }
        }
        if group.focus.as_ref().is_some_and(|f| !ids.contains(f)) {
            group.focus = None;
        }
        group.layout.clamp_ratios();
        if group.id.is_empty() {
            group.id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            groups.push(group.clone());
        } else {
            let slot = groups
                .iter_mut()
                .find(|g| g.id == group.id)
                .ok_or_else(|| anyhow!("no such group: {}", group.id))?;
            *slot = group.clone();
        }
        drop(groups);
        let _ = self.events.send(Event::GroupsChanged {});
        self.dirty.notify_one();
        Ok(group)
    }

    pub fn delete_group(&self, id: &str) -> Result<()> {
        let mut groups = self.groups.lock().unwrap();
        let before = groups.len();
        groups.retain(|g| g.id != id);
        if groups.len() == before {
            bail!("no such group: {id}");
        }
        drop(groups);
        let _ = self.events.send(Event::GroupsChanged {});
        self.dirty.notify_one();
        Ok(())
    }

    /// Every group and session as they stand. Exited sessions count: their
    /// panes are still in the layout.
    fn workspace(&self) -> Workspace {
        let groups = self.groups.lock().unwrap();
        let sessions = self.sessions.read().unwrap();
        let mut saved: Vec<SavedSession> = sessions
            .values()
            .map(|s| SavedSession::from(&*s.info.lock().unwrap()))
            .collect();
        saved.sort_by(|a, b| a.id.cmp(&b.id));
        Workspace {
            groups: groups.clone(),
            sessions: saved,
        }
    }

    /// Writes the workspace file now. On shutdown, so the last changes count.
    pub fn save_now(&self) {
        if let Err(e) = workspace::save(&workspace::path(), &self.workspace()) {
            tracing::warn!("save workspace: {e:#}");
        }
    }

    /// Writes the workspace file after changes, at most once per `SAVE_DELAY`.
    pub fn spawn_saver(self: &Arc<Self>) {
        let pool = self.clone();
        tokio::spawn(async move {
            loop {
                pool.dirty.notified().await;
                tokio::time::sleep(SAVE_DELAY).await;
                let ws = pool.workspace();
                let saved = tokio::task::spawn_blocking(move || workspace::save(&workspace::path(), &ws)).await;
                if let Ok(Err(e)) = saved {
                    tracing::warn!("save workspace: {e:#}");
                }
            }
        });
    }

    /// Brings back the saved groups, and each saved session as a shell in its
    /// folder under its old id, so the layouts still point at it. A session
    /// that cannot start leaves its groups the way a kill would.
    pub fn restore(self: &Arc<Self>) {
        let file = workspace::path();
        let ws = match workspace::load(&file) {
            Ok(ws) => ws,
            Err(e) => {
                tracing::warn!("restore workspace: {e:#}");
                return;
            }
        };
        let mut groups = ws.groups;
        let mut restored = 0;
        for saved in ws.sessions {
            // A worktree removed meanwhile: start in the home folder instead.
            let cwd = saved.cwd.is_dir().then(|| saved.cwd.clone());
            let spec = SessionSpec {
                label: String::new(),
                name: saved.name.clone(),
                role: saved.role,
                cwd,
                command: None,
                args: Vec::new(),
                cols: saved.cols,
                rows: saved.rows,
                ..Default::default()
            };
            match self.spawn(spec, Some(saved.id.clone())) {
                Ok(_) => {
                    restored += 1;
                    if saved.theme.is_some() {
                        let _ = self.set_theme(&saved.id, saved.theme);
                    }
                }
                Err(e) => {
                    tracing::warn!("restore session {}: {e:#}", saved.id);
                    prune(&mut groups, &saved.id, &saved.cwd);
                }
            }
        }
        // Drop panes whose session was never saved.
        {
            let sessions = self.sessions.read().unwrap();
            for g in groups.iter_mut() {
                for id in g.layout.sessions() {
                    if !skiff_core::group::is_slot(&id) && !sessions.contains_key(&id) {
                        prune(std::slice::from_mut(g), &id, Path::new(""));
                    }
                }
            }
        }
        tracing::info!("restored {} groups and {restored} sessions from {}", groups.len(), file.display());
        *self.groups.lock().unwrap() = groups;
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

    /// Flips quiet `working` sessions to `idle`.
    pub fn spawn_idle_watcher(self: &Arc<Self>) {
        let pool = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                let sessions: Vec<Arc<Session>> =
                    pool.sessions.read().unwrap().values().cloned().collect();
                for s in sessions {
                    // Check and flip under one lock, so a bell or output the
                    // reader handles meanwhile is not overwritten with `idle`.
                    let id = {
                        let mut info = s.info.lock().unwrap();
                        let last = s.last_work.load(Ordering::Relaxed);
                        let quiet = now_ms().saturating_sub(last) > IDLE_AFTER.as_millis() as u64;
                        if !quiet || info.state != SessionState::Working {
                            continue;
                        }
                        info.state = SessionState::Idle;
                        info.id.clone()
                    };
                    let _ = pool.events.send(Event::State {
                        session: id,
                        state: SessionState::Idle,
                    });
                }
            }
        });
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
    use super::{prints, Screen, ScreenState, SCAN};
    use std::time::Instant;

    fn state() -> ScreenState {
        ScreenState {
            screen: Screen::new(40, 5),
            codex_approval_visible: false,
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
        assert!(st.scan().work);
        assert!(!st.unscanned);
        print(&mut st, b"", false);
        assert!(!st.scan().work);
    }

    #[test]
    fn echo_updates_the_baseline_without_work() {
        let mut st = state();
        print(&mut st, b"typed", true);
        assert!(!st.scan().work);
        print(&mut st, b"", false);
        assert!(!st.scan().work);
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

/// Drops `id` from every group. A group left with no session keeps an empty
/// slot, and the session's folder so it stays in that worktree. True when a
/// group changed.
fn prune(groups: &mut [Group], id: &str, cwd: &Path) -> bool {
    let mut pruned = false;
    for g in groups.iter_mut() {
        if !g.layout.sessions().iter().any(|s| s == id) {
            continue;
        }
        pruned = true;
        match g.layout.without(id) {
            Some(layout) => {
                g.layout = layout;
                if g.focus.as_deref() == Some(id) {
                    g.focus = g.layout.sessions().into_iter().next();
                }
            }
            None => {
                if !cwd.as_os_str().is_empty() {
                    g.cwd.get_or_insert_with(|| cwd.to_string_lossy().into_owned());
                }
                g.layout = skiff_core::group::Layout::Pane {
                    session: format!("slot:{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                };
                g.focus = None;
            }
        }
    }
    pruned
}

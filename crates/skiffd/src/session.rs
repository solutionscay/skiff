//! The session pool: one PTY per session, one reader thread per PTY.

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
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
use tokio::sync::broadcast;

use crate::screen::{Screen, Signals};

pub type Chunk = Arc<Vec<u8>>;

/// No output for this long turns `working` into `idle`.
pub const IDLE_AFTER: Duration = Duration::from_secs(3);

/// Output is coalesced into at most one chunk per frame.
pub const FRAME: Duration = Duration::from_millis(16);

/// Output this soon after input or a resize answers it: an echo or a
/// redraw. It skips coalescing and does not mark the session working.
const ECHO_MS: u64 = 250;

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
    pending: Vec<u8>,
    last_flush: Instant,
}

impl ScreenState {
    fn flush(&mut self, output: &broadcast::Sender<Chunk>) {
        self.last_flush = Instant::now();
        if !self.pending.is_empty() {
            let _ = output.send(Arc::new(std::mem::take(&mut self.pending)));
        }
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

    fn push_output(&self, chunk: &[u8]) -> Signals {
        let mut st = self.screen.lock().unwrap();
        let signals = st.screen.feed(chunk);
        st.pending.extend_from_slice(chunk);
        // A chunk after a quiet frame goes out now, and so does the answer
        // to a keystroke, even while the program streams output.
        if self.echoing() || st.last_flush.elapsed() >= FRAME {
            st.flush(&self.output);
        }
        signals
    }

    fn flush_output(&self) {
        let mut st = self.screen.lock().unwrap();
        if !st.pending.is_empty() {
            st.flush(&self.output);
        }
    }
}

pub struct SessionPool {
    sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
    /// Named layouts, in creation order. Locked before `sessions` when both are.
    groups: Mutex<Vec<Group>>,
    /// State changes, exits, creations, removals. Not output.
    pub events: broadcast::Sender<Event>,
}

impl Default for SessionPool {
    fn default() -> Self {
        let (events, _) = broadcast::channel(4096);
        Self {
            sessions: RwLock::new(HashMap::new()),
            groups: Mutex::new(Vec::new()),
            events,
        }
    }
}

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
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&args);
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

        let id: SessionId = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
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
            name: None,
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
                pending: Vec::new(),
                last_flush: Instant::now() - FRAME,
            }),
        });
        self.sessions
            .write()
            .unwrap()
            .insert(id.clone(), session.clone());
        let _ = self.events.send(Event::SessionCreated {
            session: info.clone(),
        });

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
                    let signals = session.push_output(chunk);
                    // A bell means "the process wants you". It stays until the user
                    // types. The BEL that ends a title sequence is not a bell.
                    // Work is visible text. An echo of typing, a redraw after a
                    // resize, or mode codes an idle program re-sends are not.
                    let work = !session.echoing() && prints(chunk);
                    if work {
                        session.last_work.store(now_ms(), Ordering::Relaxed);
                    }
                    if signals.bell {
                        pool.set_state(&session, SessionState::Waiting);
                    } else if work && session.info.lock().unwrap().state != SessionState::Waiting {
                        pool.set_state(&session, SessionState::Working);
                    }
                    if let Some(title) = signals.title {
                        let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
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
        session
            .input
            .send(data.to_vec())
            .map_err(|_| anyhow!("session {id} no longer takes input"))?;
        session.last_input.store(now_ms(), Ordering::Relaxed);
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
        Ok(info)
    }

    pub fn set_theme(&self, id: &str, theme: Option<String>) -> Result<SessionInfo> {
        let session = self.get(id).ok_or_else(|| anyhow!("no such session: {id}"))?;
        session.info.lock().unwrap().theme = theme.filter(|t| !t.is_empty());
        let info = session.info();
        let _ = self.events.send(Event::SessionUpdated {
            session: info.clone(),
        });
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
    /// group; a group with no pane left goes too.
    pub fn kill(&self, id: &str) -> Result<()> {
        let mut groups = self.groups.lock().unwrap();
        let session = self
            .sessions
            .write()
            .unwrap()
            .remove(id)
            .ok_or_else(|| anyhow!("no such session: {id}"))?;
        let _ = session.killer.lock().unwrap().kill();
        let mut pruned = false;
        groups.retain_mut(|g| {
            if !g.layout.sessions().iter().any(|s| s == id) {
                return true;
            }
            pruned = true;
            match g.layout.without(id) {
                Some(layout) => {
                    g.layout = layout;
                    if g.focus.as_deref() == Some(id) {
                        g.focus = g.layout.sessions().into_iter().next();
                    }
                    true
                }
                None => false,
            }
        });
        drop(groups);
        let _ = self.events.send(Event::SessionRemoved {
            session: id.to_string(),
        });
        if pruned {
            let _ = self.events.send(Event::GroupsChanged {});
        }
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
        Ok(())
    }

    /// Sends output held back by coalescing, once per frame.
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
    use super::prints;

    #[test]
    fn only_text_counts_as_printing() {
        assert!(!prints(b"\x1b(B\x0f\x1b[?2004h\x1b[?1000h\x1b[?1006h"));
        assert!(!prints(b"\x1b]0;\xe2\x9c\xb3 Claude Code\x07\r\n"));
        assert!(!prints(b"\x1b[?2026h\x1b[?25l\x1b[43;16H\x1b[?25h\x1b[?2026l"));
        assert!(prints(b"\x1b[38;2;215;119;87m*\x1b[39m"));
        assert!(prints("\x1b[3G✻".as_bytes()));
    }
}

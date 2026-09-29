//! Connects to `skiffd`, sends requests, and fans events out to subscribers.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use anyhow::{anyhow, bail, Context, Result};
use skiff_core::{
    config::FolderInfo,
    group::Group,
    project::{AgentInfo, Project, Worktree},
    protocol::{Envelope, Event, Request, Response, ServerMessage},
    session::{SessionId, SessionInfo, SessionSpec},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{broadcast, mpsc, oneshot},
};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>;
type Outputs = Arc<Mutex<HashMap<SessionId, OutputTx>>>;

/// Output chunks a session's receiver may hold before it counts as lagged.
const OUTPUT_QUEUE: usize = 4096;

struct OutputTx {
    tx: mpsc::Sender<Event>,
    lagged: Arc<AtomicBool>,
}

/// Output and snapshots for one session, from [`Client::output`].
pub struct Output {
    rx: mpsc::Receiver<Event>,
    lagged: Arc<AtomicBool>,
}

impl Output {
    /// The next [`Event::Output`] or [`Event::Snapshot`]. `Lagged` means
    /// chunks were dropped: subscribe again for a fresh snapshot. `Closed`
    /// means the connection is gone or a newer receiver replaced this one.
    pub async fn recv(&mut self) -> Result<Event, broadcast::error::RecvError> {
        if self.lagged.swap(false, Ordering::Relaxed) {
            return Err(broadcast::error::RecvError::Lagged(0));
        }
        self.rx.recv().await.ok_or(broadcast::error::RecvError::Closed)
    }

    /// Drops everything queued. After a lag, before subscribing again, so the
    /// new snapshot has room.
    pub fn clear(&mut self) {
        while self.rx.try_recv().is_ok() {}
        self.lagged.store(false, Ordering::Relaxed);
    }
}

/// Hands output to its session's receiver alone, without a copy per listener.
/// Never waits: a full queue marks the receiver lagged and drops the chunk.
fn route(outputs: &Outputs, e: Event) {
    let session = match &e {
        Event::Output { session, .. } | Event::Snapshot { session, .. } => session.clone(),
        _ => return,
    };
    let mut map = outputs.lock().unwrap();
    let Some(o) = map.get(&session) else { return };
    match o.tx.try_send(e) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => o.lagged.store(true, Ordering::Relaxed),
        Err(mpsc::error::TrySendError::Closed(_)) => {
            map.remove(&session);
        }
    }
}

/// A request with no reply after this long fails instead of hanging.
const REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub struct Client {
    /// The daemon's pid, from the socket's peer credentials.
    pub daemon_pid: Option<i32>,
    /// Unbounded, so a keystroke is queued without a wait, in order.
    tx: mpsc::UnboundedSender<Vec<u8>>,
    next_id: AtomicU64,
    pending: Pending,
    /// Every event except output and snapshots, which go to [`Self::output`].
    events: broadcast::Sender<Event>,
    outputs: Outputs,
    /// Set when the daemon closes the connection.
    closed: Arc<AtomicBool>,
}

impl Client {
    /// Must be called from inside a tokio runtime.
    pub async fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .await
            .with_context(|| format!("connect {}", path.display()))?;
        let daemon_pid = stream.peer_cred().ok().and_then(|c| c.pid());
        let (rd, mut wr) = stream.into_split();

        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if wr.write_all(&line).await.is_err() {
                    break;
                }
            }
        });

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(4096);
        let closed = Arc::new(AtomicBool::new(false));
        let outputs: Outputs = Arc::new(Mutex::new(HashMap::new()));
        {
            let pending = pending.clone();
            let outputs = outputs.clone();
            let events = events.clone();
            let closed = closed.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(rd).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    match serde_json::from_str::<ServerMessage>(&line) {
                        Ok(ServerMessage::Reply(r)) => {
                            if let Some(s) = pending.lock().unwrap().remove(&r.id) {
                                let _ = s.send(r.response);
                            }
                        }
                        Ok(ServerMessage::Event(e @ (Event::Output { .. } | Event::Snapshot { .. }))) => {
                            route(&outputs, e);
                        }
                        Ok(ServerMessage::Event(e)) => {
                            let _ = events.send(e);
                        }
                        Err(e) => tracing::warn!("bad message from skiffd: {e}"),
                    }
                }
                closed.store(true, Ordering::Relaxed);
                // Dropping the senders wakes every waiting request with an error.
                pending.lock().unwrap().clear();
                outputs.lock().unwrap().clear();
            });
        }

        Ok(Self {
            daemon_pid,
            tx,
            next_id: AtomicU64::new(1),
            pending,
            events,
            outputs,
            closed,
        })
    }

    /// True once the daemon has closed the connection. No round trip.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed) || self.tx.is_closed()
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub async fn request(&self, request: Request) -> Result<Response> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (s, r) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, s);
        // After the reader cleared `pending`, nothing would answer. Checked
        // after the insert: the reader sets `closed` before it clears.
        if self.closed.load(Ordering::Relaxed) {
            self.pending.lock().unwrap().remove(&id);
            bail!("skiffd connection closed");
        }
        let mut line = serde_json::to_vec(&Envelope { id, request })?;
        line.push(b'\n');
        if self.tx.send(line).is_err() {
            self.pending.lock().unwrap().remove(&id);
            bail!("skiffd connection closed");
        }
        match tokio::time::timeout(REPLY_TIMEOUT, r).await {
            Ok(reply) => reply.map_err(|_| anyhow!("skiffd connection closed")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                bail!("skiffd did not answer within {}s", REPLY_TIMEOUT.as_secs())
            }
        }
    }

    async fn expect_ok(&self, request: Request) -> Result<()> {
        match self.request(request).await? {
            Response::Ok => Ok(()),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn ping(&self) -> Result<String> {
        Ok(self.hello().await?.0)
    }

    /// Daemon version and protocol number.
    pub async fn hello(&self) -> Result<(String, u32)> {
        match self.request(Request::Ping).await? {
            Response::Pong { version, protocol } => Ok((version, protocol)),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        match self.request(Request::ListSessions).await? {
            Response::Sessions { sessions } => Ok(sessions),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn create_session(&self, spec: SessionSpec) -> Result<SessionInfo> {
        match self.request(Request::CreateSession { spec }).await? {
            Response::Session { session } => Ok(session),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn write(&self, session: &str, data: Vec<u8>) -> Result<()> {
        self.expect_ok(Request::Write {
            session: session.to_string(),
            data,
        })
        .await
    }

    /// Queues input and returns at once. Calls from one thread reach the PTY
    /// in call order. The reply has no waiter, so the reader drops it.
    pub fn write_now(&self, session: &str, data: Vec<u8>) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request::Write {
            session: session.to_string(),
            data,
        };
        let mut line = serde_json::to_vec(&Envelope { id, request })?;
        line.push(b'\n');
        self.tx.send(line).map_err(|_| anyhow!("skiffd connection closed"))
    }

    pub async fn set_session_theme(&self, session: &str, theme: Option<String>) -> Result<SessionInfo> {
        let req = Request::SetSessionTheme {
            session: session.to_string(),
            theme,
        };
        match self.request(req).await? {
            Response::Session { session } => Ok(session),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn rename_session(&self, session: &str, name: &str) -> Result<SessionInfo> {
        let req = Request::RenameSession {
            session: session.to_string(),
            name: name.to_string(),
        };
        match self.request(req).await? {
            Response::Session { session } => Ok(session),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()> {
        self.expect_ok(Request::Resize {
            session: session.to_string(),
            cols,
            rows,
        })
        .await
    }

    /// The receiver for one session's output. Take it before [`Self::subscribe`]:
    /// the snapshot arrives before the reply. Replaces an earlier receiver.
    pub fn output(&self, session: &str) -> Output {
        let (tx, rx) = mpsc::channel(OUTPUT_QUEUE);
        let lagged = Arc::new(AtomicBool::new(false));
        let o = OutputTx { tx, lagged: lagged.clone() };
        self.outputs.lock().unwrap().insert(session.to_string(), o);
        Output { rx, lagged }
    }

    pub async fn subscribe(&self, session: &str) -> Result<()> {
        self.expect_ok(Request::Subscribe {
            session: session.to_string(),
        })
        .await
    }

    pub async fn unsubscribe(&self, session: &str) -> Result<()> {
        self.outputs.lock().unwrap().remove(session);
        self.expect_ok(Request::Unsubscribe {
            session: session.to_string(),
        })
        .await
    }

    pub async fn kill(&self, session: &SessionId) -> Result<()> {
        self.expect_ok(Request::Kill {
            session: session.clone(),
        })
        .await
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        match self.request(Request::ListProjects).await? {
            Response::Projects { projects } => Ok(projects),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn add_worktree(
        &self,
        project: &str,
        branch: &str,
        base: Option<String>,
    ) -> Result<Worktree> {
        let request = Request::AddWorktree {
            project: project.to_string(),
            branch: branch.to_string(),
            base,
        };
        match self.request(request).await? {
            Response::Worktree { worktree } => Ok(worktree),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn keys(&self) -> Result<std::collections::BTreeMap<String, String>> {
        match self.request(Request::GetKeys).await? {
            Response::Keys { keys } => Ok(keys),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn appearance(&self, set: Option<Option<String>>) -> Result<Option<String>> {
        let req = match set {
            Some(theme) => Request::SetAppearance { theme },
            None => Request::GetAppearance,
        };
        match self.request(req).await? {
            Response::Appearance { theme } => Ok(theme),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn list_themes(&self) -> Result<Vec<skiff_core::theme::TerminalTheme>> {
        match self.request(Request::ListThemes).await? {
            Response::Themes { themes } => Ok(themes),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn read_icon(&self, path: PathBuf) -> Result<Option<String>> {
        match self.request(Request::ReadIcon { path }).await? {
            Response::Icon { icon } => Ok(icon),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn set_project_icon(&self, project: String, icon: Option<String>) -> Result<()> {
        self.expect_ok(Request::SetProjectIcon { project, icon }).await
    }

    pub async fn reorder_projects(&self, order: Vec<String>) -> Result<()> {
        self.expect_ok(Request::ReorderProjects { order }).await
    }

    pub async fn set_project_background(&self, project: String, background: Option<String>) -> Result<()> {
        self.expect_ok(Request::SetProjectBackground { project, background }).await
    }

    pub async fn set_project_color(&self, project: String, color: String) -> Result<()> {
        self.expect_ok(Request::SetProjectColor { project, color }).await
    }

    pub async fn inspect_folder(&self, path: PathBuf) -> Result<FolderInfo> {
        match self.request(Request::InspectFolder { path }).await? {
            Response::Folder { folder } => Ok(folder),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::ListAgents).await
    }

    pub async fn set_agents(&self, enabled: Vec<String>) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::SetAgents { enabled }).await
    }

    pub async fn set_agent_command(&self, agent: String, command: String) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::SetAgentCommand { agent, command }).await
    }

    async fn agents_reply(&self, req: Request) -> Result<Vec<AgentInfo>> {
        match self.request(req).await? {
            Response::Agents { agents } => Ok(agents),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn add_project(
        &self,
        path: PathBuf,
        name: Option<String>,
        short: Option<String>,
        color: Option<String>,
        icon: Option<String>,
        agents: Vec<String>,
    ) -> Result<Project> {
        let req = Request::AddProject {
            path,
            name,
            short,
            color,
            icon,
            agents,
        };
        match self.request(req).await? {
            Response::Project { project } => Ok(project),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn remove_worktree(&self, project: &str, path: PathBuf) -> Result<()> {
        self.expect_ok(Request::RemoveWorktree {
            project: project.to_string(),
            path,
        })
        .await
    }

    pub async fn list_groups(&self) -> Result<Vec<Group>> {
        match self.request(Request::ListGroups).await? {
            Response::Groups { groups } => Ok(groups),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn save_group(&self, group: Group) -> Result<Group> {
        match self.request(Request::SaveGroup { group }).await? {
            Response::Group { group } => Ok(group),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn delete_group(&self, id: &str) -> Result<()> {
        self.expect_ok(Request::DeleteGroup {
            group: id.to_string(),
        })
        .await
    }
}

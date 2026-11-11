//! M4 data plane: the selected Host's `skiff-host`, reached directly beside
//! its skiffd (M1f topology). It is a second endpoint of the same Host: its
//! Hello must name the `host_id` the session plane already accepted, or
//! nothing is sent to it. Its `instance_id` is its own, so its continuity is
//! tracked apart from skiffd's: a data-plane restart is NewInstance here and
//! changes nothing on the session plane.
//!
//! This Mac reaches the service's socket beside skiffd's. A remote Host is
//! reached through a forward of its own, to the socket beside that Host's
//! skiffd. This module never starts a service anywhere; the desktop policy
//! starts one only for a local endpoint.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use skiff_host_api::{
    AgentEdit, Error, FolderListing, FolderPlaces, Hello, HostBranches, HostCacheImage, HostCacheUse, HostAgent, HostChanges, HostDiff, HostFile, HostFolder, HostId, HostImage, HostLedgerRow,
    HostFileScope, HostListing, HostProject, HostCronPreview, HostSchedule, HostSkipper, HostSkipperFile, HostWorktree, ProjectEdit, ProjectFleet, ReadPurpose, Request, Response, SkipperEdit, SkipperFolderDone,
    SkipperFolderMode, SkipperFolderPreview, SkipperOpened, InventorySession, TakenSkipper,
};
use tokio::{net::UnixStream, sync::broadcast};

use super::{
    local::HostSelection,
    ssh::{self, SshForward, SshOptions, SshTarget},
    HostConnection,
};

/// How long the data-plane service may take to answer Hello, and every
/// request after it: longer than the Host's longest own deadline, a diff's
/// or a status read's 20 s, so a slow read is the Host's answer, not a
/// timeout here.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum DataRoute {
    Local(PathBuf),
    Ssh {
        target: SshTarget,
        options: SshOptions,
        dir: PathBuf,
    },
}

/// Kept per selection: the last accepted data Hello, for continuity, and
/// the wait after failed remote tries.
#[derive(Default)]
pub(super) struct DataState {
    previous: Option<Hello>,
    failures: u32,
    next: Option<Instant>,
}

/// One accepted data-plane connection, and for SSH the forward it runs over.
/// Dropping it closes both; it never signals the service.
pub struct DataPlane {
    connection: Arc<HostConnection>,
    _forward: Option<SshForward>,
    /// This Mac's service, by the socket's peer credentials. Never set for
    /// a remote Host: the forward's peer is `ssh`.
    pid: Option<u32>,
}

/// A data-plane invalidation. Events are only hints: a gap always requires
/// snapshots, never replay or a guessed delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataEvent {
    Changed { projects: bool, worktrees: Vec<String>, tasks: bool },
    Gap,
}

pub struct DataEvents {
    events: broadcast::Receiver<skiff_host_api::Event>,
    next: Option<u64>,
}
impl DataEvents {
    pub async fn recv(&mut self) -> Result<DataEvent, Error> {
        loop {
            match self.events.recv().await {
                Ok(skiff_host_api::Event::DataChanged { sequence, projects, worktrees, tasks }) => {
                    let gap = self.next.is_some_and(|expected| expected != sequence);
                    self.next = sequence.checked_add(1);
                    if gap { return Ok(DataEvent::Gap) }
                    return Ok(DataEvent::Changed { projects, worktrees, tasks });
                }
                Ok(skiff_host_api::Event::DataGap { sequence }) => {
                    self.next = sequence.checked_add(1);
                    return Ok(DataEvent::Gap);
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => return Ok(DataEvent::Gap),
                Err(broadcast::error::RecvError::Closed) => return Err(Error::TransportLost),
            }
        }
    }
}
impl DataPlane {
    pub fn connection(&self) -> Arc<HostConnection> {
        self.connection.clone()
    }
    /// The local service's pid, for the one build that may replace it.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
    /// Events after this call. A data-plane Host without `data_events@1`
    /// deliberately uses the caller's snapshot/poll fallback.
    pub fn events(&self) -> Result<DataEvents, Error> {
        self.connection.hello().require(&skiff_host_api::Capability::data_events())?;
        Ok(DataEvents { events: self.connection.events()?, next: None })
    }
}

/// Why the data plane could not be reached, in words that name no local path
/// and repeat no ssh output.
#[derive(Debug)]
pub enum DataError {
    /// Nothing listens at the data socket (local), or the remote channel was
    /// refused: no `skiff-host` runs for this Host.
    NotRunning,
    /// A remote try failed recently; the next waits this long.
    Waiting { seconds: u64 },
}
impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => write!(f, "The projects service does not run for this Host."),
            Self::Waiting { seconds } => write!(f, "Skiff cannot reach the projects service. It tries again in {seconds} s."),
        }
    }
}
impl std::error::Error for DataError {}

impl HostSelection {
    /// Connects to this Host's data-plane service. `host` is the id the
    /// session plane accepted: a service that reports another is refused
    /// (`identity_changed`) before any request. Tries are serialized per
    /// selection; nothing is retried here.
    pub async fn connect_data(&self, host: &HostId) -> anyhow::Result<DataPlane> {
        let mut state = self.data_state().lock().await;
        if let Some(at) = state.next.filter(|at| *at > Instant::now()) {
            return Err(DataError::Waiting { seconds: (at - Instant::now()).as_secs() + 1 }.into());
        }
        let result = self.try_data(host, state.previous.as_ref()).await;
        match &result {
            Ok(plane) => {
                state.previous = Some(plane.connection.hello().clone());
                state.failures = 0;
                state.next = None;
            }
            Err(_) if !self.is_local() => {
                state.failures += 1;
                state.next = Some(Instant::now() + ssh::backoff(state.failures));
            }
            Err(_) => {}
        }
        result
    }

    async fn try_data(&self, host: &HostId, previous: Option<&Hello>) -> anyhow::Result<DataPlane> {
        let (stream, forward, pid) = match self.data_route().ok_or(DataError::NotRunning)? {
            DataRoute::Local(path) => {
                let stream = UnixStream::connect(&path).await.map_err(|_| DataError::NotRunning)?;
                let pid = stream.peer_cred().ok().and_then(|c| c.pid()).and_then(|p| u32::try_from(p).ok());
                (stream, None, pid)
            }
            DataRoute::Ssh { target, options, dir } => {
                let mut forward = SshForward::open(&target, &options, &dir).await?;
                let stream = forward.connect().await.map_err(|_| DataError::NotRunning)?;
                (stream, Some(forward), None)
            }
        };
        let connection = match HostConnection::connect_with_timeout(stream, previous, REQUEST_TIMEOUT).await {
            Ok(c) => c,
            // A forward whose remote socket is missing closes the channel at once.
            Err(Error::TransportLost) => return Err(DataError::NotRunning.into()),
            Err(e) => return Err(e.into()),
        };
        let received = &connection.hello().host.host_id;
        if received != host {
            return Err(Error::IdentityChanged { expected: host.clone(), received: received.clone() }.into());
        }
        if forward.is_some() {
            let v = connection.hello().service_versions.get("skiff-host").map_or("", |v| release(v));
            ssh::connect_note(format!("skiff-host {v} answers"));
        }
        Ok(DataPlane { connection: Arc::new(connection), _forward: forward, pid })
    }
}

/// A service version without its build: `0.6.26+1a2b…` is `0.6.26`.
pub fn release(version: &str) -> &str {
    version.split_once('+').map_or(version, |(v, _)| v)
}

/// The build in a service version: `1a2b…` of `0.6.26+1a2b…` and of
/// `0.6.26+1a2b….debug`.
pub fn build(version: &str) -> Option<&str> {
    version.split_once('+').map(|(_, b)| b.strip_suffix(DEBUG).unwrap_or(b))
}

/// The tail of a debug build's service version: `0.6.26+1a2b….debug`.
pub const DEBUG: &str = ".debug";

/// The service version reports a debug build.
pub fn is_debug(version: &str) -> bool {
    version.split_once('+').is_some_and(|(_, b)| b.ends_with(DEBUG))
}

pub use crate::skipper::build_of;

/// The local `skiff-host` that reports `theirs` is not the one the app
/// would start, the program at `bin` of `version`. Two apps can share one
/// skiffd socket, such as the installed app and a dev app; the rule keeps
/// them from replacing each other's service in turn:
/// - a debug app (`debug_app`) replaces any service of another build,
///   whatever its version: a dev build always runs its own;
/// - a release app replaces an older version, and this version from another
///   release build or from one that did not name its build. It never
///   replaces a newer version or a debug build.
///
/// None is stale when `bin` cannot be read, except an older version for a
/// release app.
pub fn stale_service(theirs: &str, version: &str, bin: &Path, debug_app: bool) -> bool {
    let other_build = || build_of(bin).is_some_and(|mine| build(theirs) != Some(mine.as_str()));
    if debug_app {
        return other_build();
    }
    match skiff_core::compare_versions(release(theirs), version) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => !is_debug(theirs) && other_build(),
    }
}

/// Every project of the Host, open and closed, in its config order.
pub async fn project_list(c: &HostConnection) -> Result<Vec<HostProject>, Error> {
    Ok(project_rows(c).await?.0)
}

/// [`project_list`], and the revision a `project_config` change names.
/// `None` from a Host that predates it.
pub async fn project_rows(c: &HostConnection) -> Result<(Vec<HostProject>, Option<String>), Error> {
    match c.request(Request::ProjectList).await? {
        Response::Projects { projects, revision } => Ok((projects, revision)),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One project read again.
pub async fn project_get(c: &HostConnection, name: &str) -> Result<HostProject, Error> {
    match c.request(Request::ProjectGet { name: name.into() }).await? {
        Response::Project { project } => Ok(*project),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The changed files of `worktree`, a path on the Host, and the branch they
/// count from.
pub async fn git_changes(c: &HostConnection, worktree: &str) -> Result<HostChanges, Error> {
    match c.request(Request::GitChanges { worktree: worktree.into() }).await? {
        Response::Changes { changes, base } => Ok(HostChanges { base, changes }),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One changed file's diff text in `worktree`, or the whole worktree's.
/// Both are paths on the Host.
pub async fn git_diff(c: &HostConnection, worktree: &str, file: Option<&str>) -> Result<HostDiff, Error> {
    match c.request(Request::GitDiff { worktree: worktree.into(), file: file.map(Into::into) }).await? {
        Response::Diff { diff } => Ok(diff),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The folder `dir` (relative; empty for the worktree itself) of `worktree`.
/// `follow_links`: This Mac only; a remote Host never gets it.
pub async fn file_list(c: &HostConnection, worktree: &str, dir: &str, follow_links: bool) -> Result<HostListing, Error> {
    file_list_page(c, worktree, dir, follow_links, None).await
}

/// A page after `cursor`, which came from the preceding listing.
pub async fn file_list_page(c: &HostConnection, worktree: &str, dir: &str, follow_links: bool, cursor: Option<usize>) -> Result<HostListing, Error> {
    match c.request(Request::FileList { worktree: worktree.into(), dir: dir.into(), follow_links, cursor }).await? {
        Response::Files { listing } => Ok(listing),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Shows a known worktree folder or a known regular file in this Host's file
/// manager. No Host path is passed to this computer.
pub async fn file_reveal(c: &HostConnection, worktree: &str, file: Option<&str>) -> Result<(), Error> {
    match c.request(Request::FileReveal { worktree: worktree.into(), file: file.map(Into::into) }).await? {
        Response::Ok => Ok(()),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// `file` (relative) of `worktree`, read on the Host for `purpose`.
pub async fn file_read(c: &HostConnection, worktree: &str, file: &str, purpose: ReadPurpose) -> Result<HostFile, Error> {
    match c.request(Request::FileRead { worktree: worktree.into(), file: file.into(), purpose }).await? {
        Response::File { file } => Ok(*file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The project's background image, as bytes the Host checked.
pub async fn project_background(c: &HostConnection, project: &str) -> Result<HostImage, Error> {
    match c.request(Request::ProjectBackground { project: project.into() }).await? {
        Response::Image { image } => Ok(image),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's Images library, newest first.
pub async fn image_list(c: &HostConnection) -> Result<Vec<HostCacheImage>, Error> {
    match c.request(Request::ImageList).await? {
        Response::Images { images } => Ok(images),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One Host cache image's bytes. `path` came from [`image_list`].
pub async fn image_read(c: &HostConnection, path: &str) -> Result<HostImage, Error> {
    image_get(c, path, None).await
}

/// One Host cache image scaled on the Host to at most `size` pixels on its
/// long side. An older Host answers the full image.
pub async fn image_thumb(c: &HostConnection, path: &str, size: u32) -> Result<HostImage, Error> {
    image_get(c, path, Some(size)).await
}

async fn image_get(c: &HostConnection, path: &str, thumb: Option<u32>) -> Result<HostImage, Error> {
    match c.request(Request::ImageRead { path: path.into(), thumb }).await? {
        Response::Image { image } => Ok(image),
        _ => unreachable!("checked by Request::accepts"),
    }
}

pub async fn image_delete(c: &HostConnection, paths: Vec<String>) -> Result<(), Error> {
    match c.request(Request::ImageDelete { paths }).await? {
        Response::Ok => Ok(()),
        _ => unreachable!("checked by Request::accepts"),
    }
}

pub async fn image_pin(c: &HostConnection, paths: Vec<String>, on: bool) -> Result<(), Error> {
    match c.request(Request::ImagePin { paths, on }).await? {
        Response::Ok => Ok(()),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The total cache and image sizes of this Host.
pub async fn cache_use(c: &HostConnection) -> Result<HostCacheUse, Error> {
    match c.request(Request::CacheUse).await? {
        Response::CacheUse { usage } => Ok(usage),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's Ledger panel rows, oldest first (`ledger@1`).
pub async fn ledger_rows(c: &HostConnection) -> Result<Vec<HostLedgerRow>, Error> {
    match c.request(Request::LedgerRows).await? {
        Response::Ledger { rows } => Ok(rows),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A value that changes when the Host's ledger does.
pub async fn ledger_stamp(c: &HostConnection) -> Result<String, Error> {
    match c.request(Request::LedgerStamp).await? {
        Response::LedgerStamp { stamp } => Ok(stamp),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Every schedule with its newest runs (`schedules@2`).
pub async fn schedule_list(c: &HostConnection) -> Result<Vec<HostSchedule>, Error> {
    match c.request(Request::ScheduleList).await? {
        Response::Schedules { schedules } => Ok(schedules),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A cron line checked on the Host: its next runs, or its error line.
pub async fn cron_preview(c: &HostConnection, cron: &str) -> Result<HostCronPreview, Error> {
    match c.request(Request::CronPreview { cron: cron.into() }).await? {
        Response::CronPreview { preview } => Ok(preview),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// `skiff schedule ARGS` on the Host. A refusal is `invalid` with its line.
pub async fn schedule_call(c: &HostConnection, args: Vec<String>) -> Result<(), Error> {
    c.request(Request::ScheduleCall { args }).await.map(|_| ())
}

/// A value that changes with the Host's `projects.toml` (`projects@2`).
pub async fn projects_stamp(c: &HostConnection) -> Result<String, Error> {
    match c.request(Request::ProjectsStamp).await? {
        Response::ProjectsStamp { stamp } => Ok(stamp),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The result of the row `uid`, read on the Host as a bounded preview. The
/// Host reads the path its row records; this side never names one.
pub async fn ledger_result(c: &HostConnection, uid: &str) -> Result<HostFile, Error> {
    match c.request(Request::LedgerResult { uid: uid.into() }).await? {
        Response::File { file } => Ok(*file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's `[skiff_agent]` settings and task types (`skipper@1`).
pub async fn skipper_settings(c: &HostConnection) -> Result<HostSkipper, Error> {
    match c.request(Request::SkipperSettings).await? {
        Response::Skipper { settings } => Ok(*settings),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Skipper's watcher log, chosen and read by the Host as a bounded preview.
/// A client supplies no path and never opens a Host path locally.
pub async fn skipper_watch_log(c: &HostConnection) -> Result<HostFile, Error> {
    match c.request(Request::SkipperWatchLog).await? {
        Response::File { file } => Ok(*file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// An instruction file on the Host and its revision (`skipper_file@1`). A
/// Skipper's missing file is written from the template there first.
pub async fn skipper_file_read(c: &HostConnection, scope: HostFileScope) -> Result<HostSkipperFile, Error> {
    match c.request(Request::SkipperFileRead { scope }).await? {
        Response::SkipperFile { file } => Ok(file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Sends a saved copy back: refused (`conflict`) when the Host's file
/// changed since `revision`. Answers the new revision.
pub async fn skipper_file_write(c: &HostConnection, scope: HostFileScope, text: String, revision: String) -> Result<HostSkipperFile, Error> {
    match c.request(Request::SkipperFileWrite { scope, text, revision }).await? {
        Response::SkipperFile { file } => Ok(file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Reset… on the Host: the template again, the old file in `<name>.bak`.
pub async fn skipper_file_reset(c: &HostConnection, scope: HostFileScope) -> Result<(), Error> {
    c.request(Request::SkipperFileReset { scope }).await.map(drop)
}

/// What a change of Skipper's folder to `path` would do on the Host.
pub async fn skipper_folder_preview(c: &HostConnection, path: &str) -> Result<SkipperFolderPreview, Error> {
    match c.request(Request::SkipperFolderPreview { path: path.into() }).await? {
        Response::SkipperFolder { preview } => Ok(*preview),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's agents and the `[agents]` revision they were read at (`agent_config@1`).
pub async fn agent_config(c: &HostConnection) -> Result<(Vec<HostAgent>, String), Error> {
    agents_of(c.request(Request::AgentConfig).await?)
}

fn agents_of(reply: Response) -> Result<(Vec<HostAgent>, String), Error> {
    match reply {
        Response::AgentConfig { agents, revision } => Ok((agents, revision)),
        _ => unreachable!("checked by Request::accepts"),
    }
}

// Changes. Each is sent once: a lost or late reply is reported, never
// retried, since the Host may have made the change. A project change names
// the revision it was made from, so a stale or repeated one is refused.

/// What the Host would add for `path`, a folder on the Host.
pub async fn project_inspect(c: &HostConnection, path: &str) -> Result<HostFolder, Error> {
    match c.request(Request::ProjectInspect { path: path.into() }).await? {
        Response::Folder { folder } => Ok(folder),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Runs `git init` in `path` on the Host, and inspects it again.
pub async fn project_init(c: &HostConnection, path: &str) -> Result<HostFolder, Error> {
    match c.request(Request::ProjectInit { path: path.into() }).await? {
        Response::Folder { folder } => Ok(folder),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host account's home folder (`folders@1`).
pub async fn folder_home(c: &HostConnection) -> Result<String, Error> {
    match c.request(Request::FolderHome).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The folders in `path` on the Host (`folders@1`).
pub async fn folder_list(c: &HostConnection, path: &str, hidden: bool, filter: Option<String>) -> Result<FolderListing, Error> {
    match c.request(Request::FolderList { path: path.into(), hidden, filter }).await? {
        Response::Folders { listing } => Ok(listing),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The programs in `path` on the Host (`agent_programs@1`).
pub async fn program_list(c: &HostConnection, path: &str) -> Result<Vec<String>, Error> {
    match c.request(Request::ProgramList { path: path.into() }).await? {
        Response::Programs { names } => Ok(names),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Recent and the Host's git repositories under home (`folders@1`).
pub async fn folder_places(c: &HostConnection) -> Result<FolderPlaces, Error> {
    match c.request(Request::FolderPlaces).await? {
        Response::Places { places } => Ok(places),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Makes the folder `name` in `parent` on the Host. Answers its path.
pub async fn folder_create(c: &HostConnection, parent: &str, name: &str) -> Result<String, Error> {
    match c.request(Request::FolderCreate { parent: parent.into(), name: name.into() }).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Puts a dropped or pasted file into the Host's cache. Answers its Host path.
pub async fn drop_put(c: &HostConnection, name: &str, bytes: Vec<u8>) -> Result<String, Error> {
    match c.request(Request::DropPut { name: name.into(), bytes, upload: None, total: None, offset: 0, finish: true }).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The largest file a drop sends to a remote Host: one frame holds it.
pub const DROP_MAX: u64 = 50 * 1024 * 1024;
/// The raw payload of one upload frame. JSON/base64 stays below 8 MiB.
const DROP_CHUNK: usize = 4 * 1024 * 1024;

/// A dropped or pasted file for the Host `on`: no folder, and no larger than
/// [`DROP_MAX`]. Answers its name and bytes, or the message to show. It
/// reads at once: the file is small.
pub async fn drop_file(p: &std::path::Path, on: Option<&str>) -> Result<(String, Vec<u8>), String> {
    let on = on.unwrap_or("the Host");
    let m = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if m.is_dir() {
        return Err(format!("Drop files, not folders, on a pane on {on}."));
    }
    if m.len() > DROP_MAX {
        return Err(format!("{} is larger than 50 MB. Skiff cannot send it to {on}.", p.display()));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((name, bytes))
}

/// [`drop_put`] with the message to show when it fails. `on` names the Host.
pub async fn drop_send(c: &HostConnection, name: &str, bytes: Vec<u8>, on: Option<&str>) -> Result<String, String> {
    drop_send_progress(c, name, bytes, on, |_| {}).await
}

/// [`drop_send`] that calls `sent` with the bytes of each frame the Host
/// took. A file up to [`DROP_CHUNK`] is one frame. Nothing more crosses the
/// link: the count comes from the replies.
pub async fn drop_send_progress(c: &HostConnection, name: &str, bytes: Vec<u8>, on: Option<&str>, mut sent: impl FnMut(usize)) -> Result<String, String> {
    let put = async {
        if bytes.len() <= DROP_CHUNK {
            let n = bytes.len();
            let path = drop_put(c, name, bytes).await?;
            sent(n);
            return Ok(path);
        }
        let upload = uuid::Uuid::new_v4().to_string();
        let total = bytes.len() as u64;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = (offset + DROP_CHUNK).min(bytes.len());
            let finish = end == bytes.len();
            let reply = c.request(Request::DropPut {
                name: name.into(), bytes: bytes[offset..end].to_vec(), upload: Some(upload.clone()), total: Some(total), offset: offset as u64, finish,
            }).await?;
            sent(end - offset);
            if finish {
                return match reply { Response::Path { path } => Ok(path), _ => unreachable!("checked by Request::accepts") };
            }
            offset = end;
        }
        unreachable!("a non-empty upload has a final frame")
    };
    put.await.map_err(|e| match e {
        Error::UnsupportedCapability { .. } => format!(
            "{} cannot take dropped files. Its Skiff is older than this app: use Update Skiff… on that Host in Settings › Hosts.",
            on.unwrap_or("This Host")
        ),
        e => message(e),
    })
}

/// A data-plane error as a person reads it.
pub fn message(e: Error) -> String {
    match e {
        Error::OperationFailed { message } | Error::NotFound { message } | Error::Invalid { message } | Error::Conflict { message } | Error::Permission { message } | Error::MalformedContract { message } => message,
        Error::UnsupportedCapability { capability } => format!("This Host does not offer {}.", capability.name),
        Error::HostBusy => "The projects service is busy. Try again.".into(),
        Error::TransportLost => "Skiff lost the connection to the projects service. It connects again on its own.".into(),
        Error::TimedOut => "The projects service did not answer in time.".into(),
        Error::IncompatibleVersion { .. } => "The projects service is from another Skiff version. Update Skiff… in Settings › Hosts updates it.".into(),
        Error::IdentityChanged { .. } => "The projects service answers as another Skiff Host. Skiff sends it nothing.".into(),
    }
}

/// Adds the repository at `path` on the Host. Answers the row and the new revision.
pub async fn project_add(
    c: &HostConnection,
    revision: &str,
    path: &str,
    name: Option<String>,
    short: Option<String>,
    color: Option<String>,
) -> Result<(HostProject, String), Error> {
    let request = Request::ProjectAdd { path: path.into(), revision: revision.into(), name, short, color };
    match c.request(request).await? {
        Response::ProjectAdded { project, revision } => Ok((*project, revision)),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One key of one project. Answers the new revision.
pub async fn project_edit(c: &HostConnection, revision: &str, project: &str, edit: ProjectEdit) -> Result<String, Error> {
    revised(c.request(Request::ProjectEdit { project: project.into(), revision: revision.into(), edit }).await?)
}

/// The order and the fleets given, in one write. Answers the new revision.
pub async fn project_arrange(
    c: &HostConnection,
    revision: &str,
    order: Vec<String>,
    fleets: Vec<ProjectFleet>,
) -> Result<String, Error> {
    revised(c.request(Request::ProjectArrange { revision: revision.into(), order, fleets }).await?)
}

/// Drops one project's table. Answers the new revision.
pub async fn project_remove(c: &HostConnection, revision: &str, project: &str) -> Result<String, Error> {
    revised(c.request(Request::ProjectRemove { project: project.into(), revision: revision.into() }).await?)
}

fn revised(reply: Response) -> Result<String, Error> {
    match reply {
        Response::Revision { revision } => Ok(revision),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A worktree on the new `branch` of a Host project, or, with `checkout`,
/// on an existing branch that has none.
pub async fn worktree_add(c: &HostConnection, project: &str, branch: &str, base: Option<&str>, checkout: bool) -> Result<HostWorktree, Error> {
    let request = Request::WorktreeAdd { project: project.into(), branch: branch.into(), base: base.map(Into::into), checkout };
    match c.request(request).await? {
        Response::Worktree { worktree } => Ok(worktree),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A Host project's branches, and the one its folder has checked out.
pub async fn branch_list(c: &HostConnection, project: &str) -> Result<HostBranches, Error> {
    match c.request(Request::BranchList { project: project.into() }).await? {
        Response::Branches { branches } => Ok(branches),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Removes a linked worktree, `path` on the Host, as its Host allows.
pub async fn worktree_remove(c: &HostConnection, project: &str, path: &str, force: bool) -> Result<(), Error> {
    c.request(Request::WorktreeRemove { project: project.into(), path: path.into(), force }).await.map(drop)
}

/// One Skipper setting on the Host. Answers the settings, read again.
pub async fn skipper_edit(c: &HostConnection, edit: SkipperEdit) -> Result<HostSkipper, Error> {
    match c.request(Request::SkipperEdit { edit }).await? {
        Response::Skipper { settings } => Ok(*settings),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Changes Skipper's folder on the Host to `path`, a preview's `to`.
pub async fn skipper_folder_change(
    c: &HostConnection,
    path: &str,
    mode: SkipperFolderMode,
    end_skipper: bool,
    anyway: bool,
) -> Result<SkipperFolderDone, Error> {
    match c.request(Request::SkipperFolderChange { path: path.into(), mode, end_skipper, anyway }).await? {
        Response::SkipperFolderDone { done } => Ok(done),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Finds or starts Skipper's session on the Host, and its watcher. `None`:
/// a restore found no session to continue.
pub async fn skipper_open(c: &HostConnection, restore: Option<u64>) -> Result<Option<SkipperOpened>, Error> {
    match c.request(Request::SkipperOpen { restore }).await? {
        Response::SkipperOpened { opened } => Ok(Some(*opened)),
        Response::Ok => Ok(None),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Starts the Host's watcher for Skipper's session `session`. True when this started one.
pub async fn skipper_watch(c: &HostConnection, session: &str) -> Result<bool, Error> {
    match c.request(Request::SkipperWatch { session: session.into() }).await? {
        Response::Watch { started } => Ok(started),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Finds or starts the Host's project Skipper of `project`. Answers its session.
pub async fn skipper_project(c: &HostConnection, project: &str) -> Result<InventorySession, Error> {
    match c.request(Request::SkipperProject { project: project.into() }).await? {
        Response::Session { session } => Ok(*session),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Finds or starts the Host's fleet Skipper of `fleet`, taking the fleet's
/// project Skippers over. Answers its session.
pub async fn skipper_fleet(c: &HostConnection, fleet: &str) -> Result<InventorySession, Error> {
    match c.request(Request::SkipperFleet { fleet: fleet.into() }).await? {
        Response::Session { session } => Ok(*session),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The project Skippers a takeover of `fleet` would stop on the Host.
pub async fn fleet_takeover_preview(c: &HostConnection, fleet: &str, join: Option<&str>) -> Result<Vec<TakenSkipper>, Error> {
    match c.request(Request::FleetTakeoverPreview { fleet: fleet.into(), join: join.map(Into::into) }).await? {
        Response::FleetTakeover { taken } => Ok(taken),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The takeover on the Host: `join` joins `fleet`, whose Skipper takes its
/// project Skipper over. Answers what it took over.
pub async fn fleet_takeover(c: &HostConnection, fleet: &str, join: &str) -> Result<Vec<TakenSkipper>, Error> {
    match c.request(Request::FleetTakeover { fleet: fleet.into(), join: join.into() }).await? {
        Response::FleetTakeover { taken } => Ok(taken),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Renames a fleet on the Host, with its Skipper.
pub async fn fleet_rename(c: &HostConnection, old: &str, new: &str) -> Result<(), Error> {
    c.request(Request::FleetRename { old: old.into(), new: new.into() }).await.map(|_| ())
}

/// One change to the Host's `[agents]`, from `revision`. Answers the agents
/// and the new revision.
pub async fn agent_edit(c: &HostConnection, revision: &str, edit: AgentEdit) -> Result<(Vec<HostAgent>, String), Error> {
    agents_of(c.request(Request::AgentEdit { revision: revision.into(), edit }).await?)
}

#[cfg(test)]
mod tests {
    /// A local skiff-host of this version from another build, or from one
    /// that did not name its build, is stale; a newer one never is.
    #[test]
    fn a_service_of_another_build_is_stale() {
        let bin = std::env::temp_dir().join(format!("skiff-host-build-{}", std::process::id()));
        std::fs::write(&bin, "a build").unwrap();
        let mine = super::build_of(&bin).unwrap();
        let stale = |theirs: &str| super::stale_service(theirs, "0.6.26", &bin, false);
        assert!(stale("0.6.25"));
        assert!(stale(&format!("0.6.25+{mine}")));
        assert!(stale("0.6.26"));
        assert!(stale("0.6.26+0000000000000000"));
        assert!(!stale(&format!("0.6.26+{mine}")));
        assert!(!stale("0.6.27"));
        assert!(!super::stale_service("0.6.26+0000000000000000", "0.6.26", &bin.with_extension("gone"), false));
        // Two apps on one socket: a release app leaves a dev build's service
        // alone, and a dev app replaces any service but its own.
        assert!(!stale("0.6.26+0000000000000000.debug"));
        let dev = |theirs: &str| super::stale_service(theirs, "0.6.26", &bin, true);
        assert!(dev("0.6.26+0000000000000000"));
        assert!(dev("0.6.27+0000000000000000"));
        assert!(dev("0.6.26"));
        assert!(!dev(&format!("0.6.26+{mine}.debug")));
        assert!(!dev(&format!("0.6.26+{mine}")));
        let _ = std::fs::remove_file(&bin);
    }
}

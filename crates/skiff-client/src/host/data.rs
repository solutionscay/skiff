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
    Error, Hello, HostId,
};
use tokio::{net::UnixStream, sync::broadcast};

use super::{
    local::HostSelection,
    ssh::{self, SshForward, SshOptions, SshTarget},
    HostConnection,
};

mod requests;
mod uploads;

pub use requests::{
    project_list, project_rows, project_get, git_changes, git_diff, file_list,
    file_list_page, file_reveal, file_read, project_background, image_list, image_read,
    image_thumb, image_delete, image_pin, cache_use, ledger_rows, ledger_stamp,
    schedule_list, cron_preview, schedule_call, projects_stamp, ledger_result, skipper_settings,
    skipper_watch_log, skipper_file_read, skipper_file_write, skipper_file_reset, skipper_folder_preview, agent_config,
    project_inspect, project_init, folder_home, folder_list, program_list, usage_read, folder_places,
    folder_create, project_add, project_edit, project_arrange, project_remove, worktree_add,
    branch_list, worktree_remove, skipper_edit, skipper_folder_change, skipper_open, skipper_watch,
    skipper_project, skipper_fleet, fleet_takeover_preview, fleet_takeover, fleet_rename, agent_edit,
};
pub use uploads::{
    drop_put, DROP_MAX, drop_file, drop_send, drop_send_progress,
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

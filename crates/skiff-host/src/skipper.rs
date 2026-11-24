//! Skipper and the agents on this Host, through the data plane (M6, #166 R7
//! and R8): settings changes and the folder change (`skipper_config@1`),
//! the instruction files (`skipper_file@1`),
//! Skipper's session and its watcher (`skipper_run@1`), fleet Skippers and
//! their takeover (`skipper_run@2`), and the `[agents]`
//! table (`agent_config@1`). The same `skiff-client` code the app runs on
//! its own desktop does the work, against this Host's config and skiffd.
//!
//! The watcher is this service's to keep: [`supervise`] checks on Skipper's
//! session now and then and starts `skiff watch` when none runs. It replaces
//! a stale watcher (an older version, or another build of this one) once per
//! holder pid, and never a newer one; the lock beside the socket keeps it to
//! one. A service built for an older skiffd than the one that runs starts no
//! watcher.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use skiff_client::{agent_folder, fleet, host::project_agent, skipper, Client};
use skiff_core::{
    config, files,
    session::{SessionInfo, SessionState},
};
use skiff_host_api::{AgentEdit, Error, Request, Response, SkipperOpened};

use crate::{failed, host_file, unread, refused, SESSIONS_TIMEOUT};

/// How often [`supervise`] looks for Skipper's session without a watcher.
const WATCH_EVERY: Duration = Duration::from_secs(15);

/// One request of the three families. The caller checked the capability.
pub async fn answer(request: Request) -> Result<Response, Error> {
    match request {
        Request::SkipperEdit { edit } => {
            let settings = blocking(move || {
                skipper::apply(&edit).map_err(refused)?;
                settings()
            })
            .await?;
            Ok(Response::Skipper { settings: Box::new(settings) })
        }
        // A file by what it is for. The Host makes the path; a name that
        // is not its fleet, project or task type is refused.
        Request::SkipperFileRead { scope } => {
            let file = blocking(move || skipper::host_read(&config::load().map_err(failed)?, &scope).map_err(invalid)).await?;
            Ok(Response::SkipperFile { file })
        }
        Request::SkipperFileWrite { scope, text, revision } => {
            let wrote = blocking(move || skipper::host_write(&config::load().map_err(failed)?, &scope, &text, &revision).map_err(invalid)).await?;
            match wrote {
                skipper::Wrote::Done(file) => Ok(Response::SkipperFile { file }),
                skipper::Wrote::Changed => Err(Error::Conflict { message: "The file changed on this Host since it was read.".into() }),
            }
        }
        Request::SkipperFileReset { scope } => {
            let settings = blocking(move || {
                let cfg = config::load().map_err(failed)?;
                skipper::reset_file(&cfg, &skipper::scope_of(&scope).map_err(invalid)?).map_err(invalid)?;
                settings()
            })
            .await?;
            Ok(Response::Skipper { settings: Box::new(settings) })
        }
        Request::SkipperFolderMove { path } => {
            let home = skipper::home().map_err(failed)?;
            let to = agent_folder::resolve(&path, &home, &home).map_err(invalid)?;
            let c = connect().await?;
            let moved = agent_folder::run(&c, &to).await.map_err(failed)?;
            Ok(Response::SkipperFolderMoved { moved: moved.to_host() })
        }
        Request::SkipperOpen { restore } => {
            let c = connect().await?;
            let how = match restore {
                Some(expect) => skipper::Open::Restore { expect },
                None => skipper::Open::Start,
            };
            let Some(found) = skipper::open(&c, how).await.map_err(failed)? else { return Ok(Response::Ok) };
            let watch_error = ensure_watch(found.session.id.clone(), Vec::new(), None).await.err();
            let session = skiff_client::host::project_session(found.session)?;
            Ok(Response::SkipperOpened {
                opened: Box::new(SkipperOpened { session, created: found.created, typed: found.typed, watch_error }),
            })
        }
        Request::SkipperWatch { session } => {
            let s = coordinator().await?;
            if s.as_ref().map(|s| s.id.as_str()) != Some(session.as_str()) {
                return Err(Error::NotFound { message: "This session is not Skipper's on this Host.".into() });
            }
            let started = ensure_watch(session, Vec::new(), None).await.map_err(|message| Error::OperationFailed { message })?;
            Ok(Response::Watch { started })
        }
        Request::SkipperWatchLog => {
            let file = blocking(watch_log).await?;
            Ok(Response::File { file: Box::new(file) })
        }
        Request::SkipperProject { project } => {
            let id = blocking(move || skipper::start_project(&skiff_core::state::socket(), &cli(), &project).map_err(failed)).await?;
            let c = connect().await?;
            let s = c.list_sessions().await.map_err(failed)?.into_iter().find(|s| s.id == id);
            let s = s.ok_or_else(|| Error::OperationFailed { message: format!("session {id} is gone") })?;
            Ok(Response::Session { session: Box::new(skiff_client::host::project_session(s)?) })
        }
        Request::SkipperFleet { fleet } => {
            let c = connect().await?;
            let daemon = c.daemon_info().await.map_err(failed)?;
            if skipper::too_old_for_fleet(&daemon.version) {
                return Err(Error::OperationFailed { message: format!("skiffd {} on this Host cannot start a fleet Skipper. Update Skiff on this Host first", daemon.version) });
            }
            let id = blocking(move || skipper::start_fleet(&skiff_core::state::socket(), &cli(), &fleet).map_err(failed)).await?;
            let s = c.list_sessions().await.map_err(failed)?.into_iter().find(|s| s.id == id);
            let s = s.ok_or_else(|| Error::OperationFailed { message: format!("session {id} is gone") })?;
            Ok(Response::Session { session: Box::new(skiff_client::host::project_session(s)?) })
        }
        Request::FleetTakeoverPreview { fleet, join } => {
            let c = connect().await?;
            let taken = fleet::preview(&c, &fleet, join.as_deref()).await.map_err(failed)?;
            Ok(Response::FleetTakeover { taken: taken.iter().map(fleet::Taken::to_host).collect() })
        }
        Request::FleetTakeover { fleet, join } => {
            let c = connect().await?;
            let taken = fleet::takeover(&c, &skiff_core::state::socket(), &fleet, Some(&join), true).await.map_err(failed)?;
            Ok(Response::FleetTakeover { taken: taken.iter().map(fleet::Taken::to_host).collect() })
        }
        Request::FleetRename { old, new } => {
            let c = connect().await?;
            fleet::rename(&c, &old, new.trim()).await.map_err(refused)?;
            Ok(Response::Ok)
        }
        Request::AgentConfig => {
            let revision = blocking(|| Ok(config::agents_revision(&config::load().map_err(failed)?.agents))).await?;
            Ok(Response::AgentConfig { agents: agents().await?, revision })
        }
        Request::AgentEdit { revision, edit } => {
            let ((), revision) = blocking(move || {
                config::edit_agents_at(&revision, || match &edit {
                    AgentEdit::Enabled { agents } => config::set_enabled_agents(agents),
                    AgentEdit::Command { agent, command } => config::set_agent_command(agent, command),
                    AgentEdit::Rename { agent, name } => config::rename_agent(agent, name),
                    AgentEdit::Remove { agent } => config::remove_agent(agent),
                    AgentEdit::Restore => config::restore_agents(),
                })
                .map_err(refused)
            })
            .await?;
            Ok(Response::AgentConfig { agents: agents().await?, revision })
        }
        _ => Err(Error::OperationFailed { message: "not a Skipper or agent request".into() }),
    }
}

/// The one watcher log this Host owns, read as a bounded regular file. Its
/// name comes only from this Host's socket; links and other file kinds are
/// refused so a remote path or link target never reaches the client.
fn watch_log() -> Result<skiff_host_api::HostFile, Error> {
    let log = skiff_core::state::socket().with_extension("watch.log");
    match std::fs::symlink_metadata(&log) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(Error::Invalid { message: "Skiff's watcher log is a link on this Host.".into() });
        }
        Ok(meta) if !meta.is_file() => {
            return Err(Error::NotFound { message: "Skiff has no watcher log on this Host.".into() });
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotFound { message: "Skiff has no watcher log on this Host.".into() });
        }
        Err(e) => return Err(failed(e)),
    }
    let dir = log.parent().ok_or_else(|| Error::NotFound { message: "Skiff has no watcher log on this Host.".into() })?;
    let name = log.file_name().and_then(|name| name.to_str()).ok_or_else(|| Error::NotFound { message: "Skiff has no watcher log on this Host.".into() })?;
    let root = skiff_core::git::Root::open(dir).map_err(failed)?;
    let read = files::read_in(&root, dir, name, files::Purpose::Preview).map_err(unread)?;
    if read.content != files::Content::Text {
        return Err(Error::Invalid { message: "Skiff's watcher log is not a text file on this Host.".into() });
    }
    Ok(host_file(read))
}

/// Keeps a current watcher on Skipper's session while this service runs.
/// A stale holder gets one replacement per pid. When another service
/// replaced this one's watcher with one of the same version, this service
/// does not take it back, so two builds of one version do not trade it.
pub async fn supervise() {
    let mut tick = tokio::time::interval(WATCH_EVERY);
    let mut tries = Tries::default();
    let mut mine: Option<u32> = None;
    loop {
        tick.tick().await;
        let Ok(Some(s)) = coordinator().await else { continue };
        let socket = skiff_core::state::socket();
        let known: Vec<u32> = s.watch.as_ref().map(|w| w.pids.clone()).unwrap_or_default();
        let holder = skipper::watch_status(&socket).ok().filter(|l| l.held).map(|l| l.holder);
        let pid = match &holder {
            Some(Some(h)) => {
                if !skipper::stale_holder(h, &cli(), skipper::CLI_VERSION) {
                    if h.version.as_deref() == Some(skipper::CLI_VERSION) {
                        mine = Some(h.pid);
                    }
                    continue;
                }
                if mine.is_some_and(|m| m != h.pid) && h.version.as_deref() == Some(skipper::CLI_VERSION) {
                    tries.flag(h.pid).store(true, Ordering::SeqCst);
                }
                Some(h.pid)
            }
            Some(None) => (known.len() == 1).then(|| known[0]),
            None => None,
        };
        // A held lock whose pid was tried keeps its watcher, and the log is not said again.
        if holder.is_some() && pid.is_none_or(|p| tries.taken(p)) {
            continue;
        }
        let tried = pid.map(|p| tries.flag(p));
        let _ = ensure_watch(s.id, known, tried.clone()).await;
        // A holder it could not prove had its attempt too.
        if let Some(t) = tried {
            t.store(true, Ordering::SeqCst);
        }
    }
}

/// One replacement attempt per watcher pid, for one run of this service.
#[derive(Default)]
struct Tries(HashMap<u32, Arc<AtomicBool>>);

impl Tries {
    /// The attempt flag for `pid`, kept for the run.
    fn flag(&mut self, pid: u32) -> Arc<AtomicBool> {
        self.0.entry(pid).or_default().clone()
    }

    /// True when `pid` had its attempt.
    fn taken(&self, pid: u32) -> bool {
        self.0.get(&pid).is_some_and(|t| t.load(Ordering::SeqCst))
    }
}

/// Why this service starts no watcher: it was built for an older skiffd
/// than `daemon`, so its `skiff` predates the CLI that skiffd pairs with.
fn too_old_for(daemon: &str) -> Option<String> {
    (skiff_core::compare_versions(skiff_core::VERSION, daemon) == std::cmp::Ordering::Less)
        .then(|| format!("This Host's skiff was built for skiffd {}, and skiffd is {daemon}. No watcher started.", skiff_core::VERSION))
}

/// Starts `skiff watch` for `session` unless one runs. With `tried`, a stale
/// holder gets one replacement ([`skipper::ensure_watch`]); `known` holds the
/// watcher pids skiffd registered. True when it started one. A failure is
/// said in this service's log too, once for a service too old for skiffd.
/// The watcher sends no desktop notification: the app that uses this Host
/// sends them.
async fn ensure_watch(session: String, known: Vec<u32>, tried: Option<Arc<AtomicBool>>) -> Result<bool, String> {
    static SAID_OLD: AtomicBool = AtomicBool::new(false);
    let daemon = match connect().await {
        Ok(c) => c.daemon_info().await.ok().map(|i| i.version),
        Err(_) => None,
    };
    if let Some(why) = daemon.as_deref().and_then(too_old_for) {
        if !SAID_OLD.swap(true, Ordering::SeqCst) {
            eprintln!("skiff watch: {why}");
        }
        return Err(why);
    }
    let socket = skiff_core::state::socket();
    let run = tokio::task::spawn_blocking(move || skipper::ensure_watch(&socket, &cli(), skipper::CLI_VERSION, &session, false, &known, tried.as_deref())).await;
    let out = match run {
        Ok(Ok(skipper::WatchOutcome::Replaced { pid })) => {
            eprintln!("skiff watch: replaced the older watcher (pid {pid})");
            Ok(true)
        }
        Ok(Ok(skipper::WatchOutcome::Kept(why))) => {
            eprintln!("skiff watch not updated: {why}");
            Ok(false)
        }
        Ok(Ok(o)) => Ok(matches!(o, skipper::WatchOutcome::Started)),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = &out {
        eprintln!("skiff watch: {e}");
    }
    out
}

/// The `skiff` CLI beside this binary, as a managed install has it, else
/// the one on `PATH`.
fn cli() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|e| e.with_file_name("skiff"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("skiff"))
}

/// Skipper's live session on this Host.
async fn coordinator() -> Result<Option<SessionInfo>, Error> {
    let c = connect().await?;
    let all = tokio::time::timeout(SESSIONS_TIMEOUT, c.list_sessions()).await.map_err(|_| Error::TimedOut)?.map_err(failed)?;
    Ok(all.into_iter().find(|s| s.coordinator && s.state != SessionState::Done))
}

/// The agents as this Host's skiffd lists them: installed from its PATH.
async fn agents() -> Result<Vec<skiff_host_api::HostAgent>, Error> {
    let c = connect().await?;
    let mut list = tokio::time::timeout(SESSIONS_TIMEOUT, c.list_agents()).await.map_err(|_| Error::TimedOut)?.map_err(failed)?;
    // What uses each agent comes from this Host's config, whatever its skiffd build.
    skiff_core::project::mark_used(&mut list);
    let cfg = config::load().map_err(failed)?;
    Ok(list
        .into_iter()
        .map(|agent| {
            let flags = skiff_client::cache::flags_typed(&cfg, &agent);
            let mut agent = project_agent(agent);
            agent.cache_flags = flags;
            agent
        })
        .collect())
}

/// The skiffd this service is paired with.
pub(crate) async fn connect() -> Result<Client, Error> {
    let socket = skiff_core::state::socket();
    match tokio::time::timeout(SESSIONS_TIMEOUT, Client::connect(&socket)).await {
        Ok(Ok(c)) => Ok(c),
        _ => Err(Error::OperationFailed { message: "Skiff cannot reach this Host's sessions service.".into() }),
    }
}

fn settings() -> Result<skiff_host_api::HostSkipper, Error> {
    let cfg = config::load().map_err(failed)?;
    skipper::settings(&cfg).map_err(failed)
}

fn invalid(e: anyhow::Error) -> Error {
    Error::Invalid { message: format!("{e:#}") }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, Error> + Send + 'static) -> Result<T, Error> {
    tokio::task::spawn_blocking(f).await.unwrap_or_else(|e| Err(failed(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A service built for an older skiffd than the one that runs starts no
    /// watcher. One built for this skiffd, or a newer one, may.
    #[test]
    fn starts_no_watcher_for_a_newer_skiffd() {
        assert!(too_old_for("999.0.0").is_some_and(|w| w.contains("No watcher started")));
        assert_eq!(too_old_for(skiff_core::VERSION), None);
        assert_eq!(too_old_for("0.0.1"), None);
    }

    /// One attempt per holder pid: a pid that had its attempt stays taken,
    /// and another pid gets its own.
    #[test]
    fn one_replacement_per_holder_pid() {
        let mut tries = Tries::default();
        assert!(!tries.taken(7));
        assert!(!tries.flag(7).swap(true, Ordering::SeqCst));
        assert!(tries.taken(7));
        assert!(tries.flag(7).swap(true, Ordering::SeqCst));
        assert!(!tries.taken(8));
    }
}

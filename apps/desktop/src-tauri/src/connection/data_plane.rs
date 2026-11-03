//! Selected Host data-plane lifecycle and status helpers.

use std::{sync::{atomic::Ordering, Arc}, time::Duration};
use serde::Serialize;
use skiff_client::host::{HostConnection, data::{DataError, DataPlane}, local::{HostSelection, ProcessControl, SelectedHost}};
use tauri::{ipc::Channel, State};
use super::{err, host_now, App, HELLO_TIMEOUT};
use super::daemon::{check_info, stop_daemon, Check};
use super::process::{data_binary, spawn_daemon, spawn_data_service, DATA_START_WAIT};
use super::remote::connect_remote;


/// One skiff-host per computer, from this app's build: a local service of an
/// older version, or of this version from another build, is stopped with
/// SIGTERM and the bundled one started, once per app run, as the `skiff
/// watch` watcher is (`skipper::stale_holder`). So is one whose reply was
/// unreadable. It holds no session, so nothing is lost. A newer version
/// stays. One that does not stop is used as it is. One that does not come
/// back within 5 s is an error for this read: the next read starts it as for
/// any missing service. Only for a local endpoint (`_local` proves it).
pub(super) async fn replace_older_data_service(app: &App, _local: &ProcessControl, selection: &HostSelection, host: &skiff_host_api::HostId, plane: DataPlane) -> Result<DataPlane, String> {
    let theirs = plane.connection().hello().service_versions.get("skiff-host").cloned().unwrap_or_default();
    let Some(pid) = plane.pid() else { return Ok(plane) };
    let unreadable = app.data_unreadable.load(Ordering::SeqCst);
    if !unreadable && !stale_service(&theirs).await {
        return Ok(plane);
    }
    if app.data_replaced.swap(true, Ordering::SeqCst) {
        return Ok(plane);
    }
    app.data_unreadable.store(false, Ordering::SeqCst);
    eprintln!("skiff-host {theirs} is not this app's build ({}): replacing it", skiff_core::VERSION);
    drop(plane);
    restart_data_service(app, _local, selection, host, pid).await
}

/// Stops this computer's `skiff-host` at `pid` and starts the bundled one.
pub(super) async fn restart_data_service(app: &App, _local: &ProcessControl, selection: &HostSelection, host: &skiff_host_api::HostId, pid: u32) -> Result<DataPlane, String> {
    if !stop_daemon(_local, pid as i32, false).await {
        eprintln!("skiff-host (pid {pid}) did not stop; it keeps running");
        return selection.connect_data(host).await.map_err(|e| data_failure(&e, selection));
    }
    spawn_data_service(_local, selection.socket()).map_err(start_failure)?;
    *app.data_started.lock().unwrap() = Some(std::time::Instant::now());
    let mut last = None;
    for _ in 0..50 {
        match selection.connect_data(host).await {
            Ok(p) => return Ok(p),
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(last.map(|e| data_failure(&e, selection)).unwrap_or_else(|| "The new projects service on this computer did not answer. Skiff checks again on its own.".into()))
}

/// The local `skiff-host` that reports `theirs` is not this app's: an older
/// version, or this version from another build or from a build that did not
/// name its build. A newer version is never replaced, nor is any when this
/// app's `skiff-host` cannot be read.
async fn stale_service(theirs: &str) -> bool {
    let (theirs, bin) = (theirs.to_string(), data_binary());
    tauri::async_runtime::spawn_blocking(move || skiff_client::host::data::stale_service(&theirs, skiff_core::VERSION, &bin)).await.unwrap_or(false)
}

/// The Hello of a data service this app cannot use: another Host API, or
/// one it cannot read.
pub(super) fn unreadable(e: &anyhow::Error) -> bool {
    use skiff_host_api::Error as E;
    matches!(e.downcast_ref(), Some(E::IncompatibleVersion { .. } | E::MalformedContract { .. }))
}

/// The pid of the service at this computer's data socket, by its peer
/// credentials.
pub(super) async fn data_service_pid(selection: &HostSelection) -> Option<u32> {
    let s = tokio::net::UnixStream::connect(skiff_core::socket::data_socket(selection.socket())).await.ok()?;
    s.peer_cred().ok()?.pid().and_then(|p| u32::try_from(p).ok())
}

/// Why this computer's `skiff-host` did not start.
pub(super) fn start_failure(e: anyhow::Error) -> String {
    format!("Skiff cannot start the projects service on this computer ({e:#}). It tries again on its own.")
}

/// One data-plane read. Its reply is used only if the connection it came on
/// is still the selected Host's data connection: a reply from a connection
/// that a Host switch or a reconnect replaced is never shown.
pub(crate) async fn data_read<T, F, Fut>(app: &App, read: F) -> Result<T, String>
where
    F: FnOnce(Arc<HostConnection>) -> Fut,
    Fut: std::future::Future<Output = Result<T, skiff_host_api::Error>>,
{
    let c = data_now(app).await?;
    let result = read(c.clone()).await;
    let current = app.data.lock().await.as_ref().map(|d| d.connection());
    if !current.is_some_and(|d| Arc::ptr_eq(&d, &c)) {
        return Err(DATA_REPLACED.into());
    }
    result.map_err(|e| read_err(app, e))
}

/// A data-plane read error as the page shows it. An unreadable reply from
/// this computer's `skiff-host` marks it for a replace at the next
/// connection, once per run.
fn read_err(app: &App, e: skiff_host_api::Error) -> String {
    let skiff_host_api::Error::MalformedContract { .. } = e else { return data_err(e) };
    match app.selection().ssh_target().map(|t| t.destination.clone()) {
        Some(d) => format!("Skiff cannot read the replies from the projects service on {d}. Update Skiff… in Settings › Hosts updates it."),
        None if app.data_replaced.load(Ordering::SeqCst) => {
            "Skiff cannot read the replies from the projects service on this computer, also after a restart. Skiff checks again on its own.".into()
        }
        None => {
            app.data_unreadable.store(true, Ordering::SeqCst);
            "Skiff cannot read the replies from the projects service on this computer. It starts the service again.".into()
        }
    }
}

/// Keeps the revision of a project read or change from the Host `c` names.
pub(crate) fn keep_revision(app: &App, c: &HostConnection, revision: Option<String>) {
    *app.revision.lock().unwrap() = revision.map(|r| (c.hello().host.host_id.clone(), r));
}

/// One data-plane change, sent once and never again: not after a lost
/// connection or a late reply, whose outcome is then unknown.
pub(crate) async fn data_write<T, F, Fut>(app: &App, write: F) -> Result<T, String>
where
    F: FnOnce(Arc<HostConnection>) -> Fut,
    Fut: std::future::Future<Output = Result<T, skiff_host_api::Error>>,
{
    use skiff_host_api::Error as E;
    let c = data_now(app).await?;
    match write(c).await {
        Ok(out) => Ok(out),
        Err(E::TransportLost | E::TimedOut) => Err(DATA_UNCERTAIN.into()),
        Err(e) => Err(data_err(e)),
    }
}

/// A project change, made from this Host's last project read or change:
/// without one nothing is sent. Its answer's revision is kept for the next
/// change. A conflict forgets it, so the next change waits for the page to
/// read the projects again; the change is not sent again.
pub(crate) async fn project_write<T, F, Fut>(app: &App, write: F) -> Result<T, String>
where
    F: FnOnce(Arc<HostConnection>, String) -> Fut,
    Fut: std::future::Future<Output = Result<(T, String), skiff_host_api::Error>>,
{
    data_write(app, |c| async move {
        let host = c.hello().host.host_id.clone();
        let revision = app.revision.lock().unwrap().clone().filter(|(h, _)| *h == host).map(|(_, r)| r);
        let Some(revision) = revision else {
            return Err(skiff_host_api::Error::Invalid { message: DATA_UNREAD.into() });
        };
        match write(c.clone(), revision).await {
            Ok((out, revision)) => {
                keep_revision(app, &c, Some(revision));
                Ok(out)
            }
            Err(skiff_host_api::Error::Conflict { message }) => {
                keep_revision(app, &c, None);
                Err(skiff_host_api::Error::Conflict { message: format!("{message} Skiff reads them again: try again if it is still needed.") })
            }
            Err(e) => Err(e),
        }
    })
    .await
}

/// The data plane as the page shows it: reachable or why not, and which
/// connection, so the page reads projects and Git again after a new one.
#[derive(Serialize, Debug, PartialEq)]
pub(crate) struct DataPlaneStatus {
    pub(super) ready: bool,
    /// No live session plane: the data plane is not tried.
    pub(super) waiting: bool,
    pub(super) host_id: Option<String>,
    pub(super) instance_id: Option<String>,
    /// Changes with every new data connection.
    pub(super) connection: u64,
    pub(super) capabilities: Vec<String>,
    pub(super) message: Option<String>,
}

pub(super) async fn data_status(app: &App) -> DataPlaneStatus {
    let live = app.live_session().await;
    let mut status = DataPlaneStatus {
        ready: false,
        waiting: live.is_none(),
        host_id: None,
        instance_id: None,
        connection: app.data_gen.load(Ordering::SeqCst),
        capabilities: Vec::new(),
        message: None,
    };
    if live.is_none() {
        return status;
    }
    match data_now(app).await {
        Ok(c) => {
            status.ready = true;
            status.host_id = Some(c.hello().host.host_id.as_str().to_string());
            status.instance_id = Some(c.hello().instance_id.as_str().to_string());
            status.capabilities = capability_names(&c);
        }
        Err(e) => status.message = Some(e),
    }
    status.connection = app.data_gen.load(Ordering::SeqCst);
    status
}

/// Why the data plane is out of reach, in words that name no local path.
pub(super) fn data_failure(e: &anyhow::Error, selection: &HostSelection) -> String {
    let host = selection.ssh_target().map(|t| t.destination.clone());
    let on = host.as_deref().map(|d| format!(" on {d}")).unwrap_or_default();
    if let Some(e) = e.downcast_ref::<skiff_client::host::ssh::SshError>() {
        return e.to_string();
    }
    match (e.downcast_ref::<DataError>(), e.downcast_ref::<skiff_host_api::Error>()) {
        (Some(DataError::NotRunning), _) if host.is_some() => format!(
            "The projects service does not run{on}, so Skiff cannot show its projects. Settings › Hosts starts it."
        ),
        (Some(DataError::NotRunning), _) => {
            "The projects service did not start on this computer, so Skiff cannot show the projects. It starts the service again on its own.".into()
        }
        (Some(DataError::Waiting { seconds }), _) => format!("Skiff cannot reach the projects service{on}. It tries again in {seconds} s."),
        (_, Some(skiff_host_api::Error::IdentityChanged { .. })) => format!(
            "The projects service{on} answers as another Skiff Host than the sessions service. Skiff sends it nothing and checks again on its own."
        ),
        (_, Some(skiff_host_api::Error::IncompatibleVersion { .. })) if host.is_some() => {
            format!("The projects service{on} is from another Skiff version. Update Skiff… in Settings › Hosts updates it.")
        }
        (_, Some(skiff_host_api::Error::IncompatibleVersion { .. } | skiff_host_api::Error::MalformedContract { .. })) if host.is_none() => {
            "The projects service on this computer is from another Skiff build, and Skiff could not replace it. It checks again on its own.".into()
        }
        _ => format!("Skiff cannot reach the projects service{}. It checks again on its own.", if on.is_empty() { " on this computer" } else { &on }),
    }
}

/// A data-plane reply error as the page shows it.
pub(super) fn data_err(e: skiff_host_api::Error) -> String {
    skiff_client::host::data::message(e)
}

pub(super) fn capability_names(c: &HostConnection) -> Vec<String> {
    c.hello().capabilities.iter().map(|c| format!("{}@{}", c.name, c.version)).collect()
}

pub(crate) async fn ensure_host(app: &App) -> Result<(Arc<SelectedHost>, bool), String> {
    let mut guard = app.host.lock().await;
    if let Some(host) = guard.as_ref() {
        if !host.connection().is_closed() {
            return Ok((host.clone(), false));
        }
    }
    *guard = None;
    let selection = app.selection();
    if !selection.is_local() {
        return connect_remote(app, &selection, &mut guard).await;
    }
    let path = selection.socket();
    // Starting a daemon is endpoint-local lifecycle. Without it this only connects.
    let control = selection.process_control();
    let mut spawned = false;
    let mut replaced = false;
    let mut last = String::new();
    for _ in 0..40 {
        let mut c = match selection.connect().await {
            Ok(c) => c,
            Err(e) => {
                last = format!("{e:#}");
                // A reload failed: starting a new daemon is the user's call.
                if let Some(why) = app.failed.lock().unwrap().clone() {
                    return Err(why);
                }
                if !spawned {
                    if let Some(control) = &control {
                        spawn_daemon(control).map_err(|e| format!("start skiffd: {e:#}"))?;
                        spawned = true;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        // Recorded for activation: the Host's identity comes from this pong.
        let info = tokio::time::timeout(HELLO_TIMEOUT, c.daemon_info()).await.ok().and_then(|r| r.ok());
        match check_info(app, c.client(), info, !replaced).await {
            Check::Use => {
                *app.failed.lock().unwrap() = None;
                let host = Arc::new(c.activate().await.map_err(err)?);
                *app.live.lock().unwrap() = Some(host.connection());
                *guard = Some(host.clone());
                return Ok((host, spawned));
            }
            Check::Replaced => {
                replaced = true;
                spawned = false;
            }
            Check::Hung => return Err("skiffd is not responding".into()),
        }
    }
    Err(format!("cannot reach skiffd at {}: {last}", path.display()))
}

/// M4: the selected Host's data-plane connection. Its Host id must be the
/// one the session plane accepted, so data from another Host never shows
/// under this one. This Mac's service is started when none answers (a
/// local endpoint's lifecycle); a remote Host's never is.
pub(crate) async fn data_now(app: &App) -> Result<Arc<HostConnection>, String> {
    let session = host_now(app).await?;
    let host = session.hello().host.host_id.clone();
    let mut guard = app.data.lock().await;
    if let Some(d) = guard.as_ref() {
        let c = d.connection();
        if !c.is_closed() && c.hello().host.host_id == host {
            return Ok(c);
        }
    }
    *guard = None;
    let selection = app.selection();
    let not_running = |e: &anyhow::Error| matches!(e.downcast_ref(), Some(DataError::NotRunning));
    let plane = match selection.connect_data(&host).await {
        Ok(plane) => plane,
        Err(e) if not_running(&e) && selection.process_control().is_some() => {
            let control = selection.process_control().expect("checked");
            // One that stops right after it starts is tried again later,
            // not on every read: its log stays readable meanwhile.
            let too_soon = {
                let mut started = app.data_started.lock().unwrap();
                let soon = started.is_some_and(|at| at.elapsed() < DATA_START_WAIT);
                if !soon {
                    *started = Some(std::time::Instant::now());
                }
                soon
            };
            if too_soon {
                return Err(data_failure(&e, &selection));
            }
            spawn_data_service(&control, selection.socket()).map_err(start_failure)?;
            let mut last = e;
            let mut plane = None;
            for _ in 0..50 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                match selection.connect_data(&host).await {
                    Ok(p) => {
                        plane = Some(p);
                        break;
                    }
                    Err(e) if not_running(&e) => last = e,
                    Err(e) => return Err(data_failure(&e, &selection)),
                }
            }
            plane.ok_or_else(|| data_failure(&last, &selection))?
        }
        // This computer's service from a build this app cannot read: it is
        // replaced as in `replace_older_data_service`, by its socket's peer.
        Err(e) if unreadable(&e) && selection.process_control().is_some() => {
            let control = selection.process_control().expect("checked");
            match data_service_pid(&selection).await {
                Some(pid) if !app.data_replaced.swap(true, Ordering::SeqCst) => {
                    eprintln!("skiff-host (pid {pid}) is from a build this app cannot read: replacing it");
                    restart_data_service(app, &control, &selection, &host, pid).await?
                }
                _ => return Err(data_failure(&e, &selection)),
            }
        }
        Err(e) => return Err(data_failure(&e, &selection)),
    };
    let plane = match selection.process_control() {
        Some(control) => replace_older_data_service(app, &control, &selection, &host, plane).await?,
        None => plane,
    };
    let c = plane.connection();
    *guard = Some(plane);
    app.data_gen.fetch_add(1, Ordering::SeqCst);
    Ok(c)
}

/// Why a data reply is not shown: the connection it came on was replaced.
pub(crate) const DATA_REPLACED: &str = "The connection to the projects service changed during the read. Skiff reads again.";

/// Why a change was not sent: no project read of this Host to make it from.
pub(crate) const DATA_UNREAD: &str = "Skiff has not read this Host's projects yet. It reads them again: try again.";
/// Why a change's outcome is unknown: its reply was lost or late.
pub(crate) const DATA_UNCERTAIN: &str =
    "The Host did not answer in time, so the change may or may not have been made. Skiff reads the projects again: check them before you try again.";

/// Checks the data plane over the live session plane only: it never connects
/// or starts skiffd, and never touches a terminal stream. On This Mac a
/// missing `skiff-host` starts, as for any data read.
#[tauri::command]
pub(crate) async fn data_plane_status(app: State<'_, App>) -> Result<DataPlaneStatus, String> {
    Ok(data_status(&app).await)
}

#[derive(Serialize, Clone)]
pub(crate) struct DataEvent { pub projects: bool, pub worktrees: Vec<String>, pub tasks: bool, pub gap: bool }

#[tauri::command]
pub(crate) async fn subscribe_data_events(app: State<'_, App>, on_event: Channel<DataEvent>) -> Result<(), String> {
    let c = data_now(&app).await?;
    c.hello().require(&skiff_host_api::Capability::data_events()).map_err(data_err)?;
    let mut events = c.events().map_err(data_err)?;
    let task = tauri::async_runtime::spawn(async move {
        loop { match events.recv().await {
            Ok(skiff_host_api::Event::DataChanged { projects, worktrees, tasks, .. }) => if on_event.send(DataEvent { projects, worktrees, tasks, gap: false }).is_err() { return },
            Ok(skiff_host_api::Event::DataGap { .. }) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => if on_event.send(DataEvent { projects: true, worktrees: vec![], tasks: true, gap: true }).is_err() { return },
            Ok(_) => {}, Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }}
    });
    if let Some(old) = app.data_events.lock().await.replace(task) { old.abort(); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// M4: this Mac's data plane is started for the selected socket when none
    /// answers, paired with the session plane's Host id, and refused when it
    /// names another Host. Private config only; nothing live is read.
    #[test]
    fn the_data_plane_starts_here_and_pairs_with_the_session_host() {
        use super::*;
        use std::path::PathBuf;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let _env = crate::reload::tests::ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = PathBuf::from("/tmp").join(format!("sd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let old = (std::env::var_os("SKIFF_HOST_BIN"), std::env::var_os("SKIFF_CONFIG"));
        let launcher = dir.join("skiff-host");
        std::fs::write(&launcher, format!("#!/bin/sh\necho \"$@\" >> {}/started\n", dir.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("SKIFF_HOST_BIN", &launcher);
        std::env::set_var("SKIFF_CONFIG", dir.join("projects.toml"));
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let session = dir.join("s.sock");
            let listener = tokio::net::UnixListener::bind(&session).unwrap();
            // A skiffd that reports Host A on every connection.
            tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    tokio::spawn(async move {
                        let mut stream = BufReader::new(stream);
                        let mut line = String::new();
                        while stream.read_line(&mut line).await.unwrap_or(0) > 0 {
                            let id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"].clone();
                            let reply = serde_json::json!({"id":id,"type":"pong","version":skiff_core::VERSION,"protocol":skiff_core::PROTOCOL,
                                "identity":{"host_id":"A","instance_id":"run","os":"test","account":"uid:1"}});
                            let mut bytes = serde_json::to_vec(&reply).unwrap(); bytes.push(b'\n');
                            stream.get_mut().write_all(&bytes).await.unwrap();
                            line.clear();
                        }
                    });
                }
            });
            let app = App::default();
            app.set_selection(HostSelection::new(session.clone(), dir.join("bootstrap-id")));
            // The launcher's build is the bundled service's: the service is current.
            let version = format!("{}+{}", skiff_core::VERSION, skiff_client::host::data::build_of(&launcher).unwrap());
            let host = |id: &str| {
                let mut h = skiff_host::hello().unwrap();
                h.host.host_id = skiff_host_api::HostId::try_from(id.to_string()).unwrap();
                h.service_versions.insert("skiff-host".into(), version.clone());
                h
            };
            // The "started" service binds once the launcher has run.
            let data_socket = skiff_core::socket::data_socket(&session);
            let (marks, socket, hello_a) = (dir.join("started"), data_socket.clone(), host("A"));
            let service = tokio::spawn(async move {
                while !marks.exists() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let listener = tokio::net::UnixListener::bind(&socket).unwrap();
                let _ = skiff_host::serve(listener, Arc::new(hello_a)).await;
            });
            let c = data_now(&app).await.unwrap();
            assert_eq!(c.hello().host.host_id.as_str(), "A");
            let started = std::fs::read_to_string(dir.join("started")).unwrap();
            assert_eq!(started.trim(), format!("--session-socket {} --this-mac", session.display()));
            assert!(skiff_client::host::data::project_list(&c).await.unwrap().is_empty());
            // Reused while open: no second start.
            assert!(Arc::ptr_eq(&c, &data_now(&app).await.unwrap()));
            assert_eq!(std::fs::read_to_string(dir.join("started")).unwrap().lines().count(), 1);

            // The page's check: ready, with the data plane's own families.
            let first = data_status(&app).await;
            assert!(first.ready && !first.waiting && first.host_id.as_deref() == Some("A"), "{first:?}");
            assert!(first.capabilities.contains(&"git_diff@1".to_string()), "{first:?}");
            // A reply that comes on a connection replaced meanwhile is never shown.
            let app = &app;
            let stale = data_read(app, |c| async move {
                *app.data.lock().await = None;
                skiff_client::host::data::project_list(&c).await
            })
            .await;
            assert_eq!(stale, Err(DATA_REPLACED.to_string()));
            let again = data_status(app).await;
            assert!(again.ready && again.connection > first.connection && again.instance_id == first.instance_id, "{again:?}");

            // The service restarts: the next check finds the new instance on
            // a new connection, with no page reload and nothing else redone.
            service.abort();
            let _ = service.await;
            let _ = std::fs::remove_file(&data_socket);
            let listener = tokio::net::UnixListener::bind(&data_socket).unwrap();
            let restarted = tokio::spawn(skiff_host::serve(listener, Arc::new(host("A"))));
            let old_c = app.data.lock().await.as_ref().unwrap().connection();
            for _ in 0..100 {
                if old_c.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let back = data_status(app).await;
            assert!(back.ready && back.connection > again.connection && back.instance_id != again.instance_id, "{back:?}");
            assert_eq!(std::fs::read_to_string(dir.join("started")).unwrap().lines().count(), 1, "a running service was started again");
            let service = restarted;

            // Another Host's service at this socket: refused, nothing sent.
            service.abort();
            let _ = service.await;
            *app.data.lock().await = None;
            let _ = std::fs::remove_file(&data_socket);
            let listener = tokio::net::UnixListener::bind(&data_socket).unwrap();
            let other = tokio::spawn(skiff_host::serve(listener, Arc::new(host("B"))));
            let e = data_now(app).await.err().unwrap();
            assert!(e.contains("answers as another Skiff Host"), "{e}");
            other.abort();
        });
        for (key, value) in [("SKIFF_HOST_BIN", old.0), ("SKIFF_CONFIG", old.1)] {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

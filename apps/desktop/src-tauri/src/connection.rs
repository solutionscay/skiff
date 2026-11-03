use std::{collections::HashMap, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::Duration};
use serde::Serialize;
use skiff_client::{Client, host::{HostConnection, data::{DataError, DataPlane}, local::{HostSelection, ProcessControl, SelectedHost}}};
use skiff_core::socket::socket_path;
use tauri::{async_runtime::JoinHandle, State};
use tokio::sync::Mutex;
use crate::{input::Input, reload::Plan, streaming::Flow};

mod daemon;
mod data_plane;
mod process;
mod remote;

use daemon::*;
use data_plane::*;
use process::*;
use remote::*;
pub(crate) use data_plane::{data_read, data_write, ensure_host, keep_revision, project_write, DataPlaneStatus};
pub(crate) use daemon::{check_daemon, compare_versions, restart_warning, status_now};
pub(crate) use process::{bundled, daemon_binary};

#[derive(Default)]
pub(crate) struct App {
    pub(crate) host: Mutex<Option<Arc<SelectedHost>>>,
    /// M4: the selected Host's data-plane service (`skiff-host`), checked
    /// against the session plane's Host id on every connection.
    pub(crate) data: Mutex<Option<DataPlane>>,
    /// This run replaced a local `skiff-host` of another build already:
    /// once per run.
    pub(crate) data_replaced: AtomicBool,
    /// A reply from the local `skiff-host` was unreadable: the next
    /// connection replaces it, whatever its build.
    pub(crate) data_unreadable: AtomicBool,
    /// When this app run last started `skiff-host`: one that stops right
    /// after it starts is not started again every read.
    pub(crate) data_started: std::sync::Mutex<Option<std::time::Instant>>,
    /// Counts the data-plane connections made: a new one may bring other
    /// data than the page shows (`data_plane_status`).
    pub(crate) data_gen: std::sync::atomic::AtomicU64,
    /// The Host id and the `[[project]]` revision of the last project read or
    /// change: what the next project change is made from.
    pub(crate) revision: std::sync::Mutex<Option<(skiff_host_api::HostId, String)>>,
    /// The Host id and the `[agents]` revision of the last agent read or
    /// change of a remote Host: what the next agent change is made from.
    pub(crate) agent_revision: std::sync::Mutex<Option<(skiff_host_api::HostId, String)>>,
    /// The one endpoint this window reaches: this Mac, or the remote Host
    /// `hosts.toml` selects. Only `select_host` changes it, never a reconnect.
    /// Legacy paths only borrow its accepted Client.
    selection: std::sync::Mutex<Option<HostSelection>>,
    /// The same selected Host connection, for keystrokes, which must not
    /// wait on a lock.
    pub(crate) live: std::sync::Mutex<Option<Arc<HostConnection>>>,
    /// The shown remote Host reported another identity than its pin: the
    /// destination and the id it reported, for `accept_host_identity`.
    pub(crate) changed: std::sync::Mutex<Option<(String, skiff_host_api::HostId)>>,
    pub(crate) subs: Mutex<HashMap<String, JoinHandle<()>>>,
    /// The task that streams events to the page.
    pub(crate) events: Mutex<Option<JoinHandle<()>>>,
    /// Terminal input on its way to the daemon.
    pub(crate) input: Input,
    /// Per session: how far the page's xterm is behind its stream.
    pub(crate) flows: std::sync::Mutex<HashMap<String, Arc<Flow>>>,
    /// Set when the daemon differs from this app and was not replaced.
    pub(crate) warning: Mutex<Option<Warning>>,
    /// The version of a daemon this app replaced at launch.
    pub(crate) replaced: Mutex<Option<String>>,
    /// An older daemon that can reload onto the bundled one. The page starts
    /// the reload with `reload_daemon`.
    pub(crate) reload: Mutex<Option<Plan>>,
    /// A reload ran. The app does not start another one on its own.
    pub(crate) reload_tried: AtomicBool,
    /// Why the daemon cannot move onto the bundled skiffd with its sessions:
    /// no handover, or a reload it refused. The warning says it on each
    /// check, including after a reconnect.
    pub(crate) no_reload: std::sync::Mutex<Option<String>>,
    /// Why the daemon is gone after a reload. No daemon starts on its own
    /// while it is set: only an explicit restart starts one.
    pub(crate) failed: std::sync::Mutex<Option<String>>,
    /// Watcher pids skiffd registered, from its peer credentials, kept from
    /// before a reload, which drops the registrations. They identify a
    /// watcher from before the lock record.
    pub(crate) watch_pids: std::sync::Mutex<Vec<u32>>,
    /// The app tried to replace the watcher this run. Set before the
    /// signal, so a watcher that did not stop is not signalled again.
    pub(crate) watch_tried: Arc<AtomicBool>,
    /// Held across each watcher check, so a start and a restore at once do
    /// not both act on the same lock.
    pub(crate) watch_gate: Mutex<()>,
}

impl App {
    /// The selected Host's session connection, while it is open.
    pub(crate) async fn live_session(&self) -> Option<Arc<HostConnection>> {
        self.host.lock().await.as_ref().map(|h| h.connection()).filter(|c| !c.is_closed())
    }

    /// The selected endpoint, from `hosts.toml` on first use. A file that
    /// cannot be read selects this Mac.
    pub(crate) fn selection(&self) -> HostSelection {
        let mut selection = self.selection.lock().unwrap();
        let selection = selection.get_or_insert_with(|| {
            crate::hosts::load(&crate::hosts::path())
                .map(|hosts| crate::hosts::selected(&hosts, local_selection))
                .unwrap_or_else(|e| {
                    eprintln!("hosts.toml: {e:#}; showing this computer");
                    local_selection()
                })
        });
        selection.clone()
    }

    /// The window shows a remote Host. Commands that read this Mac's data
    /// are refused then (see `lib.rs`).
    pub(crate) fn remote(&self) -> bool {
        !self.selection().is_local()
    }

    /// The Host id this window's live connection accepted, if it has one.
    pub(crate) fn live_identity(&self) -> Option<skiff_host_api::HostId> {
        self.live.lock().unwrap().as_ref().map(|c| c.hello().host.host_id.clone())
    }

    #[cfg(test)]
    pub(crate) fn set_selection(&self, selection: HostSelection) {
        *self.selection.lock().unwrap() = Some(selection);
    }
}

/// This Mac: the local skiffd socket, and the bootstrap id for an older one.
pub(crate) fn local_selection() -> HostSelection {
    HostSelection::new(socket_path(), skiff_core::config::config_path().with_file_name("local-host-id"))
}

/// Shows another endpoint. The old connection, its streams and any held
/// input go first: nothing typed for one Host reaches another.
pub(crate) async fn replace_selection(app: &App, selection: HostSelection) {
    let mut guard = app.host.lock().await;
    *guard = None;
    *app.data.lock().await = None;
    *app.live.lock().unwrap() = None;
    app.input.discard();
    for (_, task) in app.subs.lock().await.drain() {
        task.abort();
    }
    app.flows.lock().unwrap().clear();
    if let Some(task) = app.events.lock().await.take() {
        task.abort();
    }
    *app.warning.lock().await = None;
    *app.replaced.lock().await = None;
    *app.reload.lock().await = None;
    *app.failed.lock().unwrap() = None;
    *app.changed.lock().unwrap() = None;
    *app.selection.lock().unwrap() = Some(selection);
}


#[derive(Serialize, Clone)]
pub(crate) struct Warning {
    /// `outdated`, `protocol`, `newer`, `hung`, `failed` for a reload that
    /// did not come back, `remote` for a remote Host's problem,
    /// `identity` for a remote Host that reported another identity, or
    /// `update` for a Skiff that Skiff installed there and can update.
    pub(crate) kind: &'static str,
    pub(crate) message: String,
    /// Live sessions a restart would stop. `None` when the daemon did not say.
    pub(crate) sessions: Option<usize>,
}

#[derive(Serialize, Clone)]
pub(crate) struct DaemonStatus {
    connected: bool,
    version: Option<String>,
    /// The daemon's protocol. `None` when the app could not ask.
    protocol: Option<u32>,
    socket: String,
    /// The daemon's pid, from the socket's peer credentials.
    pid: Option<i32>,
    spawned: bool,
    warning: Option<Warning>,
    replaced: Option<String>,
    /// The daemon is older and can reload onto this app's skiffd. The page
    /// calls `reload_daemon` next.
    reload: bool,
}

/// A daemon that takes longer than this to answer `hello` is hung.
pub(crate) const HELLO_TIMEOUT: Duration = Duration::from_secs(2);

/// Reuses a live connection, else connects, else starts the daemon and connects.
/// A live connection costs no round trip: every keystroke comes through here.
pub(crate) async fn ensure_client(app: &App) -> Result<(Arc<Client>, bool), String> {
    let (host, spawned) = ensure_host(app).await?;
    Ok((host.client(), spawned))
}

/// The one M1b read path: same owner and legacy result, through HostConnection.
pub(crate) async fn screen_now(app: &App, session: &str, rows: u16) -> Result<Vec<String>, String> {
    let (host, _) = ensure_host(app).await?;
    skiff_client::host::local::legacy_screen(&host.connection(), session, rows).await.map_err(err)
}

/// The one M1c read path; lifecycle compatibility checks still use Client.
pub(crate) async fn inventory_now(app: &App) -> Result<Vec<skiff_core::session::SessionInfo>, String> {
    let (host, _) = ensure_host(app).await?;
    skiff_client::host::local::legacy_inventory(&host.connection()).await.map_err(err)
}

/// The selected owner's Host connection: the M1e terminal plane and the M1g
/// session lifecycle and groups. The caller keeps no Client of its own.
pub(crate) async fn host_now(app: &App) -> Result<Arc<HostConnection>, String> {
    let (host, _) = ensure_host(app).await?;
    Ok(host.connection())
}

/// The selected Host's capabilities as `name@major`, for the page: it offers
/// a session or terminal surface only when the Host has its capability. The
/// Host connection checks them again on every request.
///
/// One Host, one list: the session plane's capabilities and, when its
/// data-plane service answers, that service's. Without the service the
/// list simply lacks `projects` and `git_status`; nothing falls back to
/// this Mac's data.
///
/// This Mac's agent and Skiff agent changes are the exception: this app's
/// local adapter writes them (`config.rs`, `skiff_agent.rs`), so they are
/// offered whatever skiff-host answers, or whether one answers at all.
#[tauri::command]
pub(crate) async fn host_capabilities(app: State<'_, App>) -> Result<Vec<String>, String> {
    let c = host_now(&app).await?;
    let mut names = capability_names(&c);
    match data_now(&app).await {
        Ok(d) => names.extend(capability_names(&d).into_iter().filter(|n| !names.contains(n)).collect::<Vec<_>>()),
        Err(e) => eprintln!("data plane: {e}"),
    }
    if !app.remote() {
        add_local_adapter(&mut names);
    }
    Ok(names)
}

/// The capabilities this app serves itself for This Mac: the settings it
/// writes, and their reads, so a change reads back from the same config, and
/// Skipper's sessions, which it starts with the bundled `skiff`, fleet
/// Skippers included, and the Skippers' instruction files.
const LOCAL_ADAPTER: [&str; 8] = ["agent_config@1", "skipper_config@1", "skipper@1", "skipper_file@1", "skipper_run@1", "skipper_run@2", "session_agents@1", "daemon_metrics@1"];

fn add_local_adapter(names: &mut Vec<String>) {
    for cap in LOCAL_ADAPTER {
        if !names.iter().any(|n| n == cap) {
            names.push(cap.to_string());
        }
    }
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

/// Stops the running daemon, which ends its sessions, and starts a new one.
#[tauri::command]
pub(crate) async fn restart_daemon(app: State<'_, App>) -> Result<DaemonStatus, String> {
    {
        let mut guard = app.host.lock().await;
        // A peer pid is the local socket's peer: signal it only for a local endpoint.
        let selection = app.selection();
        let control = selection.process_control().ok_or("skiffd on this Host is not managed by this app")?;
        let pid = match guard.as_ref() {
            Some(host) => host.client().daemon_pid,
            None => selection.connect().await.ok().and_then(|c| c.client().daemon_pid),
        };
        *guard = None;
        *app.live.lock().unwrap() = None;
        if let Some(pid) = pid {
            if !stop_daemon(&control, pid, true).await {
                return Err(format!("skiffd (pid {pid}) did not stop"));
            }
        }
        *app.warning.lock().await = None;
        *app.failed.lock().unwrap() = None;
        *app.no_reload.lock().unwrap() = None;
        *app.reload.lock().await = None;
    }
    for (_, task) in app.subs.lock().await.drain() {
        task.abort();
    }
    daemon_status(app).await
}

pub(crate) fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Runs a call that may wait, such as a file lock, off the async workers.
pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
pub(crate) async fn daemon_status(app: State<'_, App>) -> Result<DaemonStatus, String> {
    Ok(status_now(&app).await)
}

#[cfg(test)]
mod tests {
    #[test]
    fn page_status_dtos_keep_their_json_shapes() {
        let daemon = super::DaemonStatus { connected: false, version: None, protocol: None, socket: "/s.sock".into(), pid: None, spawned: false, warning: None, replaced: None, reload: false };
        assert_eq!(serde_json::to_value(daemon).unwrap(), serde_json::json!({"connected":false,"version":null,"protocol":null,"socket":"/s.sock","pid":null,"spawned":false,"warning":null,"replaced":null,"reload":false}));
        let data = super::DataPlaneStatus { ready: false, waiting: true, host_id: None, instance_id: None, connection: 0, capabilities: vec![], message: None };
        assert_eq!(serde_json::to_value(data).unwrap(), serde_json::json!({"ready":false,"waiting":true,"host_id":null,"instance_id":null,"connection":0,"capabilities":[],"message":null}));
    }

    /// This Mac's settings writes do not wait on skiff-host: an old one that
    /// lacks them, or none, still leaves them offered, each once.
    #[test]
    fn this_mac_offers_its_own_settings_writes() {
        let mut none = vec!["session_inventory@1".to_string()];
        super::add_local_adapter(&mut none);
        assert_eq!(none, ["session_inventory@1", "agent_config@1", "skipper_config@1", "skipper@1", "skipper_file@1", "skipper_run@1", "skipper_run@2", "session_agents@1", "daemon_metrics@1"]);
        let mut current = ["skipper_config@1", "agent_config@1", "skipper@1", "skipper_file@1", "skipper_run@1", "skipper_run@2", "session_agents@1", "daemon_metrics@1"].map(String::from).to_vec();
        super::add_local_adapter(&mut current);
        assert_eq!(current.len(), 8);
    }

    #[tokio::test]
    async fn status_and_legacy_commands_share_the_selected_local_connection() {
        use super::*;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let dir = std::env::temp_dir().join(format!("skiff-m1a-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("selected.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        // Existing data remains byte-for-byte unchanged by the new bootstrap.
        let config = dir.join("projects.toml");
        let ledger = dir.join("ledger.db");
        std::fs::write(&config, "# existing config\n").unwrap();
        std::fs::write(&ledger, b"existing ledger fixture").unwrap();
        let app = App::default();
        app.set_selection(HostSelection::new(socket.clone(), dir.join("local-host-id")));
        let inventory_fixture: serde_json::Value = serde_json::from_str(include_str!("../../../../crates/skiff-host-api/tests/fixtures/m1c.json")).unwrap();
        let inventory = inventory_fixture["reply"]["sessions"].clone();
        let expected_inventory: Vec<skiff_core::session::SessionInfo> = serde_json::from_value(inventory.clone()).unwrap();
        let updates_fixture: serde_json::Value = serde_json::from_str(include_str!("../../../../crates/skiff-host-api/tests/fixtures/m1d.json")).unwrap();
        let page_events = updates_fixture["legacy"].as_array().unwrap().clone();
        let wire_events = page_events.clone();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            for id in 1..=3 {
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), serde_json::json!({"id":id,"cmd":"ping"}));
                let reply = serde_json::json!({"id":id,"type":"pong","version":skiff_core::VERSION,"protocol":skiff_core::PROTOCOL});
                let mut bytes = serde_json::to_vec(&reply).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            for (id, response) in [
                (4, serde_json::json!({"type":"screen","lines":["", "λ  "]})),
                (5, serde_json::json!({"type":"error","message":"unknown variant `screen`"})),
            ] {
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), serde_json::json!({"id":id,"cmd":"screen","session":"snapshot","rows":2}));
                let mut response = response; response["id"] = id.into();
                let mut bytes = serde_json::to_vec(&response).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            for (id, response) in [
                (6, serde_json::json!({"type":"sessions","sessions":inventory})),
                (7, serde_json::json!({"type":"error","message":"inventory unavailable"})),
            ] {
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), serde_json::json!({"id":id,"cmd":"list_sessions"}));
                let mut response = response; response["id"] = id.into();
                let mut bytes = serde_json::to_vec(&response).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            // Pushed events, with no subscribe request from the page stream.
            for event in wire_events {
                let mut bytes = serde_json::to_vec(&event).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            // M1e: the terminal plane sends exactly the legacy frames.
            let legacy = |id: u64, request: skiff_core::protocol::Request| serde_json::to_value(skiff_core::protocol::Envelope { id, request }).unwrap();
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), legacy(8, skiff_core::protocol::Request::Subscribe { session: "t1".into() }));
            // The snapshot and output precede the reply, as skiffd may send them.
            for frame in [
                serde_json::json!({"event":"snapshot","session":"t1","data":"AP8KDQ==","cols":80,"rows":24}),
                serde_json::json!({"event":"output","session":"other","data":"eA=="}),
                serde_json::json!({"event":"output","session":"t1","data":"bHMK","echo":{"wrote":1,"read":2}}),
                serde_json::json!({"id":8,"type":"ok"}),
            ] {
                let mut bytes = serde_json::to_vec(&frame).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            line.clear();
            stream.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), legacy(9, skiff_core::protocol::Request::Write { session: "t1".into(), data: vec![0, 255, b'\n'], paste: false }));
            line.clear();
            stream.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), legacy(10, skiff_core::protocol::Request::Write { session: "t1".into(), data: b"ls\n".to_vec(), paste: false }));
            for (id, request, reply) in [
                (11, skiff_core::protocol::Request::Resize { session: "t1".into(), cols: 100, rows: 30 }, serde_json::json!({"type":"ok"})),
                (12, skiff_core::protocol::Request::Resize { session: "gone".into(), cols: 1, rows: 1 }, serde_json::json!({"type":"error","message":"no such session"})),
                (13, skiff_core::protocol::Request::Unsubscribe { session: "t1".into() }, serde_json::json!({"type":"ok"})),
            ] {
                line.clear();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), legacy(id, request));
                let mut reply = reply; reply["id"] = id.into();
                let mut bytes = serde_json::to_vec(&reply).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            // M1g: lifecycle and groups send exactly the legacy frames.
            let spec: skiff_core::session::SessionSpec = serde_json::from_value(serde_json::json!({"label":"sh","cwd":"/srv/λ","cols":90,"rows":20})).unwrap();
            let group: skiff_core::group::Group = serde_json::from_value(serde_json::json!({"name":"G","layout":{"type":"pane","session":"t1"}})).unwrap();
            let mut row = inventory[0].clone();
            row["id"] = "made".into();
            for (id, request, reply) in [
                (14, skiff_core::protocol::Request::CreateSession { spec }, serde_json::json!({"type":"session","session":row})),
                (15, skiff_core::protocol::Request::Kill { session: "gone".into() }, serde_json::json!({"type":"error","message":"no such session"})),
                (16, skiff_core::protocol::Request::ListGroups, serde_json::json!({"type":"groups","groups":[]})),
                (17, skiff_core::protocol::Request::SaveGroup { group }, serde_json::json!({"type":"group","group":{"id":"g1","name":"G","layout":{"type":"pane","session":"t1"},"focus":null}})),
            ] {
                line.clear();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), legacy(id, request));
                let mut reply = reply; reply["id"] = id.into();
                let mut bytes = serde_json::to_vec(&reply).unwrap(); bytes.push(b'\n');
                stream.get_mut().write_all(&bytes).await.unwrap();
            }
            // Keep the one connection alive through the reuse assertions.
            line.clear();
            stream.read_line(&mut line).await.ok();
        });
        let first = status_now(&app).await;
        assert!(first.connected && !first.spawned);
        assert_eq!(first.version.as_deref(), Some(skiff_core::VERSION));
        assert_eq!(first.protocol, Some(skiff_core::PROTOCOL));
        assert_eq!(first.socket, socket.display().to_string());
        assert!(first.warning.is_none() && !first.reload);
        let owner = app.host.lock().await.as_ref().unwrap().clone();
        let mut updates = skiff_client::host::local::legacy_updates(&owner.connection()).unwrap();
        let (client, spawned) = ensure_client(&app).await.unwrap();
        assert!(!spawned && Arc::ptr_eq(&owner.client(), &client));
        let live = app.live.lock().unwrap().clone().unwrap();
        assert!(Arc::ptr_eq(&live, &owner.connection()));
        let second = status_now(&app).await;
        assert_eq!(second.version, first.version);
        assert_eq!(screen_now(&app, "snapshot", 2).await.unwrap(), vec!["", "λ  "]);
        assert_eq!(screen_now(&app, "snapshot", 2).await.unwrap_err(), "unknown variant `screen`");
        let listed = inventory_now(&app).await.unwrap();
        assert_eq!(listed, expected_inventory);
        assert_eq!(serde_json::to_value(&listed).unwrap(), serde_json::to_value(&expected_inventory).unwrap());
        assert_eq!(inventory_now(&app).await.unwrap_err(), "inventory unavailable");
        assert!(Arc::ptr_eq(&owner, app.host.lock().await.as_ref().unwrap()));
        for want in &page_events {
            assert_eq!(serde_json::to_value(updates.recv().await.unwrap()).unwrap(), *want);
        }
        // M1e: attach/output/input/resize/detach on the same selected owner.
        let terminal = host_now(&app).await.unwrap();
        assert!(Arc::ptr_eq(&terminal, &live));
        let mut output = skiff_client::host::local::legacy_output(&terminal, "t1").unwrap();
        skiff_client::host::local::legacy_attach(&terminal, "t1").await.unwrap();
        use skiff_client::host::TerminalFrame;
        assert_eq!(output.recv().await, Ok(TerminalFrame::Snapshot { data: vec![0, 255, 10, 13], cols: 80, rows: 24 }));
        let Ok(TerminalFrame::Output { data, echo: Some(echo) }) = output.recv().await else { panic!("no output") };
        assert_eq!(data, b"ls\n");
        // The latency trace reads the same JSON as before.
        assert_eq!(serde_json::to_value(echo).unwrap(), serde_json::json!({"wrote":1,"read":2}));
        // Input through the bridge, in order, with arbitrary bytes.
        assert!(matches!(app.input.write(Some(&live), "t1", vec![0, 255, b'\n']).unwrap(), crate::input::Sent::Done));
        assert!(matches!(app.input.write(Some(&live), "t1", b"ls\n".to_vec()).unwrap(), crate::input::Sent::Done));
        skiff_client::host::local::legacy_resize(&terminal, "t1", 100, 30).await.unwrap();
        assert_eq!(skiff_client::host::local::legacy_resize(&terminal, "gone", 1, 1).await.unwrap_err().to_string(), "no such session");
        skiff_client::host::local::legacy_detach(&terminal, "t1").await.unwrap();
        // Detach closes the receiver first, as unsubscribe did.
        assert_eq!(output.recv().await, Err(skiff_client::host::OutputGap::Closed));
        let mut reattached = skiff_client::host::local::legacy_output(&terminal, "t1").unwrap();
        // M1g: lifecycle and groups on the same selected owner, and the
        // capabilities the page reads.
        assert_eq!(capability_names(&terminal), ["connection_status@1", "session_screen@1", "session_inventory@1", "session_updates@1", "session_terminal@1", "session_lifecycle@1", "session_groups@1", "session_agents@1"]);
        let made = skiff_client::host::local::legacy_create(&terminal, serde_json::from_value(serde_json::json!({"label":"sh","cwd":"/srv/λ","cols":90,"rows":20})).unwrap()).await.unwrap();
        assert_eq!(made.id, "made");
        assert_eq!(skiff_client::host::local::legacy_kill(&terminal, "gone").await.unwrap_err().to_string(), "no such session");
        assert!(skiff_client::host::local::legacy_groups(&terminal).await.unwrap().is_empty());
        let saved = skiff_client::host::local::legacy_save_group(&terminal, serde_json::from_value(serde_json::json!({"name":"G","layout":{"type":"pane","session":"t1"}})).unwrap()).await.unwrap();
        assert_eq!(saved.id, "g1");
        assert!(Arc::ptr_eq(&owner, app.host.lock().await.as_ref().unwrap()));
        drop(terminal);
        drop(live);
        // Dropping the selected owner, as reconnect/restart/reload do, ends it.
        drop(owner);
        drop(client);
        *app.host.lock().await = None;
        assert!(updates.recv().await.is_none());
        assert_eq!(reattached.recv().await, Err(skiff_client::host::OutputGap::Closed));
        *app.live.lock().unwrap() = None;
        assert_eq!(std::fs::read_to_string(config).unwrap(), "# existing config\n");
        assert_eq!(std::fs::read(ledger).unwrap(), b"existing ledger fixture");
        peer.abort();
        let _ = std::fs::remove_dir_all(dir);
    }



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

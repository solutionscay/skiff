//! Known Hosts and which one this window shows: client-owned `hosts.toml`
//! beside `projects.toml`. "This Mac" is the local Host and needs no entry.
//! A remote Host has a local id that never changes, a shown name, an SSH
//! destination (an alias from ssh config, a hostname or an address), an
//! optional user and port, and the skiffd socket on it: the one the probe
//! found (Settings › Hosts › Check), or one the person set. No two Hosts
//! have the same destination, user and port. Each remote Host keeps the Skiff identity it first
//! reported (`host_id`): a different one is refused on every later
//! connection, after a new selection or app start too, until the person
//! accepts it. Each remote Host has its own clipboard opt-in for OSC 52
//! writes from its terminals, given to that identity only, and it counts
//! only while this window's live connection answers as that identity.
//!
//! Every change to the file is one transaction for all windows and app
//! runs: [`update`] holds an exclusive `flock` from the read to the rename.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use skiff_client::host::{
    keys,
    local::HostSelection,
    ssh::{SshOptions, SshTarget},
};
use skiff_host_api::HostId;
use tauri::{Manager, State};

use crate::connection::{err, App};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnownHost {
    /// Local only. Rename and edit keep it; `selected` names it.
    pub(crate) id: String,
    /// Shown in the Host menu, Settings and the window title.
    pub(crate) name: String,
    /// What `ssh` takes: an alias from ssh config, `host` or `user@host`.
    pub(crate) destination: String,
    /// Sent as `-l` only when the person set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) user: Option<String>,
    /// Sent as `-p` only when the person set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) port: Option<u16>,
    /// skiffd's socket on that Host: the one the last check found, or the
    /// one the person set. None until a check finds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) socket: Option<String>,
    /// The person set `socket`; a check does not replace it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) socket_override: bool,
    /// The Host id skiffd reported on the first accepted connection. Only
    /// the person changes it, by accepting a new identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) host_id: Option<String>,
    /// The Host id whose OSC 52 clipboard writes reach this Mac. It counts
    /// only while it is `host_id`: a new identity has no permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) clipboard_host: Option<String>,
}

impl KnownHost {
    #[cfg(test)]
    fn new(destination: String, socket: String) -> Self {
        Self { id: format!("h_{destination}"), name: destination.clone(), destination, user: None, port: None, socket: Some(socket), socket_override: false, host_id: None, clipboard_host: None }
    }

    pub(crate) fn target(&self) -> SshTarget {
        SshTarget { destination: self.destination.clone(), user: self.user.clone(), port: self.port, socket: self.socket.clone().unwrap_or_default() }
    }

    /// The same login: destination, user and port.
    fn logs_in_as(&self, t: &SshTarget) -> bool {
        self.destination == t.destination && self.user == t.user && self.port == t.port
    }

    /// Clipboard writes from a connection that answers as `live` are
    /// allowed: it is the identity this Host must have, and the one the
    /// person allowed. Another window's newer pin or grant does not count.
    pub(crate) fn clipboard_for(&self, live: &HostId) -> bool {
        self.host_id.as_deref() == Some(live.as_str()) && self.clipboard_host.as_deref() == Some(live.as_str())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Hosts {
    /// The id of the Host this window shows. None: this Mac.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selected: Option<String>,
    #[serde(default, rename = "host")]
    pub(crate) hosts: Vec<KnownHost>,
}

pub(crate) fn path() -> PathBuf {
    skiff_core::config::config_path().with_file_name("hosts.toml")
}

pub(crate) fn load(path: &Path) -> anyhow::Result<Hosts> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Hosts::default()),
        Err(e) => Err(e.into()),
    }
}

/// Changes the file as one step for every window and app run. An exclusive
/// `flock` on `hosts.toml.lock`, beside it, is held from the read to the
/// rename, so no change is lost to another, and a check such as "no pin yet"
/// still holds when the change is written. Nothing is written when `change`
/// fails or changes nothing. Every write of the file goes through here.
fn update<T>(path: &Path, change: impl FnOnce(&mut Hosts) -> anyhow::Result<T>) -> anyhow::Result<T> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let lock = std::fs::File::options().create(true).truncate(false).write(true).open(path.with_extension("toml.lock"))?;
    // Released when `lock` closes, or when the process ends.
    lock.lock()?;
    let before = load(path)?;
    let mut hosts = before.clone();
    let out = change(&mut hosts)?;
    if hosts != before {
        save(path, &hosts)?;
    }
    Ok(out)
}

/// A complete file or none: written beside, then renamed over. Only under
/// [`update`]'s lock.
fn save(path: &Path, hosts: &Hosts) -> anyhow::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    std::fs::write(&temp, toml::to_string(hosts)?)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

impl Hosts {
    pub(crate) fn find(&self, id: &str) -> Option<&KnownHost> {
        self.hosts.iter().find(|h| h.id == id)
    }

    /// The Host a selection's target logs in to.
    pub(crate) fn find_login(&self, t: &SshTarget) -> Option<&KnownHost> {
        self.hosts.iter().find(|h| h.logs_in_as(t))
    }

    /// The selection the file names. A selected Host that is not in the list
    /// is this Mac: the file never points at an unknown endpoint.
    pub(crate) fn selection(&self, local: impl FnOnce() -> HostSelection) -> HostSelection {
        match self.selected.as_deref().and_then(|d| self.find(d)) {
            Some(h) => remote(h).unwrap_or_else(|_| local()),
            None => local(),
        }
    }
}

/// The private directory for this Host's forwards. Each forward makes its
/// own directory inside it. Short, as a socket path must be.
fn forward_dir(id: &str) -> PathBuf {
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    // SAFETY: geteuid takes no arguments and cannot fail.
    std::env::temp_dir().join(format!("skiff-{}-{:08x}", unsafe { libc::geteuid() }, hash as u32))
}

pub(crate) fn remote(h: &KnownHost) -> Result<HostSelection, String> {
    if h.socket.is_none() {
        return Err(format!("Skiff has not found the socket on {} yet. Check it in Settings › Hosts.", h.name));
    }
    let pin = h.host_id.clone().map(HostId::try_from).transpose()?;
    HostSelection::ssh(h.target(), SshOptions::default(), forward_dir(&h.id), pin)
}

fn known<'a>(hosts: &'a mut Hosts, id: &str) -> anyhow::Result<&'a mut KnownHost> {
    hosts.hosts.iter_mut().find(|h| h.id == id).ok_or_else(|| anyhow::anyhow!("That Host is not in the list any more."))
}

/// The Host a selection's target logs in to, for a change made from a
/// live connection.
fn known_login<'a>(hosts: &'a mut Hosts, t: &SshTarget) -> anyhow::Result<&'a mut KnownHost> {
    hosts.hosts.iter_mut().find(|h| h.logs_in_as(t)).ok_or_else(|| anyhow::anyhow!("{} is not a known Host", t.destination))
}

/// A new local id: `h_` and 8 hex digits, not in the list yet.
fn new_id(hosts: &Hosts) -> anyhow::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    loop {
        let mut b = [0u8; 4];
        f.read_exact(&mut b)?;
        let id = format!("h_{:08x}", u32::from_be_bytes(b));
        if hosts.find(&id).is_none() {
            return Ok(id);
        }
    }
}

/// Stores the identity a Host reported on its first accepted connection: a
/// compare-and-set under [`update`]'s lock. The file decides: when another
/// window or run stored a different one first, this one is refused as a
/// changed identity. Ok only once the pin is on disk.
pub(crate) fn pin_first(file: &Path, target: &SshTarget, host_id: &HostId) -> anyhow::Result<()> {
    update(file, |hosts| {
        let h = known_login(hosts, target)?;
        match &h.host_id {
            Some(pin) if pin == host_id.as_str() => Ok(()),
            Some(pin) => Err(skiff_host_api::Error::IdentityChanged {
                expected: HostId::try_from(pin.clone()).map_err(anyhow::Error::msg)?,
                received: host_id.clone(),
            }
            .into()),
            None => {
                h.host_id = Some(host_id.as_str().to_string());
                h.clipboard_host = None;
                Ok(())
            }
        }
    })
}

/// Makes `host_id` the identity Host `id` must report, as the person
/// accepted it. The clipboard permission of the old identity goes.
pub(crate) fn accept_identity(file: &Path, id: &str, host_id: &HostId) -> anyhow::Result<Hosts> {
    update(file, |hosts| {
        let h = known(hosts, id)?;
        h.host_id = Some(host_id.as_str().to_string());
        h.clipboard_host = None;
        Ok(hosts.clone())
    })
}

/// Allows or stops OSC 52 writes from `destination` for `live`, the identity
/// the asking window's connection accepted. Allowing needs that identity to
/// be the one the file pins now: a window still connected to an identity
/// another window replaced cannot grant the new one, nor its own.
fn set_clipboard(file: &Path, id: &str, live: Option<&HostId>, allow: bool) -> anyhow::Result<()> {
    update(file, |hosts| {
        let h = known(hosts, id)?;
        let destination = h.name.clone();
        h.clipboard_host = match live {
            _ if !allow => None,
            Some(id) if h.host_id.as_deref() == Some(id.as_str()) => Some(id.as_str().to_string()),
            Some(_) => anyhow::bail!("Skiff stored another identity for {destination} since this window connected. Pick the Host again, then allow clipboard writes."),
            None => anyhow::bail!("Skiff is not connected to {destination}. Connect once, then allow clipboard writes."),
        };
        Ok(())
    })
}

/// Clipboard writes from the shown Host are allowed for a connection that
/// answers as `live`, as the file says now.
fn clipboard_allowed(file: &Path, target: &SshTarget, live: &HostId) -> anyhow::Result<bool> {
    Ok(load(file)?.find_login(target).is_some_and(|h| h.clipboard_for(live)))
}

/// A known Host as the page sees it.
#[derive(Serialize)]
struct HostView {
    id: String,
    name: String,
    destination: String,
    user: Option<String>,
    port: Option<u16>,
    socket: Option<String>,
    socket_override: bool,
    /// From the last check or connection in this app run: `ok`,
    /// `attention`, or `unknown` before any.
    state: &'static str,
}

/// Each Host's last check or connection in this app run, by id: it passed.
static LAST: std::sync::Mutex<std::collections::BTreeMap<String, bool>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Notes how the last check or connection of a Host went, for the list and
/// the menu.
pub(crate) fn note(id: &str, ok: bool) {
    LAST.lock().unwrap().insert(id.to_string(), ok);
}

/// As [`note`], for the Host a live selection's target logs in to.
pub(crate) fn note_login(target: &SshTarget, ok: bool) {
    if let Some(h) = load(&path()).ok().and_then(|hosts| hosts.find_login(target).cloned()) {
        note(&h.id, ok);
    }
}

fn state(h: &KnownHost) -> &'static str {
    match LAST.lock().unwrap().get(&h.id) {
        _ if h.socket.is_none() => "attention",
        Some(true) => "ok",
        Some(false) => "attention",
        None => "unknown",
    }
}

/// What the page shows and offers: the selected Host, and the known ones.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostList {
    /// The shown Host's id. None: this Mac.
    selected: Option<String>,
    /// This computer's hostname, for the local Host's label. Empty if the
    /// system gives none.
    local_name: String,
    hosts: Vec<HostView>,
    /// The shown remote Host's clipboard permission, for the menu: given to
    /// the identity this window accepted. Each write asks `host_clipboard`.
    clipboard: bool,
}

/// `identity`: the one this window accepted, live or pinned.
fn host_list(hosts: Hosts, selected: Option<String>, identity: Option<&HostId>) -> HostList {
    let clipboard = match (&selected, identity) {
        (Some(d), Some(id)) => hosts.find(d).is_some_and(|h| h.clipboard_for(id)),
        _ => false,
    };
    let hosts = hosts
        .hosts
        .into_iter()
        .map(|h| HostView { state: state(&h), id: h.id, name: h.name, destination: h.destination, user: h.user, port: h.port, socket: h.socket, socket_override: h.socket_override })
        .collect();
    HostList { selected, local_name: local_hostname(), hosts, clipboard }
}

/// The short hostname (`thr0w-deb`, not `thr0w-deb.local`), or empty.
fn local_hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer and its length are valid; gethostname writes at most that.
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]);
    name.split('.').next().unwrap_or_default().trim().to_string()
}

#[tauri::command]
pub(crate) async fn list_hosts(app: State<'_, App>) -> Result<HostList, String> {
    let hosts = blocking(|| load(&path())).await?;
    let selection = app.selection();
    let identity = app.live_identity().or_else(|| selection.pinned());
    let selected = selection.ssh_target().and_then(|t| hosts.find_login(t)).map(|h| h.id.clone());
    Ok(host_list(hosts, selected, identity.as_ref()))
}

/// The shown Host's id, as the file names it now. None: this Mac.
fn shown_id(app: &App) -> Option<String> {
    let t = app.selection().ssh_target()?.clone();
    load(&path()).ok()?.find_login(&t).map(|h| h.id.clone())
}

/// An OSC 52 write from the shown Host may reach this Mac's clipboard now:
/// asked on every write, against the identity this window's live connection
/// accepted and the file as it is now. This Mac: always.
#[tauri::command]
pub(crate) async fn host_clipboard(app: State<'_, App>) -> Result<bool, String> {
    let Some(target) = app.selection().ssh_target().cloned() else {
        return Ok(true);
    };
    let Some(live) = app.live_identity() else { return Ok(false) };
    blocking(move || clipboard_allowed(&path(), &target, &live)).await
}

/// What the Add and Edit form sends. Empty name, user or port: the name
/// comes from the destination, user and port from ssh config. Empty
/// socket: the probe finds it.
#[derive(Deserialize, Debug, Clone)]
pub(crate) struct HostForm {
    #[serde(default)]
    name: String,
    destination: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    port: String,
    #[serde(default)]
    socket: String,
}

impl HostForm {
    /// The checked values: name, target. An empty target socket: none set.
    /// Copy follows Settings › Hosts.
    fn parse(&self) -> anyhow::Result<(String, SshTarget)> {
        let destination = self.destination.trim().to_string();
        anyhow::ensure!(!destination.is_empty(), "Pick a server or type its address.");
        let user = Some(self.user.trim().to_string()).filter(|u| !u.is_empty());
        let port = match self.port.trim() {
            "" => None,
            p => Some(p.parse::<u16>().ok().filter(|&p| p > 0).ok_or_else(|| anyhow::anyhow!("Type a port from 1 to 65535."))?),
        };
        let socket = self.socket.trim().to_string();
        let target = SshTarget { destination, user, port, socket };
        target.validate_login().map_err(anyhow::Error::msg)?;
        if !target.socket.is_empty() && !skiff_client::host::check::valid_socket(&target.socket) {
            anyhow::bail!("Type a full path on the server, such as /run/user/1000/skiff/skiffd.sock.");
        }
        let name = match self.name.trim() {
            "" => default_name(&target.destination),
            n => n.to_string(),
        };
        anyhow::ensure!(!name.chars().any(|c| c.is_control()), "Type a name on one line.");
        Ok((name, target))
    }
}

/// The alias, or the host part of `user@host`.
fn default_name(destination: &str) -> String {
    destination.rsplit('@').next().unwrap_or(destination).to_string()
}

/// Refuses a name or a login that another Host has. `except`: the Host
/// being edited.
fn check_unique(hosts: &Hosts, name: &str, target: &SshTarget, except: Option<&str>) -> anyhow::Result<()> {
    let others = || hosts.hosts.iter().filter(|h| Some(h.id.as_str()) != except);
    if let Some(h) = others().find(|h| h.logs_in_as(target)) {
        anyhow::bail!("{} is in the list already.", h.name);
    }
    anyhow::ensure!(!others().any(|h| h.name.eq_ignore_ascii_case(name)), "Another Host has this name.");
    Ok(())
}

/// Adds a remote Host and answers its id. It does not connect.
#[tauri::command]
pub(crate) async fn add_host(form: HostForm) -> Result<String, String> {
    blocking(move || {
        let (name, target) = form.parse()?;
        update(&path(), |hosts| {
            check_unique(hosts, &name, &target, None)?;
            let id = new_id(hosts)?;
            let set = !target.socket.is_empty();
            hosts.hosts.push(KnownHost {
                id: id.clone(),
                name,
                destination: target.destination,
                user: target.user,
                port: target.port,
                socket: Some(target.socket).filter(|_| set),
                socket_override: set,
                host_id: None,
                clipboard_host: None,
            });
            Ok(id)
        })
    })
    .await
}

/// Changes a Host's name, server, user, port or socket. Its id and its
/// pin stay: if another machine answers there now, the pin catches it. The
/// shown Host keeps its connection, so only its name can change.
#[tauri::command]
pub(crate) async fn edit_host(app: State<'_, App>, id: String, form: HostForm) -> Result<(), String> {
    let shown = shown_id(&app);
    blocking(move || {
        let (name, target) = form.parse()?;
        update(&path(), |hosts| {
            check_unique(hosts, &name, &target, Some(&id))?;
            let h = known(hosts, &id)?;
            let set = !target.socket.is_empty();
            let login = !h.logs_in_as(&target);
            // A cleared override goes back to what the next check finds.
            let socket = match (set, login || h.socket_override) {
                (true, _) => Some(target.socket.clone()),
                (false, true) => None,
                (false, false) => h.socket.clone(),
            };
            let moved = login || socket != h.socket;
            anyhow::ensure!(!(moved && shown.as_deref() == Some(id.as_str())), "This window shows {}. Pick another Host first, then change its server.", h.name);
            if moved {
                LAST.lock().unwrap().remove(&id);
            }
            h.name = name;
            h.destination = target.destination;
            h.user = target.user;
            h.port = target.port;
            h.socket = socket;
            h.socket_override = set;
            Ok(())
        })
    })
    .await
}

/// Changes only the shown name.
#[tauri::command]
pub(crate) async fn rename_host(id: String, name: String) -> Result<(), String> {
    blocking(move || {
        update(&path(), |hosts| {
            let name = name.trim().to_string();
            anyhow::ensure!(!name.is_empty() && !name.chars().any(|c| c.is_control()), "Type a name on one line.");
            let me = known(hosts, &id)?.target();
            check_unique(hosts, &name, &me, Some(&id))?;
            known(hosts, &id)?.name = name;
            Ok(())
        })
    })
    .await
}

/// Forgets a remote Host. The shown Host cannot go: pick another first.
#[tauri::command]
pub(crate) async fn remove_host(app: State<'_, App>, id: String) -> Result<(), String> {
    if shown_id(&app).as_deref() == Some(id.as_str()) {
        return Err("This window shows that Host. Pick another Host first.".into());
    }
    blocking(move || {
        update(&path(), |hosts| {
            hosts.hosts.retain(|h| h.id != id);
            Ok(())
        })
    })
    .await
}

/// The aliases in this computer's ssh config, for the Server field.
#[tauri::command]
pub(crate) async fn ssh_aliases() -> Result<Vec<skiff_client::host::ssh_config::Alias>, String> {
    blocking(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("no home folder"))?;
        Ok(skiff_client::host::ssh_config::aliases(&home))
    })
    .await
}

/// What `ssh -G` makes of the form's server, user and port. Local only.
#[tauri::command]
pub(crate) async fn resolve_ssh(destination: String, user: String, port: String) -> Result<skiff_client::host::ssh_config::Resolved, String> {
    blocking(move || {
        let form = HostForm { name: String::new(), destination, user, port, socket: String::new() };
        let (_, target) = form.parse()?;
        skiff_client::host::ssh_config::resolve(&target, &SshOptions::default()).map_err(anyhow::Error::msg)
    })
    .await
}

/// The socket name a check looks for in the Host's runtime folder: the one
/// this build's own skiffd uses (`skiff_core::socket::socket_path`).
const SOCKET_NAME: &str = if cfg!(debug_assertions) { "skiffd-dev.sock" } else { "skiffd.sock" };

/// The check list of one Host, read only on the Host. Its outcome is noted
/// for the list and the menu, and a socket it found is stored, unless the
/// person set one.
#[tauri::command]
pub(crate) async fn check_host(id: String) -> Result<skiff_client::host::check::Report, String> {
    let file = path();
    let h = host_by_id(&id).await?;
    let input = skiff_client::host::check::CheckInput {
        target: h.target(),
        name: h.name.clone(),
        socket_override: h.socket.clone().filter(|_| h.socket_override),
        socket_name: SOCKET_NAME.into(),
        pin: h.host_id.clone().and_then(|p| HostId::try_from(p).ok()),
        forward_dir: forward_dir(&h.id),
        home: home().ok(),
        agent: true,
        installed: INSTALLED.lock().unwrap().get(&id).cloned(),
        version: VERSION.into(),
        linger_refused: LINGER_REFUSED.lock().unwrap().contains(&id),
    };
    let report = skiff_client::host::check::check(&input, &SshOptions::default()).await;
    note(&id, report.ok);
    if let Some(found) = report.socket.clone().filter(|_| !h.socket_override) {
        let target = h.target();
        blocking(move || {
            update(&file, |hosts| {
                // Only while the Host is the one this check ran on.
                if let Some(k) = hosts.hosts.iter_mut().find(|k| k.id == id && k.logs_in_as(&target) && !k.socket_override) {
                    k.socket = Some(found);
                }
                Ok(())
            })
        })
        .await?;
    }
    Ok(report)
}

async fn host_by_id(id: &str) -> Result<KnownHost, String> {
    let id = id.to_string();
    blocking(move || load(&path())?.find(&id).cloned().ok_or_else(|| anyhow::anyhow!("That Host is not in the list any more."))).await
}

/// This computer's home folder.
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| "This computer gives Skiff no home folder.".into())
}

/// What the last key install found on each server, by Host id, for the
/// check that follows it (Fix permissions).
static INSTALLED: std::sync::Mutex<std::collections::BTreeMap<String, keys::Installed>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Host keys learned into a temp file, waiting for the person's confirm, by
/// token. The page never sends a key: only the token of one Rust scanned.
static SCANNED: std::sync::Mutex<std::collections::BTreeMap<String, (String, keys::Scanned)>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The host key that answers for a Host now, for the trust dialog.
#[derive(Serialize)]
pub(crate) struct HostKeyView {
    token: String,
    fingerprints: Vec<keys::Fingerprint>,
    /// What known_hosts holds for that host now. Not empty: the key changed.
    known: Vec<keys::Fingerprint>,
    /// The known_hosts file a trust writes, with `~` for home.
    file: String,
}

/// Learns the host key that answers for a Host into a temp file. Nothing
/// the person owns changes until they confirm with [`host_key_trust`].
#[tauri::command]
pub(crate) async fn host_key_scan(id: String) -> Result<HostKeyView, String> {
    let h = host_by_id(&id).await?;
    let home = home()?;
    let scanned = keys::scan_host_key(&h.target(), &SshOptions::default(), &home).await?;
    let token = new_id(&Hosts::default()).map_err(err)?;
    let file = match scanned.file.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => scanned.file.display().to_string(),
    };
    let view = HostKeyView { token: token.clone(), fingerprints: scanned.fingerprints.clone(), known: scanned.known.clone(), file };
    SCANNED.lock().unwrap().insert(token, (id, scanned));
    Ok(view)
}

/// The person confirmed the fingerprint: its line goes into known_hosts.
/// `replace`: the old keys of that host go first, as they confirmed twice.
#[tauri::command]
pub(crate) async fn host_key_trust(token: String, replace: bool) -> Result<(), String> {
    let Some((id, scanned)) = SCANNED.lock().unwrap().remove(&token) else {
        return Err("Skiff no longer holds that key. Check again.".into());
    };
    if !scanned.known.is_empty() && !replace {
        return Err("The host key changed. Review it first.".into());
    }
    keys::trust_host_key(&scanned, replace).await?;
    LAST.lock().unwrap().remove(&id);
    Ok(())
}

/// The key to install, or why there is none to install yet.
async fn ready_key(h: &KnownHost) -> Result<String, String> {
    match keys::local_key(&h.target(), &SshOptions::default(), &home()?, true).await? {
        keys::LocalKey::Ready(line) => Ok(line),
        keys::LocalKey::Locked(_) => Err("Your SSH key is locked. Unlock it first.".into()),
        keys::LocalKey::None => Err("This computer has no SSH key. Make one first.".into()),
    }
}

/// The Skiff version this app installs on a server: its own.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Hosts whose Turn on (linger) was refused in this app run.
static LINGER_REFUSED: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// What Install Skiff… did, for the check list's note.
#[derive(Serialize)]
pub(crate) struct InstallView {
    #[serde(flatten)]
    outcome: skiff_client::host::install::Outcome,
    /// `release`, or `this build` (a dev build with no matching release).
    source: &'static str,
    version: &'static str,
}

/// The probe, for a step that changes the server: it must see the state now.
async fn probe_now(h: &KnownHost) -> Result<skiff_client::host::check::Probe, String> {
    use skiff_client::host::check;
    let over = h.socket.clone().filter(|_| h.socket_override);
    match check::run_probe(&h.target(), &SshOptions::default(), SOCKET_NAME, over.as_deref()).await? {
        check::ProbeRun::Probed(p, _) => Ok(p),
        _ => Err(format!("Skiff cannot read the state of {}. Check again.", h.name)),
    }
}

/// Installs Skiff on a Linux server in the account's home, and starts its
/// skiffd. Only where no skiffd runs that Skiff did not install. The bundle
/// is the signed release bundle for this app's version; a dev build with no
/// such release sends its own binaries.
#[tauri::command]
pub(crate) async fn install_skiff(id: String) -> Result<InstallView, String> {
    use skiff_client::host::install;
    let h = host_by_id(&id).await?;
    let p = probe_now(&h).await?;
    if p.socket_live && p.managed.is_none() {
        return Err(format!("{} runs a Skiff that Skiff did not install. Skiff uses it as it is, and does not replace it.", h.name));
    }
    let (Some("linux"), Some(arch)) = (p.bundle_os(), p.bundle_arch()) else {
        return Err(format!("Skiff installs itself on Linux servers on x86_64 and ARM64 only. {} runs {} on {}.", h.name, p.os, p.arch));
    };
    let bundle = match install::download(VERSION, "linux", arch).await {
        Ok(b) => b,
        Err(e) if cfg!(debug_assertions) => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let beside = exe.parent().ok_or("This build has no folder.")?.to_path_buf();
            install::dev_bundle(VERSION, arch, &beside).await.map_err(|dev| format!("{e} {dev}"))?
        }
        Err(e) => return Err(e),
    };
    let outcome = install::install(&h.target(), &SshOptions::default(), &bundle, VERSION, SOCKET_NAME, p.free_kb, &h.name).await?;
    if outcome.linger == "refused" {
        LINGER_REFUSED.lock().unwrap().insert(id.clone());
    }
    LAST.lock().unwrap().remove(&id);
    Ok(InstallView { outcome, source: bundle.source, version: VERSION })
}

/// Starts the managed skiffd on a server. Never one Skiff did not install.
#[tauri::command]
pub(crate) async fn start_skiff(id: String) -> Result<skiff_client::host::install::Outcome, String> {
    let h = host_by_id(&id).await?;
    let p = probe_now(&h).await?;
    if p.managed.is_none() {
        return Err(format!("Skiff did not install the Skiff on {}, so it does not start it.", h.name));
    }
    let out = skiff_client::host::install::start(&h.target(), &SshOptions::default(), SOCKET_NAME).await?;
    if out.linger == "refused" {
        LINGER_REFUSED.lock().unwrap().insert(id.clone());
    }
    if !out.running {
        return Err(format!("Skiff's sessions service on {} stops right after it starts.", h.name));
    }
    Ok(out)
}

/// Turns on lingering for the account on a server, so its user services
/// start at boot and stay after logout. False: the server refused it, and
/// the check list says so in plain words.
#[tauri::command]
pub(crate) async fn turn_on_linger(id: String) -> Result<bool, String> {
    let h = host_by_id(&id).await?;
    let on = skiff_client::host::install::linger(&h.target(), &SshOptions::default()).await?;
    let mut refused = LINGER_REFUSED.lock().unwrap();
    if on { refused.remove(&id) } else { refused.insert(id) };
    Ok(on)
}

/// Makes `~/.ssh/id_ed25519` with no passphrase, when it is not there.
#[tauri::command]
pub(crate) async fn make_key() -> Result<(), String> {
    let user = std::env::var("USER").unwrap_or_default();
    keys::make_key(&home()?, &format!("{user}@{} (Skiff)", local_hostname())).await.map(drop)
}

/// Gives the locked key ssh would use for this Host to the agent.
#[tauri::command]
pub(crate) async fn unlock_key(id: String, passphrase: String) -> Result<(), String> {
    let h = host_by_id(&id).await?;
    match keys::local_key(&h.target(), &SshOptions::default(), &home()?, true).await? {
        keys::LocalKey::Locked(key) => keys::unlock_key(&key, passphrase).await,
        _ => Ok(()),
    }
}

/// This computer's public key for this Host, for Copy public key.
#[tauri::command]
pub(crate) async fn public_key(id: String) -> Result<String, String> {
    ready_key(&host_by_id(&id).await?).await
}

/// Adds this computer's key to the Host's `authorized_keys`, once, after one
/// password entry. The password goes to ssh through askpass only.
#[tauri::command]
pub(crate) async fn install_key(id: String, password: String) -> Result<keys::Installed, String> {
    let h = host_by_id(&id).await?;
    let key = ready_key(&h).await?;
    match keys::install_key(&h.target(), &SshOptions::default(), keys::BY_PASSWORD, &key, password).await {
        Ok(done) => {
            INSTALLED.lock().unwrap().insert(id, done.clone());
            Ok(done)
        }
        Err(keys::StepError::Refused) => Err(format!("The password did not work. Nothing changed on {}.", h.name)),
        Err(keys::StepError::Other(e)) => Err(e),
    }
}

/// Puts the Host's foreign-owned `authorized_keys` aside and writes a new
/// one with this computer's key, after one password entry.
#[tauri::command]
pub(crate) async fn fix_permissions(id: String, password: String) -> Result<(), String> {
    let h = host_by_id(&id).await?;
    let key = ready_key(&h).await?;
    match keys::fix_key_file(&h.target(), &SshOptions::default(), keys::BY_PASSWORD, &key, password).await {
        Ok(()) => {
            INSTALLED.lock().unwrap().remove(&id);
            Ok(())
        }
        Err(keys::StepError::Refused) => Err(format!("The password did not work. Nothing changed on {}.", h.name)),
        Err(keys::StepError::Other(e)) => Err(e),
    }
}

/// Shows another Host in this window: the old connection, its streams and
/// any held input go first, so nothing typed for one Host reaches another.
/// The page reloads after it. `None`: this Mac.
#[tauri::command]
pub(crate) async fn select_host(handle: tauri::AppHandle, app: State<'_, App>, id: Option<String>) -> Result<(), String> {
    let selection = blocking(move || {
        update(&path(), |hosts| {
            if let Some(id) = &id {
                let h = hosts.find(id).ok_or_else(|| anyhow::anyhow!("That Host is not in the list any more."))?;
                remote(h).map_err(anyhow::Error::msg)?;
            }
            hosts.selected = id;
            Ok(hosts.selection(crate::connection::local_selection))
        })
    })
    .await?;
    crate::connection::replace_selection(&app, selection).await;
    // A launch on a remote Host skipped this Mac's Skipper setup.
    if !app.remote() {
        let handle = handle.clone();
        std::thread::spawn(move || crate::skiff_agent::sync(&handle.state::<App>()));
    }
    Ok(())
}

/// Allows or stops OSC 52 clipboard writes from a remote Host's terminals.
/// Allowed for the identity this window's live connection accepted, which
/// must be the one the Host must report, so a Host that answers with another
/// identity later has no permission.
#[tauri::command]
pub(crate) async fn set_host_clipboard(app: State<'_, App>, id: String, allow: bool) -> Result<(), String> {
    let shown = shown_id(&app).as_deref() == Some(id.as_str());
    let live = app.live_identity().filter(|_| shown);
    blocking(move || set_clipboard(&path(), &id, live.as_ref(), allow)).await
}

/// The person accepts the new identity the shown Host reported. Only the one
/// Rust saw and refused can be accepted, not one the page names. The page
/// reloads after it.
#[tauri::command]
pub(crate) async fn accept_host_identity(app: State<'_, App>, id: String) -> Result<(), String> {
    let changed = app.changed.lock().unwrap().clone();
    if shown_id(&app).as_deref() != Some(id.as_str()) {
        return Err("This window shows another Host now.".into());
    }
    let Some((_, received)) = changed.filter(|(d, _)| app.selection().ssh_target().is_some_and(|t| t.destination == *d)) else {
        return Err("This Host reported no new identity to accept.".into());
    };
    let selection = blocking(move || {
        let hosts = accept_identity(&path(), &id, &received)?;
        Ok(hosts.selection(crate::connection::local_selection))
    })
    .await?;
    crate::connection::replace_selection(&app, selection).await;
    Ok(())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev() -> SshTarget {
        SshTarget::new("devbox", "/s.sock")
    }

    #[test]
    fn hosts_round_trip_and_an_unknown_selection_is_this_mac() {
        let dir = std::env::temp_dir().join(format!("skiff-hosts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("hosts.toml");
        assert_eq!(load(&file).unwrap(), Hosts::default());
        let hosts = Hosts {
            selected: Some("h_devbox".into()),
            hosts: vec![KnownHost::new("devbox".into(), "/run/user/1000/skiff/skiffd.sock".into())],
        };
        save(&file, &hosts).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("selected = \"h_devbox\"") && text.contains("[[host]]"), "{text}");
        assert_eq!(load(&file).unwrap(), hosts);
        let local = || HostSelection::new("/l.sock".into(), "/id".into());
        let picked = hosts.selection(local);
        assert_eq!(picked.ssh_target().map(|t| t.destination.as_str()), Some("devbox"));
        assert!(picked.process_control().is_none());
        // Room under the forwards' directory for `<pid>-<n>/skiffd.sock`.
        assert!(picked.socket().as_os_str().len() + 32 < 104, "{}", picked.socket().display());
        assert_eq!(picked.pinned(), None);
        let gone = Hosts { selected: Some("h_gone".into()), ..hosts.clone() };
        assert!(gone.selection(local).is_local());
        let bad = Hosts { hosts: vec![KnownHost { destination: "-oProxyCommand=x".into(), ..hosts.hosts[0].clone() }], selected: Some("h_devbox".into()) };
        assert!(bad.selection(local).is_local(), "an invalid entry never becomes an endpoint");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_form_keeps_id_name_user_and_port_and_refuses_duplicates() {
        let form = |name: &str, d: &str, user: &str, port: &str| HostForm { name: name.into(), destination: d.into(), user: user.into(), port: port.into(), socket: String::new() };
        let (name, t) = form("", "jose@devbox.lan", "", "").parse().unwrap();
        assert_eq!((name.as_str(), t.user, t.port), ("devbox.lan", None, None));
        let (name, t) = form(" Dev ", "devbox", "me", "2222").parse().unwrap();
        assert_eq!((name.as_str(), t.user.as_deref(), t.port), ("Dev", Some("me"), Some(2222)));
        for (f, want) in [
            (form("", "", "", ""), "Pick a server"),
            (form("", "-oProxyCommand=x", "", ""), "Type one name"),
            (form("", "dev box", "", ""), "Type one name"),
            (form("", "devbox", "", "0"), "Type a port"),
            (form("", "devbox", "", "70000"), "Type a port"),
            (form("", "devbox", "-l", ""), "Type one user"),
            (HostForm { socket: "run/x.sock".into(), ..form("", "devbox", "", "") }, "Type a full path"),
        ] {
            let e = f.parse().unwrap_err().to_string();
            assert!(e.starts_with(want), "{e}");
        }
        let mut h = KnownHost::new("devbox".into(), "/s.sock".into());
        h.user = Some("me".into());
        h.port = Some(2222);
        let hosts = Hosts { selected: Some(h.id.clone()), hosts: vec![h.clone()] };
        let text = toml::to_string(&hosts).unwrap();
        assert!(text.contains("id = \"h_devbox\"") && text.contains("user = \"me\"") && text.contains("port = 2222"), "{text}");
        assert_eq!(toml::from_str::<Hosts>(&text).unwrap(), hosts);
        // The same login is one Host, whatever its name. Another user is another Host.
        assert!(check_unique(&hosts, "x", &h.target(), None).unwrap_err().to_string().contains("in the list already"));
        assert!(check_unique(&hosts, "x", &h.target(), Some("h_devbox")).is_ok());
        assert!(check_unique(&hosts, "DEVBOX", &SshTarget::new("devbox", "/s.sock"), None).unwrap_err().to_string().contains("this name"));
        assert!(check_unique(&hosts, "other", &SshTarget::new("devbox", "/s.sock"), None).is_ok());
        // A file without an id is not a shape any release wrote: it does not load.
        assert!(toml::from_str::<Hosts>("[[host]]\ndestination = \"d\"\nsocket = \"/s\"\n").is_err());
    }

    /// Review P1: accept A, then B at the same endpoint. A new selection from
    /// the file, as after picking another Host or restarting, still refuses B
    /// and A's clipboard permission does not pass to B.
    #[test]
    fn the_first_identity_is_pinned_and_only_the_person_changes_it() {
        let dir = std::env::temp_dir().join(format!("skiff-hosts-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("hosts.toml");
        let id = |s: &str| HostId::try_from(s.to_string()).unwrap();
        save(&file, &Hosts { selected: Some("h_devbox".into()), hosts: vec![KnownHost::new("devbox".into(), "/s.sock".into())] }).unwrap();
        pin_first(&file, &dev(), &id("A")).unwrap();
        pin_first(&file, &dev(), &id("A")).unwrap();
        let changed = pin_first(&file, &dev(), &id("B")).unwrap_err();
        assert!(matches!(changed.downcast_ref(), Some(skiff_host_api::Error::IdentityChanged { .. })), "{changed:#}");
        let hosts = load(&file).unwrap();
        assert_eq!(hosts.hosts[0].host_id.as_deref(), Some("A"));
        let local = || HostSelection::new("/l.sock".into(), "/id".into());
        assert_eq!(hosts.selection(local).pinned(), Some(id("A")));

        // Clipboard needs a live identity, and is given to that one only.
        assert!(set_clipboard(&file, "h_devbox", None, true).is_err());
        set_clipboard(&file, "h_devbox", Some(&id("A")), true).unwrap();
        assert!(load(&file).unwrap().hosts[0].clipboard_for(&id("A")));
        let accepted = accept_identity(&file, "h_devbox", &id("B")).unwrap();
        assert_eq!(accepted.hosts[0].host_id.as_deref(), Some("B"));
        assert!(!accepted.hosts[0].clipboard_for(&id("B")), "the old identity's permission is gone");
        assert_eq!(load(&file).unwrap().selection(local).pinned(), Some(id("B")));
        // A file whose permission names another identity allows nothing.
        let foreign = KnownHost { host_id: Some("B".into()), clipboard_host: Some("A".into()), ..KnownHost::new("x".into(), "/s".into()) };
        assert!(!foreign.clipboard_for(&id("A")) && !foreign.clipboard_for(&id("B")));
        // The old bool key from before pinning is ignored, not trusted.
        std::fs::write(&file, "[[host]]\nid = \"h_devbox\"\nname = \"devbox\"\ndestination = \"devbox\"\nsocket = \"/s.sock\"\nclipboard = true\n").unwrap();
        assert!(!load(&file).unwrap().hosts[0].clipboard_for(&id("A")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Residual P2: window 1 stays connected to B with no permission; window
    /// 2, another app run, accepts C at the same destination and allows C.
    /// Window 1's page reloads (its live B connection survives): neither the
    /// list nor a write allows B, and window 1 cannot grant B or C.
    #[test]
    fn clipboard_follows_the_live_identity_not_another_windows_grant() {
        let dir = std::env::temp_dir().join(format!("skiff-hosts-clip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("hosts.toml");
        let id = |s: &str| HostId::try_from(s.to_string()).unwrap();
        update(&file, |h| {
            h.selected = Some("h_devbox".into());
            h.hosts.push(KnownHost::new("devbox".into(), "/s.sock".into()));
            Ok(())
        })
        .unwrap();
        pin_first(&file, &dev(), &id("B")).unwrap();
        let live_b = id("B");
        // Window 2.
        accept_identity(&file, "h_devbox", &id("C")).unwrap();
        set_clipboard(&file, "h_devbox", Some(&id("C")), true).unwrap();
        // Window 1 after its page reloads: its live identity is still B.
        let list = host_list(load(&file).unwrap(), Some("h_devbox".into()), Some(&live_b));
        assert!(!list.clipboard, "C's grant allowed B's writes in the list");
        assert!(!clipboard_allowed(&file, &dev(), &live_b).unwrap(), "C's grant allowed a write from B");
        assert!(set_clipboard(&file, "h_devbox", Some(&live_b), true).is_err(), "window 1 granted while connected to B");
        assert_eq!(load(&file).unwrap().hosts[0].clipboard_host.as_deref(), Some("C"), "window 1 changed C's grant");
        // Window 2, live on C, may write.
        assert!(host_list(load(&file).unwrap(), Some("h_devbox".into()), Some(&id("C"))).clipboard);
        assert!(clipboard_allowed(&file, &dev(), &id("C")).unwrap());
        // The page's list carries no per-Host permission to fall back on.
        let json = serde_json::to_value(host_list(load(&file).unwrap(), Some("h_devbox".into()), Some(&live_b))).unwrap();
        assert!(json["hosts"][0].get("clipboard").is_none(), "{json}");
        // No identity yet: nothing allowed.
        assert!(!host_list(load(&file).unwrap(), Some("h_devbox".into()), None).clipboard);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// One store step in a child process of this test binary, released by
    /// the parent's `go` file at the same time as the others. Run only by
    /// the tests below.
    #[test]
    #[ignore]
    fn store_child() {
        let Ok(spec) = std::env::var("SKIFF_HOSTS_CHILD") else { return };
        let [file, dir, role]: [&str; 3] = spec.splitn(3, '|').collect::<Vec<_>>().try_into().unwrap();
        let (file, dir) = (Path::new(file), Path::new(dir));
        let id = |s: &str| HostId::try_from(s.to_string()).unwrap();
        std::fs::write(dir.join(format!("ready-{role}")), "").unwrap();
        while !dir.join("go").exists() {
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        let result = match role.split_once(':').unwrap() {
            ("pin", h) => pin_first(file, &dev(), &id(h)).map(drop),
            ("accept", h) => accept_identity(file, "h_devbox", &id(h)).map(drop),
            ("add", d) => update(file, |hosts| {
                hosts.hosts.push(KnownHost::new(d.into(), "/s.sock".into()));
                Ok(())
            }),
            _ => unreachable!(),
        };
        let out = match result {
            Ok(()) => "ok".to_string(),
            Err(e) if matches!(e.downcast_ref(), Some(skiff_host_api::Error::IdentityChanged { .. })) => "changed".into(),
            Err(e) => format!("error {e:#}"),
        };
        std::fs::write(dir.join(format!("out-{role}")), out).unwrap();
    }

    /// Runs `roles` in separate processes against one file, all released at
    /// once, and returns what each reported.
    fn race(name: &str, trial: usize, roles: &[&str]) -> (PathBuf, Vec<String>) {
        let dir = std::env::temp_dir().join(format!("skiff-hosts-race-{name}-{}-{trial}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hosts.toml");
        update(&file, |h| {
            h.hosts.push(KnownHost::new("devbox".into(), "/s.sock".into()));
            Ok(())
        })
        .unwrap();
        let exe = std::env::current_exe().unwrap();
        let children: Vec<_> = roles
            .iter()
            .map(|role| {
                std::process::Command::new(&exe)
                    .args(["--exact", "hosts::tests::store_child", "--ignored", "--test-threads=1"])
                    .env("SKIFF_HOSTS_CHILD", format!("{}|{}|{role}", file.display(), dir.display()))
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !roles.iter().all(|r| dir.join(format!("ready-{r}")).exists()) {
            assert!(std::time::Instant::now() < deadline, "children never got ready");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        std::fs::write(dir.join("go"), "").unwrap();
        for mut c in children {
            assert!(c.wait().unwrap().success());
        }
        let outs = roles.iter().map(|r| std::fs::read_to_string(dir.join(format!("out-{r}"))).unwrap()).collect();
        (dir, outs)
    }

    /// Residual P1: two app runs pin different first identities at once.
    /// Exactly one wins, the other is refused as a changed identity, and the
    /// file holds the winner's pin.
    #[test]
    fn a_first_pin_is_one_compare_and_set_across_processes() {
        for trial in 0..12 {
            let (dir, outs) = race("pin", trial, &["pin:A", "pin:B"]);
            let pin = load(&dir.join("hosts.toml")).unwrap().hosts[0].host_id.clone().unwrap();
            let winner = if pin == "A" { 0 } else { 1 };
            assert_eq!(outs[winner], "ok", "trial {trial}: {outs:?}, pin {pin}");
            assert_eq!(outs[1 - winner], "changed", "trial {trial}: both stored a first pin: {outs:?}, pin {pin}");
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    /// Residual P1: a first pin and the person's accepted identity at once,
    /// beside other writers. The accepted identity is never overwritten by
    /// a first pin, and no writer's change is lost.
    #[test]
    fn identity_acceptance_and_other_writers_lose_no_update_across_processes() {
        for trial in 0..8 {
            let (dir, outs) = race("accept", trial, &["pin:A", "accept:C", "add:x", "add:y"]);
            let hosts = load(&dir.join("hosts.toml")).unwrap();
            assert_eq!(hosts.find("h_devbox").unwrap().host_id.as_deref(), Some("C"), "trial {trial}: {outs:?}");
            assert!(outs[0] == "ok" || outs[0] == "changed", "trial {trial}: {outs:?}");
            assert!(outs[1..].iter().all(|o| o == "ok"), "trial {trial}: {outs:?}");
            assert!(hosts.find("h_x").is_some() && hosts.find("h_y").is_some(), "trial {trial}: an added Host was lost");
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}

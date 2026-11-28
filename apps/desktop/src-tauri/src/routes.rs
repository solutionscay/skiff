//! A command holds its Host entry for its entire lifetime, including across awaits.
use serde::{Deserialize, Serialize};
use skiff_client::host::local::HostSelection;
use skiff_host_api::HostId;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tauri::{
    ipc::{CommandArg, CommandItem, InvokeBody, InvokeError},
    AppHandle, Emitter, Manager, Runtime,
};

use crate::connection::HostState;

pub(crate) const CHANGED: &str = "The Host changed. The page subscribes again.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Route {
    pub entry: u64,
    /// The Settings entry before Hello. None for this computer.
    pub endpoint: Option<String>,
    pub host: Option<HostId>,
    pub generation: u64,
}

#[derive(Clone, Serialize)]
pub(crate) struct HostEvent<T> {
    pub route: Route,
    pub value: T,
}

#[derive(Default)]
pub(crate) struct Routes {
    table: Arc<Mutex<Table>>,
    pub switch: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct Table {
    process: Arc<crate::connection::LocalProcessState>,
    next: u64,
    remembered: HashSet<u64>,
    entries: Vec<Arc<HostState>>,
    hosts: HashMap<HostId, Arc<HostState>>,
    handle: Option<AppHandle>,
}

#[derive(Default)]
pub(crate) struct Address {
    owner: OnceLock<Weak<Mutex<Table>>>,
    route: Mutex<Option<Route>>,
}

impl Address {
    pub fn snapshot(&self) -> Option<Route> {
        self.route.lock().unwrap().clone()
    }

    pub fn get(&self) -> Route {
        self.route
            .lock()
            .unwrap()
            .clone()
            .expect("registered Host entry")
    }

    pub fn connect_log(&self) -> impl Fn(String) + Send + Sync + 'static {
        let sink = self.owner.get().and_then(Weak::upgrade).and_then(|table| {
            let handle = table.lock().unwrap().handle.clone()?;
            Some((handle, self.get()))
        });
        move |value| {
            if let Some((handle, route)) = &sink {
                let _ = handle.emit(
                    "skiff:connect-line",
                    HostEvent {
                        route: route.clone(),
                        value,
                    },
                );
            }
        }
    }

    pub fn accepted(&self, host: HostId) -> Result<(), String> {
        let Some(table) = self.owner.get().and_then(Weak::upgrade) else {
            return Ok(());
        };
        let mut table = table.lock().unwrap();
        let mut route = self.route.lock().unwrap();
        let route = route.as_mut().unwrap();
        let entry = table
            .entries
            .iter()
            .find(|a| std::ptr::eq(&a.address, self))
            .cloned()
            .ok_or(CHANGED)?;
        if table
            .hosts
            .get(&host)
            .is_some_and(|other| !Arc::ptr_eq(other, &entry))
        {
            return Err(format!("{} is already connected.", host.as_str()));
        }
        if let Some(old) = &route.host {
            table.hosts.remove(old);
        }
        route.host = Some(host.clone());
        route.generation += 1;
        table.hosts.insert(host, entry);
        if let Some(handle) = &table.handle {
            let _ = handle.emit("skiff:host-route", &*route);
        }
        Ok(())
    }
}

impl Routes {
    fn insert(
        &self,
        table: &mut Table,
        app: HostState,
        endpoint: Option<String>,
    ) -> Arc<HostState> {
        table.next += 1;
        app.address.owner.set(Arc::downgrade(&self.table)).ok();
        *app.address.route.lock().unwrap() = Some(Route {
            entry: table.next,
            endpoint,
            host: None,
            generation: 0,
        });
        let entry = Arc::new(app);
        table.entries.push(entry.clone());
        if let Some(handle) = &table.handle {
            crate::ledger::watch(handle.clone(), entry.clone());
            crate::projects_watch::watch(handle.clone(), entry.clone());
        }
        entry
    }

    pub fn start(&self, handle: AppHandle) {
        let mut table = self.table.lock().unwrap();
        table.handle = Some(handle);
        let known = crate::hosts::load(&crate::hosts::path()).unwrap_or_default();
        let ids: Vec<Option<String>> =
            std::fs::read(crate::hosts::path().with_file_name("connected-hosts.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_else(|| vec![known.selected.clone()]);
        for endpoint in ids {
            let selection = match &endpoint {
                None => crate::connection::local_selection(),
                Some(id) => match known.find(id).and_then(|h| crate::hosts::remote(h).ok()) {
                    Some(s) => s,
                    None => continue,
                },
            };
            let state = HostState::for_selection(selection, table.process.clone());
            let entry = self.insert(&mut table, state, endpoint);
            table.remembered.insert(entry.address.get().entry);
        }
        if table.entries.is_empty() {
            let state = HostState::for_selection(
                crate::connection::local_selection(),
                table.process.clone(),
            );
            self.insert(&mut table, state, None);
        }
    }

    pub fn entries(&self) -> Vec<Arc<HostState>> {
        self.table.lock().unwrap().entries.clone()
    }

    pub fn selected(&self) -> Arc<HostState> {
        self.table.lock().unwrap().entries[0].clone()
    }

    pub fn connect(&self, selection: HostSelection, endpoint: Option<String>) -> Arc<HostState> {
        let mut table = self.table.lock().unwrap();
        if let Some(entry) = table
            .entries
            .iter()
            .find(|entry| entry.address.get().endpoint == endpoint)
        {
            return entry.clone();
        }
        let state = HostState::for_selection(selection, table.process.clone());
        self.insert(&mut table, state, endpoint)
    }

    pub fn remove(&self, entry: &Arc<HostState>) -> Result<(), String> {
        self.remove_if(entry, |e| {
            e.live
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|link| !link.is_closed())
        })
    }

    fn remove_if(
        &self,
        entry: &Arc<HostState>,
        online: impl Fn(&HostState) -> bool,
    ) -> Result<(), String> {
        let mut table = self.table.lock().unwrap();
        if table.entries.len() <= 1 || !table.entries.iter().any(|e| Arc::ptr_eq(e, entry)) {
            return Err(CHANGED.into());
        }
        // A first handshake in progress cannot take the place of the last connected Host.
        if entry.address.get().host.is_some()
            && !table
                .entries
                .iter()
                .any(|e| !Arc::ptr_eq(e, entry) && online(e))
        {
            return Err(CHANGED.into());
        }
        entry
            .retired
            .store(true, std::sync::atomic::Ordering::Release);
        entry.cancel.notify_waiters();
        table.remembered.remove(&entry.address.get().entry);
        table.entries.retain(|e| !Arc::ptr_eq(e, entry));
        table.hosts.retain(|_, e| !Arc::ptr_eq(e, entry));
        Ok(())
    }

    pub fn remember(&self) -> Result<(), String> {
        let table = self.table.lock().unwrap();
        let ids: Vec<_> = table
            .entries
            .iter()
            .map(|e| e.address.get())
            .filter(|r| r.host.is_some() || table.remembered.contains(&r.entry))
            .map(|r| r.endpoint)
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let path = crate::hosts::path().with_file_name("connected-hosts.json");
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&tmp, serde_json::to_vec(&ids).unwrap())
            .and_then(|_| std::fs::rename(tmp, path))
            .map_err(|e| e.to_string())
    }

    pub fn host(&self, id: &HostId) -> Option<Arc<HostState>> {
        self.table.lock().unwrap().hosts.get(id).cloned()
    }

    pub fn resolve(&self, route: &Route) -> Option<Arc<HostState>> {
        let table = self.table.lock().unwrap();
        let entry = match &route.host {
            Some(host) => table.hosts.get(host),
            None => table
                .entries
                .iter()
                .find(|e| e.address.get().entry == route.entry),
        }?;
        (entry.address.get() == *route).then(|| entry.clone())
    }
}

#[tauri::command]
pub(crate) fn host_route(routes: tauri::State<'_, Routes>) -> Route {
    routes.selected().address.get()
}

pub(crate) struct Routed {
    pub state: Arc<HostState>,
    pub route: Route,
}
impl Deref for Routed {
    type Target = HostState;
    fn deref(&self) -> &HostState {
        &self.state
    }
}

pub(crate) fn target<R: Runtime>(message: &tauri::ipc::InvokeMessage<R>) -> Option<Routed> {
    let InvokeBody::Json(body) = message.payload() else {
        return None;
    };
    let route: Route = serde_json::from_value(body.get("route")?.clone()).ok()?;
    let webview = message.webview();
    let routes = webview.state::<Routes>();
    let entry = routes.resolve(&route).or_else(|| {
        // A startup probe can finish Hello before these read-only calls arrive.
        if !matches!(message.command(), "daemon_status" | "list_hosts") {
            return None;
        }
        let entry = routes
            .entries()
            .into_iter()
            .find(|e| e.address.get().entry == route.entry)?;
        let active = entry.address.get();
        (active.entry == route.entry
            && active.endpoint == route.endpoint
            && (route.host.is_none() || active.host == route.host))
            .then_some(entry)
    })?;
    Some(Routed {
        state: entry,
        route,
    })
}

impl<'a, R: Runtime> CommandArg<'a, R> for Routed {
    fn from_command(item: CommandItem<'a, R>) -> Result<Self, InvokeError> {
        target(item.message).ok_or_else(|| CHANGED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_hosts_reconnect_cancel_and_disconnect_independently() {
        let routes = Routes::default();
        let endpoint = || HostSelection::new("/unused".into(), "/unused-id".into());
        let a = routes.connect(endpoint(), None);
        let b = routes.connect(endpoint(), Some("ssh".into()));
        let pending = a.address.get();
        let id = HostId::try_from("A".to_owned()).unwrap();
        a.address.accepted(id.clone()).unwrap();
        let first = a.address.get();
        assert!(routes.resolve(&pending).is_none());
        assert!(b.address.accepted(id.clone()).is_err());
        b.address
            .accepted(HostId::try_from("B".to_owned()).unwrap())
            .unwrap();
        let second = b.address.get();
        a.address.accepted(id).unwrap();
        assert!(routes.resolve(&first).is_none());
        assert!(Arc::ptr_eq(&routes.resolve(&second).unwrap(), &b));
        let pending = routes.connect(endpoint(), Some("cancel".into()));
        let old = pending.address.get();
        routes.remove(&pending).unwrap();
        assert!(routes.resolve(&old).is_none());
        assert!(pending
            .address
            .accepted(HostId::try_from("C".to_owned()).unwrap())
            .is_err());
        assert!(routes.resolve(&a.address.get()).is_some());
        assert!(routes.resolve(&second).is_some());
        assert!(routes.remove_if(&b, |_| false).is_err());
        routes.remove_if(&b, |_| true).unwrap();
        assert!(routes.resolve(&second).is_none());
        assert!(routes.resolve(&a.address.get()).is_some());
        assert!(routes.remove_if(&a, |_| true).is_err());
    }
}

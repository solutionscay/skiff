//! A command holds its Host entry for its entire lifetime, including across awaits.
use serde::{Deserialize, Serialize};
use skiff_client::host::local::HostSelection;
use skiff_host_api::HostId;
use std::{
    collections::HashMap,
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
    selected: Option<Arc<HostState>>,
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

    pub fn accepted(&self, host: HostId) {
        let Some(table) = self.owner.get().and_then(Weak::upgrade) else {
            return;
        };
        let mut table = table.lock().unwrap();
        let mut route = self.route.lock().unwrap();
        let route = route.as_mut().unwrap();
        let Some(entry) = table
            .selected
            .clone()
            .filter(|a| std::ptr::eq(&a.address, self))
        else {
            return;
        };
        if let Some(old) = &route.host {
            table.hosts.remove(old);
        }
        route.host = Some(host.clone());
        route.generation += 1;
        table.hosts.insert(host, entry);
        if let Some(handle) = &table.handle {
            let _ = handle.emit("skiff:host-route", &*route);
        }
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
        table.selected = Some(entry.clone());
        entry
    }

    pub fn start(&self, handle: AppHandle) {
        let mut table = self.table.lock().unwrap();
        table.handle = Some(handle);
        let (selection, endpoint) = crate::hosts::load(&crate::hosts::path())
            .map(|hosts| {
                let selection = crate::hosts::selected(&hosts, crate::connection::local_selection);
                let endpoint = if selection.is_local() {
                    None
                } else {
                    hosts.selected
                };
                (selection, endpoint)
            })
            .unwrap_or_else(|e| {
                eprintln!("hosts.toml: {e:#}; showing this computer");
                (crate::connection::local_selection(), None)
            });
        let app = HostState::for_selection(selection, table.process.clone());
        self.insert(&mut table, app, endpoint);
    }

    pub fn selected(&self) -> Arc<HostState> {
        self.table
            .lock()
            .unwrap()
            .selected
            .clone()
            .expect("initial Host entry")
    }

    pub fn replace(&self, selection: HostSelection, endpoint: Option<String>) -> Arc<HostState> {
        let mut table = self.table.lock().unwrap();
        if let Some(old) = table.selected.as_ref() {
            old.retired
                .store(true, std::sync::atomic::Ordering::Release);
            if let Some(handle) = &table.handle {
                let _ = handle.emit("skiff:host-removed", old.address.get());
            }
        }
        table.hosts.clear();
        let app = HostState::for_selection(selection, table.process.clone());
        self.insert(&mut table, app, endpoint)
    }

    pub fn host(&self, id: &HostId) -> Option<Arc<HostState>> {
        self.table.lock().unwrap().hosts.get(id).cloned()
    }

    pub fn resolve(&self, route: &Route) -> Option<Arc<HostState>> {
        let table = self.table.lock().unwrap();
        let entry = match &route.host {
            Some(host) => table.hosts.get(host),
            None => table.selected.as_ref(),
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
        let entry = routes.selected();
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
    fn reconnect_and_switch_invalidate_the_old_route() {
        let routes = Routes::default();
        let endpoint = || HostSelection::new("/unused".into(), "/unused-id".into());
        let a = routes.replace(endpoint(), None);
        let pending = a.address.get();
        let id = HostId::try_from("A".to_owned()).unwrap();
        a.address.accepted(id.clone());
        let first = a.address.get();
        assert!(routes.resolve(&pending).is_none());
        let other = Route {
            host: Some(HostId::try_from("B".to_owned()).unwrap()),
            ..first.clone()
        };
        assert!(routes.resolve(&other).is_none());
        assert!(Arc::ptr_eq(&routes.resolve(&first).unwrap(), &a));
        a.address.accepted(id.clone());
        assert!(routes.resolve(&first).is_none());
        let second = a.address.get();
        let b = routes.replace(endpoint(), None);
        // An old handshake finishing after the switch cannot claim the new entry.
        a.address.accepted(id.clone());
        b.address.accepted(id);
        assert!(routes.resolve(&second).is_none());
        assert!(Arc::ptr_eq(&routes.resolve(&b.address.get()).unwrap(), &b));
    }
}

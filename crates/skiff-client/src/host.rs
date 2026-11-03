//! Host contract client, with in-process local sources for M1a–M1g.
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use skiff_host_api::{Capability, Error, Event, Hello, Request, Response};
use tokio::sync::{broadcast, watch};

use crate::transport::Transport;
use engine::{Engine, Reply, MAX_FRAME, MAX_PENDING};

pub mod bundle;
pub mod check;
pub mod data;
pub mod install;
pub mod keys;
pub mod known;
mod engine;
mod inventory;
mod lifecycle;
pub mod local;
pub mod ssh;
pub mod ssh_config;
mod sources;
mod terminal;
mod updates;

pub use skiff_host_api::TerminalFrame;
/// Core rows in the Host contract's shapes and back, for a service that
/// answers with them (`skiff-host`) and the app that reads them.
pub use inventory::{project_session, restore_session};
pub use lifecycle::{project_agent, restore_agent};
use terminal::OutputInner;
pub use sources::{
    AgentSource, GroupSource, InventorySource, LifecycleSource, ScreenSource, SessionUpdates,
    StatusSource, UpdateGap, UpdateSource, UpdateStream,
};
use sources::UpdatesInner;
pub use terminal::{OutputGap, OutputStream, TerminalOutput, TerminalSource};


/// The in-process sources one local adapter offers. Each optional source
/// lets the adapter advertise exactly its own capability.
pub struct LocalSources {
    pub status: Arc<dyn StatusSource>,
    pub screen: Option<Arc<dyn ScreenSource>>,
    pub inventory: Option<Arc<dyn InventorySource>>,
    pub updates: Option<Arc<dyn UpdateSource>>,
    pub terminal: Option<Arc<dyn TerminalSource>>,
    pub lifecycle: Option<Arc<dyn LifecycleSource>>,
    pub groups: Option<Arc<dyn GroupSource>>,
    pub agents: Option<Arc<dyn AgentSource>>,
}

#[allow(clippy::large_enum_variant, reason = "one Backend per Host connection, never in a collection")]
enum Backend {
    Wire(Engine),
    Local {
        source: Arc<dyn StatusSource>,
        screen: Option<Arc<dyn ScreenSource>>,
        inventory: Option<Arc<dyn InventorySource>>,
        updates: Option<Arc<dyn UpdateSource>>,
        terminal: Option<Arc<dyn TerminalSource>>,
        lifecycle: Option<Arc<dyn LifecycleSource>>,
        groups: Option<Arc<dyn GroupSource>>,
        agents: Option<Arc<dyn AgentSource>>,
        read_slots: tokio::sync::Semaphore,
        /// Attach/detach/resize: bounded, apart from reads and status.
        terminal_slots: tokio::sync::Semaphore,
        closed: watch::Sender<bool>,
        slots: tokio::sync::Semaphore,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuity {
    FirstConnection,
    SameInstance,
    NewInstance,
}

/// One negotiated Host, independent of endpoint/socket/process ownership.
/// Dropping a request future cancels its local wait; it never replays a request.
/// Dropping/disconnecting this connection closes transport tasks, not host sessions.
pub struct HostConnection {
    backend: Backend,
    hello: Hello,
    continuity: Continuity,
}
impl HostConnection {
    pub async fn connect<T: Transport>(
        transport: T,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::connect_with_timeout(transport, previous, Duration::from_secs(20)).await
    }

    pub async fn connect_with_timeout<T: Transport>(
        transport: T,
        previous: Option<&Hello>,
        timeout: Duration,
    ) -> Result<Self, Error> {
        let engine = Engine::new(transport, timeout);
        let Response::Hello { hello } = engine
            .request(Request::Hello, MAX_PENDING, MAX_FRAME)
            .await?
        else {
            unreachable!()
        };
        Self::negotiated(Backend::Wire(engine), hello, previous)
    }

    fn negotiated(backend: Backend, hello: Hello, previous: Option<&Hello>) -> Result<Self, Error> {
        hello.validate()?;
        let continuity = match previous {
            None => Continuity::FirstConnection,
            Some(old) if old.host.host_id != hello.host.host_id => {
                return Err(Error::IdentityChanged {
                    expected: old.host.host_id.clone(),
                    received: hello.host.host_id.clone(),
                })
            }
            Some(old) if old.instance_id != hello.instance_id => Continuity::NewInstance,
            Some(_) => Continuity::SameInstance,
        };
        Ok(Self {
            backend,
            hello,
            continuity,
        })
    }

    /// Bootstrap an in-process local status source. It is never a wire Hello
    /// from legacy skiffd, and cannot claim the session-plane capability.
    pub fn local_status(
        source: Arc<dyn StatusSource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: None,
                inventory: None,
                updates: None,
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// M1b adds only session_screen; broad sessions@1 stays unavailable locally.
    pub fn local_screen(
        source: Arc<dyn StatusSource>,
        screen: Arc<dyn ScreenSource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: Some(screen),
                inventory: None,
                updates: None,
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// M1c adds only the lossless read-only inventory, not sessions@1.
    pub fn local_inventory(
        source: Arc<dyn StatusSource>,
        screen: Arc<dyn ScreenSource>,
        inventory: Arc<dyn InventorySource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: Some(screen),
                inventory: Some(inventory),
                updates: None,
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// M1d adds only the read-only session-update stream; no output, input,
    /// subscription request or lifecycle operation.
    pub fn local_updates(
        source: Arc<dyn StatusSource>,
        screen: Arc<dyn ScreenSource>,
        inventory: Arc<dyn InventorySource>,
        updates: Arc<dyn UpdateSource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: Some(screen),
                inventory: Some(inventory),
                updates: Some(updates),
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// An in-process local adapter. It may advertise only the capabilities
    /// its sources serve; M1e adds `session_terminal@1` with a TerminalSource,
    /// M1g `session_lifecycle@1` and `session_groups@1`, M3 `session_agents@1`,
    /// and `program_status@1` with the inventory, when skiffd reads OSC 7501.
    /// Never `sessions@1`.
    pub fn local(
        sources: LocalSources,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        let LocalSources {
            status: source,
            screen,
            inventory,
            updates,
            terminal,
            lifecycle,
            groups,
            agents,
        } = sources;
        hello.validate()?;
        if hello.capabilities.iter().any(|c| {
            c.name != skiff_host_api::CONNECTION_STATUS
                && !(screen.is_some() && c.name == skiff_host_api::SESSION_SCREEN)
                && !(inventory.is_some() && c.name == skiff_host_api::SESSION_INVENTORY)
                && !(updates.is_some() && c.name == skiff_host_api::SESSION_UPDATES)
                && !(terminal.is_some() && c.name == skiff_host_api::SESSION_TERMINAL)
                && !(lifecycle.is_some() && c.name == skiff_host_api::SESSION_LIFECYCLE)
                && !(groups.is_some() && c.name == skiff_host_api::SESSION_GROUPS)
                && !(agents.is_some() && c.name == skiff_host_api::SESSION_AGENTS)
                // A field of the inventory and its updates, from the same source.
                && !(inventory.is_some() && c.name == skiff_host_api::PROGRAM_STATUS)
        }) {
            return Err(Error::MalformedContract {
                message: "local adapter cannot advertise capabilities without a source".into(),
            });
        }
        let limit = hello.limits.max_in_flight.min(MAX_PENDING);
        let slots = tokio::sync::Semaphore::new(limit);
        // Reserve one slot for status while Mission control fans out reads.
        let read_slots = tokio::sync::Semaphore::new(limit.saturating_sub(1).max(1));
        Self::negotiated(
            Backend::Local {
                source,
                screen,
                inventory,
                updates,
                terminal,
                lifecycle,
                groups,
                agents,
                read_slots,
                terminal_slots: tokio::sync::Semaphore::new(limit),
                closed: watch::channel(false).0,
                slots,
            },
            hello,
            previous,
        )
    }

    pub fn hello(&self) -> &Hello {
        &self.hello
    }
    /// NewInstance requires new subscriptions/snapshots; M0 does not restore them automatically.
    pub fn continuity(&self) -> Continuity {
        self.continuity
    }

    pub async fn request(&self, request: Request) -> Reply {
        let capability = request
            .capability()
            .ok_or_else(|| Error::MalformedContract {
                message: "Hello is handshake-only".into(),
            })?;
        self.hello.require(&capability)?;
        match &self.backend {
            Backend::Wire(engine) => {
                engine
                    .request(
                        request,
                        self.hello.limits.max_in_flight.min(MAX_PENDING),
                        self.hello.limits.max_frame_bytes.min(MAX_FRAME),
                    )
                    .await
            }
            Backend::Local {
                source,
                screen,
                inventory,
                terminal,
                lifecycle,
                groups,
                agents,
                read_slots,
                terminal_slots,
                closed,
                slots,
                ..
            } => {
                let mut closing = closed.subscribe();
                if *closing.borrow() {
                    return Err(Error::TransportLost);
                }
                let operation: Pin<Box<dyn Future<Output = Reply> + Send + '_>> = match &request {
                    Request::ConnectionStatus => Box::pin(async {
                        let _slot = slots.try_acquire().map_err(|_| Error::HostBusy)?;
                        source.status().await
                    }),
                    Request::SessionScreen { session, rows } => {
                        let screen =
                            screen
                                .as_ref()
                                .ok_or_else(|| Error::UnsupportedCapability {
                                    capability: capability.clone(),
                                })?;
                        Box::pin(async move {
                            // Mission control fans out every visible screen. Queue
                            // inside the existing deadline instead of introducing
                            // HostBusy errors for boards with more than 64 panes.
                            let _read = read_slots
                                .acquire()
                                .await
                                .map_err(|_| Error::TransportLost)?;
                            let _slot = slots.acquire().await.map_err(|_| Error::TransportLost)?;
                            screen.screen(session.clone(), *rows).await
                        })
                    }
                    Request::SessionInventory => {
                        let inventory =
                            inventory
                                .as_ref()
                                .ok_or_else(|| Error::UnsupportedCapability {
                                    capability: capability.clone(),
                                })?;
                        Box::pin(async move {
                            let _read = read_slots
                                .acquire()
                                .await
                                .map_err(|_| Error::TransportLost)?;
                            let _slot = slots.acquire().await.map_err(|_| Error::TransportLost)?;
                            inventory.inventory().await
                        })
                    }
                    Request::TerminalAttach { .. }
                    | Request::TerminalDetach { .. }
                    | Request::TerminalResize { .. }
                    | Request::TerminalInput { .. } => {
                        let terminal =
                            terminal
                                .as_ref()
                                .ok_or_else(|| Error::UnsupportedCapability {
                                    capability: capability.clone(),
                                })?;
                        let request = &request;
                        Box::pin(async move {
                            let _slot = terminal_slots
                                .acquire()
                                .await
                                .map_err(|_| Error::TransportLost)?;
                            match request {
                                Request::TerminalAttach { session } => {
                                    terminal.attach(session.clone()).await
                                }
                                Request::TerminalDetach { session } => {
                                    terminal.detach(session.clone()).await
                                }
                                Request::TerminalResize {
                                    session,
                                    cols,
                                    rows,
                                } => terminal.resize(session.clone(), *cols, *rows).await,
                                Request::TerminalInput {
                                    session,
                                    data,
                                    paste,
                                } => terminal
                                    .input(session, data.clone(), *paste)
                                    .map(|()| Response::Ok),
                                _ => unreachable!(),
                            }
                        })
                    }
                    Request::SessionCreate { .. }
                    | Request::SessionKill { .. }
                    | Request::SessionRename { .. }
                    | Request::SessionSetTheme { .. }
                    | Request::SessionSeen { .. } => {
                        let lifecycle =
                            lifecycle
                                .as_ref()
                                .ok_or_else(|| Error::UnsupportedCapability {
                                    capability: capability.clone(),
                                })?;
                        let request = request.clone();
                        // The read lane: a closing project ends its sessions at
                        // once; they wait for a slot within the deadline, and
                        // the status reservation stays.
                        Box::pin(async move {
                            let _read = read_slots
                                .acquire()
                                .await
                                .map_err(|_| Error::TransportLost)?;
                            let _slot = slots.acquire().await.map_err(|_| Error::TransportLost)?;
                            lifecycle.lifecycle(request).await
                        })
                    }
                    Request::GroupList
                    | Request::GroupSave { .. }
                    | Request::GroupDelete { .. }
                    | Request::GroupOrder { .. } => {
                        let groups =
                            groups
                                .as_ref()
                                .ok_or_else(|| Error::UnsupportedCapability {
                                    capability: capability.clone(),
                                })?;
                        let request = request.clone();
                        Box::pin(async move {
                            let _read = read_slots
                                .acquire()
                                .await
                                .map_err(|_| Error::TransportLost)?;
                            let _slot = slots.acquire().await.map_err(|_| Error::TransportLost)?;
                            groups.groups(request).await
                        })
                    }
                    Request::AgentList => {
                        let agents =
                            agents
                                .as_ref()
                                .ok_or_else(|| Error::UnsupportedCapability {
                                    capability: capability.clone(),
                                })?;
                        Box::pin(async move {
                            let _read = read_slots
                                .acquire()
                                .await
                                .map_err(|_| Error::TransportLost)?;
                            let _slot = slots.acquire().await.map_err(|_| Error::TransportLost)?;
                            agents.agents().await
                        })
                    }
                    _ => return Err(Error::UnsupportedCapability { capability }),
                };
                let reply = tokio::select! {
                    biased;
                    _ = closing.changed() => return Err(Error::TransportLost),
                    reply = tokio::time::timeout(Duration::from_secs(20), operation) => reply.map_err(|_| Error::TimedOut)??,
                };
                if !request.accepts(&reply) {
                    return Err(Error::MalformedContract {
                        message: "reply does not match request".into(),
                    });
                }
                Ok(reply)
            }
        }
    }

    /// Every event on the wire. Listen before requesting TerminalAttach: a
    /// snapshot may precede its reply. Lagged means discard queued output
    /// and attach again. A local backend has no wire.
    pub fn events(&self) -> Result<broadcast::Receiver<Event>, Error> {
        match &self.backend {
            Backend::Wire(engine) => {
                let s = engine.state.inner.lock().unwrap();
                s.events
                    .as_ref()
                    .map(|events| events.subscribe())
                    .ok_or_else(|| s.failure.clone().unwrap_or(Error::TransportLost))
            }
            Backend::Local { .. } => Err(Error::UnsupportedCapability {
                capability: skiff_host_api::Capability::session_terminal(),
            }),
        }
    }

    /// Session updates after this call, in order; take it before reading the
    /// inventory it amends. No backlog or uncertain update is replayed.
    /// Requires `session_updates@1` on both backends; never falls back.
    pub fn session_updates(&self) -> Result<SessionUpdates, Error> {
        self.hello.require(&Capability::session_updates())?;
        match &self.backend {
            Backend::Wire(engine) => {
                let s = engine.state.inner.lock().unwrap();
                s.events
                    .as_ref()
                    .map(|events| SessionUpdates(UpdatesInner::Wire(events.subscribe())))
                    .ok_or_else(|| s.failure.clone().unwrap_or(Error::TransportLost))
            }
            Backend::Local {
                updates, closed, ..
            } => {
                let updates = updates
                    .as_ref()
                    .ok_or_else(|| Error::UnsupportedCapability {
                        capability: Capability::session_updates(),
                    })?;
                let closed = closed.subscribe();
                if *closed.borrow() {
                    return Err(Error::TransportLost);
                }
                Ok(SessionUpdates(UpdatesInner::Local {
                    stream: updates.updates(),
                    closed,
                }))
            }
        }
    }

    /// The receiver for one session's terminal frames from now on. Take it
    /// before requesting `terminal_attach`: the snapshot can precede the
    /// reply. On the local adapter it replaces an earlier receiver for the
    /// same session. Requires `session_terminal@1`; never falls back.
    pub fn terminal_output(&self, session: &str) -> Result<TerminalOutput, Error> {
        self.hello.require(&Capability::session_terminal())?;
        match &self.backend {
            Backend::Wire(engine) => {
                let s = engine.state.inner.lock().unwrap();
                s.events
                    .as_ref()
                    .map(|events| {
                        TerminalOutput(OutputInner::Wire {
                            session: session.into(),
                            events: events.subscribe(),
                        })
                    })
                    .ok_or_else(|| s.failure.clone().unwrap_or(Error::TransportLost))
            }
            Backend::Local {
                terminal, closed, ..
            } => {
                let terminal = terminal
                    .as_ref()
                    .ok_or_else(|| Error::UnsupportedCapability {
                        capability: Capability::session_terminal(),
                    })?;
                let closed = closed.subscribe();
                if *closed.borrow() {
                    return Err(Error::TransportLost);
                }
                Ok(TerminalOutput(OutputInner::Local {
                    stream: terminal.output(session),
                    closed,
                }))
            }
        }
    }

    /// Queues terminal input without waiting for a reply or a lock held
    /// across I/O, so calls from one thread stay in order. An error means
    /// this input was not sent; it is never queued for a later retry.
    /// Requires `session_terminal@1`; never falls back.
    pub fn terminal_input(&self, session: &str, data: Vec<u8>, paste: bool) -> Result<(), Error> {
        self.hello.require(&Capability::session_terminal())?;
        match &self.backend {
            Backend::Wire(engine) => engine.send_now(
                Request::TerminalInput {
                    session: session.into(),
                    data,
                    paste,
                },
                self.hello.limits.max_frame_bytes.min(MAX_FRAME),
            ),
            Backend::Local {
                terminal, closed, ..
            } => {
                let terminal = terminal
                    .as_ref()
                    .ok_or_else(|| Error::UnsupportedCapability {
                        capability: Capability::session_terminal(),
                    })?;
                if *closed.borrow() {
                    return Err(Error::TransportLost);
                }
                terminal.input(session, data, paste)
            }
        }
    }

    /// True once this connection failed, was disconnected, or its local
    /// source lost its transport. No round trip.
    pub fn is_closed(&self) -> bool {
        match &self.backend {
            Backend::Wire(engine) => engine.state.inner.lock().unwrap().failure.is_some(),
            Backend::Local { source, closed, .. } => *closed.borrow() || source.is_lost(),
        }
    }

    pub fn disconnect(&self) {
        match &self.backend {
            Backend::Wire(engine) => {
                engine.state.fail(Error::TransportLost);
                for task in &engine.tasks {
                    task.abort();
                }
            }
            Backend::Local { closed, .. } => {
                closed.send_replace(true);
            }
        }
    }
}

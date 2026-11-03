//! Host contract client, with in-process local sources for M1a–M1g.
use std::{sync::Arc, time::Duration};

use skiff_host_api::{Capability, Error, Event, Hello, Request, Response};
use tokio::sync::{broadcast, watch};

use crate::transport::Transport;
use engine::{Engine, MAX_FRAME, MAX_PENDING};

pub mod bundle;
pub mod check;
pub mod data;
pub mod install;
pub mod keys;
pub mod known;
mod engine;
mod inventory;
mod lifecycle;
mod local_adapter;
mod request;
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
pub use local_adapter::LocalSources;
pub use sources::{
    AgentSource, GroupSource, InventorySource, LifecycleSource, ScreenSource, SessionUpdates,
    StatusSource, UpdateGap, UpdateSource, UpdateStream,
};
use sources::UpdatesInner;
pub use terminal::{OutputGap, OutputStream, TerminalOutput, TerminalSource};


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

    pub fn hello(&self) -> &Hello {
        &self.hello
    }
    /// NewInstance requires new subscriptions/snapshots; M0 does not restore them automatically.
    pub fn continuity(&self) -> Continuity {
        self.continuity
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

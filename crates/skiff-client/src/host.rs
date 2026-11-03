//! Host contract client, with in-process local sources for M1a–M1g.
use std::{sync::Arc, time::Duration};

use skiff_host_api::{Error, Hello, Request, Response};
use tokio::sync::watch;

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
mod subscribe;
mod terminal;
mod updates;

pub use skiff_host_api::TerminalFrame;
/// Core rows in the Host contract's shapes and back, for a service that
/// answers with them (`skiff-host`) and the app that reads them.
pub use inventory::{project_session, restore_session};
pub use lifecycle::{project_agent, restore_agent};
pub use local_adapter::LocalSources;
pub use sources::{
    AgentSource, GroupSource, InventorySource, LifecycleSource, ScreenSource, SessionUpdates,
    StatusSource, UpdateGap, UpdateSource, UpdateStream,
};
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

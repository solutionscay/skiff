//! The in-process source traits and the session-update receiver.
use std::{future::Future, pin::Pin};

use skiff_host_api::{Error, Event, Request, Response, SessionUpdate};
use tokio::sync::{broadcast, watch};

/// The one in-process M1a operation; no concrete socket/process type in the seam.
/// ScreenSource adds only the M1b text snapshot; other families stay on wire.
pub trait StatusSource: Send + Sync {
    fn status(&self) -> Pin<Box<dyn Future<Output = Result<Response, Error>> + Send + '_>>;
    /// True once the underlying connection is known to be gone. No round trip.
    fn is_lost(&self) -> bool {
        false
    }
}

/// One read-only text snapshot, independent of socket/core session types.
pub trait ScreenSource: Send + Sync {
    fn screen(
        &self,
        session: String,
        rows: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Response, Error>> + Send + '_>>;
}

/// Read-only desktop inventory, independent of core session lifecycle types.
pub trait InventorySource: Send + Sync {
    fn inventory(&self) -> Pin<Box<dyn Future<Output = Result<Response, Error>> + Send + '_>>;
}

/// M1g: create, kill, rename, theme and seen of one session. It receives only
/// `session_lifecycle@1` requests. No retry: a timed-out create or kill may
/// have run.
pub trait LifecycleSource: Send + Sync {
    fn lifecycle(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, Error>> + Send + '_>>;
}

/// M1g: list, save and delete the Host's session groups. It receives only
/// `session_groups@1` requests.
pub trait GroupSource: Send + Sync {
    fn groups(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, Error>> + Send + '_>>;
}

/// M3: the agent CLIs the Host account can start. It receives only
/// `session_agents@1` requests. Read-only.
pub trait AgentSource: Send + Sync {
    fn agents(&self) -> Pin<Box<dyn Future<Output = Result<Response, Error>> + Send + '_>>;
}

/// Read-only non-output session events. Each call starts a receiver at the
/// live tail: nothing earlier is replayed.
pub trait UpdateSource: Send + Sync {
    fn updates(&self) -> Box<dyn UpdateStream>;
}

/// One receiver of an UpdateSource, in the source's order.
pub trait UpdateStream: Send {
    fn recv(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<SessionUpdate, UpdateGap>> + Send + '_>>;
}

/// Why a session-update receiver returned no update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateGap {
    /// A slow receiver missed this many updates. They are never replayed;
    /// the receiver continues with later ones.
    Lagged(u64),
    /// One event could not be projected. It is skipped; the receiver continues.
    Malformed(Error),
    /// The connection ended or was disconnected. Nothing more arrives.
    Closed,
}

pub(super) enum UpdatesInner {
    Wire(broadcast::Receiver<Event>),
    Local {
        stream: Box<dyn UpdateStream>,
        closed: watch::Receiver<bool>,
    },
}

/// Session updates from one HostConnection, from [`HostConnection::session_updates`].
pub struct SessionUpdates(pub(super) UpdatesInner);
impl SessionUpdates {
    pub async fn recv(&mut self) -> Result<SessionUpdate, UpdateGap> {
        match &mut self.0 {
            UpdatesInner::Wire(rx) => loop {
                match rx.recv().await {
                    Ok(Event::SessionUpdate { update }) => return Ok(*update),
                    // Other families have their own receivers.
                    Ok(Event::TerminalSnapshot { .. } | Event::TerminalOutput { .. }) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        return Err(UpdateGap::Lagged(n))
                    }
                    Err(broadcast::error::RecvError::Closed) => return Err(UpdateGap::Closed),
                }
            },
            UpdatesInner::Local { stream, closed } => {
                if *closed.borrow_and_update() {
                    return Err(UpdateGap::Closed);
                }
                tokio::select! {
                    biased;
                    // A dropped connection ends the stream like a disconnect.
                    _ = closed.changed() => Err(UpdateGap::Closed),
                    update = stream.recv() => update,
                }
            }
        }
    }
}

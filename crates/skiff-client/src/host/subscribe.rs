//! The four `HostConnection` calls that give a caller a receiver or queue
//! input: events, session updates, terminal output and terminal input.
use skiff_host_api::{Capability, Error, Event, Request};
use tokio::sync::broadcast;

use super::{
    engine::MAX_FRAME, sources::UpdatesInner, terminal::OutputInner, Backend, HostConnection,
    SessionUpdates, TerminalOutput,
};

impl HostConnection {
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
}

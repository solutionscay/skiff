//! `HostConnection::request`: the capability gate, the wire path and the
//! local dispatch.
use std::{future::Future, pin::Pin, time::Duration};

use skiff_host_api::{Error, Request, Response};

use super::{
    engine::{Reply, MAX_FRAME, MAX_PENDING},
    Backend, HostConnection,
};

impl HostConnection {
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
}

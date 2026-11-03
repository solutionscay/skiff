//! The wire transport engine: reader and writer tasks, the pending-reply
//! table and the failure state.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use skiff_host_api::{Envelope, Error, Event, Message, Request, Response};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
};

use crate::transport::Transport;

pub(super) const MAX_FRAME: usize = 8 * 1024 * 1024;
pub(super) const MAX_PENDING: usize = 64;
const EVENT_QUEUE: usize = 256;

pub(super) type Reply = Result<Response, Error>;
pub(super) struct Shared {
    pub(super) failure: Option<Error>,
    pub(super) pending: HashMap<u64, oneshot::Sender<Reply>>,
    pub(super) events: Option<broadcast::Sender<Event>>,
}
pub(super) struct State {
    pub(super) inner: Mutex<Shared>,
    closed: watch::Sender<bool>,
}
impl State {
    pub(super) fn fail(&self, error: Error) {
        let mut s = self.inner.lock().unwrap();
        if s.failure.is_some() {
            return;
        }
        s.failure = Some(error.clone());
        for (_, reply) in s.pending.drain() {
            let _ = reply.send(Err(error.clone()));
        }
        s.events.take();
        self.closed.send_replace(true);
    }
}
struct Pending {
    state: Arc<State>,
    id: u64,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.state.inner.lock().unwrap().pending.remove(&self.id);
    }
}

pub(super) struct Engine {
    pub(super) state: Arc<State>,
    /// `(id, cancellable, frame)`. Input is not cancellable: it has no waiter.
    tx: mpsc::Sender<(u64, bool, Vec<u8>)>,
    next: AtomicU64,
    pub(super) tasks: Vec<JoinHandle<()>>,
    timeout: Duration,
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.state.fail(Error::TransportLost);
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Engine {
    pub(super) fn new<T: Transport>(transport: T, timeout: Duration) -> Self {
        let (rd, mut wr) = transport.split();
        let (events, _) = broadcast::channel(EVENT_QUEUE);
        let (closed, _) = watch::channel(false);
        let state = Arc::new(State {
            inner: Mutex::new(Shared {
                failure: None,
                pending: HashMap::new(),
                events: Some(events),
            }),
            closed,
        });
        let (tx, mut rx) = mpsc::channel::<(u64, bool, Vec<u8>)>(MAX_PENDING);
        let writer_state = state.clone();
        let mut writer_closed = state.closed.subscribe();
        let writer = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    _ = writer_closed.changed() => break,
                    frame = rx.recv() => frame,
                };
                let Some((id, cancellable, line)) = frame else {
                    break;
                };

                // Cancellation can prevent a queued request, never retract an in-flight write.
                if cancellable && !writer_state.inner.lock().unwrap().pending.contains_key(&id) {
                    continue;
                }
                let written = tokio::select! {
                    _ = writer_closed.changed() => break,
                    result = wr.write_all(&line) => result,
                };
                if written.is_err() {
                    writer_state.fail(Error::TransportLost);
                    break;
                }
            }
        });
        let reader_state = state.clone();
        let mut reader_closed = state.closed.subscribe();
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(rd);
            loop {
                let mut line = Vec::new();
                let frame = tokio::select! {
                    _ = reader_closed.changed() => break,
                    result = async {
                        (&mut reader).take(MAX_FRAME as u64 + 1).read_until(b'\n', &mut line).await
                    } => result,
                };
                match frame {
                    Ok(0) | Err(_) => {
                        reader_state.fail(Error::TransportLost);
                        break;
                    }
                    Ok(_) if line.len() > MAX_FRAME || line.last() != Some(&b'\n') => {
                        reader_state.fail(Error::MalformedContract {
                            message: "oversize or incomplete frame".into(),
                        });
                        break;
                    }
                    Ok(_) => {}
                }
                match serde_json::from_slice::<Message>(&line) {
                    Ok(Message::Reply { id, result }) => {
                        if let Some(reply) = reader_state.inner.lock().unwrap().pending.remove(&id)
                        {
                            let _ = reply.send(result);
                        }
                    }
                    Ok(Message::Event { event }) => {
                        if let Some(events) = &reader_state.inner.lock().unwrap().events {
                            let _ = events.send(event);
                        }
                    }
                    Err(_) => {
                        reader_state.fail(Error::MalformedContract {
                            message: "unreadable Host frame".into(),
                        });
                        break;
                    }
                }
            }
        });
        Self {
            state,
            tx,
            next: AtomicU64::new(1),
            tasks: vec![reader, writer],
            timeout,
        }
    }

    pub(super) async fn request(&self, request: Request, max_pending: usize, max_frame: usize) -> Reply {
        let id = self
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Error::HostBusy)?;
        let mut line = serde_json::to_vec(&Envelope {
            id,
            request: request.clone(),
        })
        .map_err(|e| Error::MalformedContract {
            message: e.to_string(),
        })?;
        line.push(b'\n');
        if line.len() > max_frame {
            return Err(Error::MalformedContract {
                message: "request exceeds frame limit".into(),
            });
        }
        let (reply, rx) = oneshot::channel();
        {
            let mut s = self.state.inner.lock().unwrap();
            if let Some(error) = &s.failure {
                return Err(error.clone());
            }
            if s.pending.len() >= max_pending {
                return Err(Error::HostBusy);
            }
            s.pending.insert(id, reply);
        }
        let _pending = Pending {
            state: self.state.clone(),
            id,
        };
        let result = tokio::time::timeout(self.timeout, async {
            self.tx
                .send((id, true, line))
                .await
                .map_err(|_| Error::TransportLost)?;
            rx.await.map_err(|_| Error::TransportLost)?
        })
        .await
        .map_err(|_| Error::TimedOut)??;
        if !request.accepts(&result) {
            let error = Error::MalformedContract {
                message: "reply does not match request".into(),
            };
            self.state.fail(error.clone());
            return Err(error);
        }
        Ok(result)
    }

    /// Queues one frame whose reply nobody waits for, behind earlier frames.
    /// A full write queue refuses it; it is never retried.
    pub(super) fn send_now(&self, request: Request, max_frame: usize) -> Result<(), Error> {
        let id = self
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Error::HostBusy)?;
        let mut line = serde_json::to_vec(&Envelope { id, request }).map_err(|e| {
            Error::MalformedContract {
                message: e.to_string(),
            }
        })?;
        line.push(b'\n');
        if line.len() > max_frame {
            return Err(Error::MalformedContract {
                message: "request exceeds frame limit".into(),
            });
        }
        if let Some(error) = &self.state.inner.lock().unwrap().failure {
            return Err(error.clone());
        }
        self.tx.try_send((id, false, line)).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => Error::HostBusy,
            mpsc::error::TrySendError::Closed(_) => Error::TransportLost,
        })
    }
}

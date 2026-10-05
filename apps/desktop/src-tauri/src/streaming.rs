use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use skiff_core::protocol::Event;
use tauri::{ipc::{Channel, InvokeResponseBody}, State};
use tokio::sync::{broadcast, Notify};
use crate::connection::{App, ensure_client, err};

/// One write to the page holds at most this much, so xterm can yield to the
/// page between pieces instead of parsing one long chunk.
const PIECE: usize = 64 * 1024;
/// Unparsed bytes past this, and live output is dropped instead of queued.
/// xterm discards writes itself past 50 MB, and a deep queue is stale anyway.
const BEHIND_HIGH: usize = 4 * 1024 * 1024;
/// Unparsed bytes under this, and a pane that dropped output redraws from a
/// fresh snapshot.
const BEHIND_LOW: usize = 1024 * 1024;

/// Bytes sent to one page stream that its xterm has not parsed yet.
pub(crate) struct Flow {
    /// The page's stream number, so an ack from an older stream is ignored.
    stream: u32,
    unparsed: AtomicUsize,
    parsed: Notify,
}

impl Flow {
    fn send(&self, on_output: &Channel<InvokeResponseBody>, data: Vec<u8>) -> tauri::Result<()> {
        if data.len() <= PIECE {
            self.unparsed.fetch_add(data.len(), Ordering::Relaxed);
            return on_output.send(InvokeResponseBody::Raw(data));
        }
        for piece in data.chunks(PIECE) {
            self.unparsed.fetch_add(piece.len(), Ordering::Relaxed);
            on_output.send(InvokeResponseBody::Raw(piece.to_vec()))?;
        }
        Ok(())
    }
}

/// Streams raw PTY bytes for one session, starting with a screen snapshot.
/// Replaces any earlier subscription. Output the page cannot keep up with is
/// dropped; once it catches up, a snapshot redraws the screen.
#[tauri::command]
pub(crate) async fn subscribe_output(
    app: State<'_, App>,
    session: String,
    stream: u32,
    on_output: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    // Listen first: the snapshot can arrive before the subscribe reply.
    let mut rx = c.output(&session);
    c.subscribe(&session).await.map_err(err)?;
    let flow = Arc::new(Flow {
        stream,
        unparsed: AtomicUsize::new(0),
        parsed: Notify::new(),
    });
    app.flows.lock().unwrap().insert(session.clone(), flow.clone());
    let sid = session.clone();
    // Weak, so a replaced client still drops and closes `rx`.
    let client = Arc::downgrade(&c);
    let task = tauri::async_runtime::spawn(async move {
        // Output from an earlier subscription predates the snapshot. Skip it.
        let mut live = false;
        // Output was dropped because the page fell behind.
        let mut behind = false;
        loop {
            let event = tokio::select! {
                event = rx.recv() => event,
                _ = flow.parsed.notified(), if behind => {
                    if flow.unparsed.load(Ordering::Relaxed) > BEHIND_LOW {
                        continue;
                    }
                    behind = false;
                    live = false;
                    rx.clear();
                    let Some(c) = client.upgrade() else { break };
                    if c.subscribe(&sid).await.is_err() {
                        break;
                    }
                    continue;
                }
            };
            match event {
                Ok(Event::Snapshot { session, data, .. }) if session == sid => {
                    live = true;
                    if flow.send(&on_output, data).is_err() {
                        break;
                    }
                }
                Ok(Event::Output { session, data }) if live && !behind && session == sid => {
                    if flow.unparsed.load(Ordering::Relaxed) >= BEHIND_HIGH {
                        behind = true;
                        continue;
                    }
                    if flow.send(&on_output, data).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Output was lost. Ask for a fresh snapshot and skip
                    // everything before it. A pane that is behind asks
                    // once it catches up.
                    live = false;
                    rx.clear();
                    if behind {
                        continue;
                    }
                    let Some(c) = client.upgrade() else { break };
                    if c.subscribe(&sid).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    if let Some(old) = app.subs.lock().await.insert(session, task) {
        old.abort();
    }
    Ok(())
}

/// The page's xterm parsed `bytes` more of stream `stream`.
#[tauri::command]
pub(crate) fn ack_output(app: State<'_, App>, session: String, stream: u32, bytes: usize) {
    let Some(flow) = app.flows.lock().unwrap().get(&session).cloned() else { return };
    if flow.stream != stream {
        return;
    }
    let _ = flow
        .unparsed
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_sub(bytes)));
    flow.parsed.notify_one();
}

#[tauri::command]
pub(crate) async fn unsubscribe_output(app: State<'_, App>, session: String) -> Result<(), String> {
    app.flows.lock().unwrap().remove(&session);
    if let Some(task) = app.subs.lock().await.remove(&session) {
        task.abort();
    }
    let (c, _) = ensure_client(&app).await?;
    c.unsubscribe(&session).await.map_err(err)
}

/// Streams every non-output event: state changes, exits, created, removed.
/// The client routes output elsewhere, so a busy terminal cannot make this lag.
#[tauri::command]
pub(crate) async fn subscribe_events(app: State<'_, App>, on_event: Channel<Event>) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    let mut rx = c.events();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if on_event.send(e).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    if let Some(old) = app.events.lock().await.replace(task) {
        old.abort();
    }
    Ok(())
}

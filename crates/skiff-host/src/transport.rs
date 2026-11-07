//! The service's socket and connections: who owns the socket file, how many
//! requests a connection may have admitted, and the frames in and out.
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::{bail, Context};
use skiff_host_api::{Envelope, Error, Hello, Message, Response};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

use super::{answer, events::DataEvents, skipper};

/// The largest frame either side sends, newline included.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
/// Requests one connection may have admitted at once, until each reply is
/// written. A client that sends more waits: the service reads no further.
pub const MAX_IN_FLIGHT: usize = 16;

/// The socket file this service bound. Only it is ever removed:
/// [`BoundSocket::release`] checks that the path still names it.
pub struct BoundSocket {
    path: std::path::PathBuf,
    id: (u64, u64),
}

/// Binds the service socket at `path`. Only a folder it creates gets locked
/// down (`0700`); the socket is `0600`. An entry already at `path` is:
/// - a live socket: another service, so this one does not start
/// - a stale socket of this account: a crash's leftover, removed
/// - anything else (a file, a link, a folder, another account's socket):
///   refused and left as it is
pub async fn bind(path: &Path) -> anyhow::Result<(UnixListener, BoundSocket)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    if let Some(dir) = path.parent().filter(|d| !d.exists()) {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(found) => {
            if !found.file_type().is_socket() {
                bail!("{} exists and is not a socket; skiff-host leaves it as it is", path.display());
            }
            // SAFETY: geteuid takes no arguments and cannot fail.
            if found.uid() != unsafe { libc::geteuid() } {
                bail!("{} belongs to another account; skiff-host leaves it as it is", path.display());
            }
            if UnixStream::connect(path).await.is_ok() {
                bail!("skiff-host is already running at {}", path.display());
            }
            // Still the same stale socket: nothing replaced it meanwhile.
            let again = std::fs::symlink_metadata(path)?;
            if (again.dev(), again.ino()) != (found.dev(), found.ino()) {
                bail!("{} changed while skiff-host checked it", path.display());
            }
            std::fs::remove_file(path)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("check {}", path.display())),
    }
    let listener = UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let meta = std::fs::symlink_metadata(path)?;
    Ok((listener, BoundSocket { path: path.to_path_buf(), id: (meta.dev(), meta.ino()) }))
}

impl BoundSocket {
    /// Removes the socket file only if the path still names the socket this
    /// service bound: never a newer service's socket, nor anything else.
    pub fn release(self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if let Ok(m) = std::fs::symlink_metadata(&self.path) {
            // The inode alone is not proof: ext4 gives a newer socket at this
            // path the freed number. This service no longer listens, so a
            // listener that answers is another one's.
            if m.file_type().is_socket()
                && (m.dev(), m.ino()) == self.id
                && std::os::unix::net::UnixStream::connect(&self.path).is_err()
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

/// Accepts connections until the listener fails. Each one is served on its
/// own; dropping this future closes them all, as a stopped service does.
pub async fn serve(listener: UnixListener, hello: Arc<Hello>) -> std::io::Result<()> {
    let events = DataEvents::start();
    let mut connections = tokio::task::JoinSet::new();
    // Skipper's watcher is this service's to keep, as long as it runs.
    connections.spawn(skipper::supervise());
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.spawn(handle(stream, hello.clone(), events.subscribe()));
            }
            Some(_) = connections.join_next() => {}
        }
    }
}

/// Requests admitted and not yet written back or abandoned, over every
/// connection of this process. For diagnostics and tests.
pub fn admitted() -> usize {
    ADMITTED.load(Ordering::Relaxed)
}
static ADMITTED: AtomicUsize = AtomicUsize::new(0);

/// One admitted request, from its frame until its reply is written. Its
/// permit bounds what a connection holds: work, encoded replies and the
/// reply being written, together.
struct Admission(#[allow(dead_code)] OwnedSemaphorePermit);
impl Admission {
    fn new(permit: OwnedSemaphorePermit) -> Self {
        ADMITTED.fetch_add(1, Ordering::Relaxed);
        Self(permit)
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        ADMITTED.fetch_sub(1, Ordering::Relaxed);
    }
}

/// One client: newline-delimited envelopes in, replies out, in any order.
///
/// At most [`MAX_IN_FLIGHT`] requests are admitted at once, and one stays
/// admitted until its reply is written. A client that stops reading stalls
/// at that bound: no more frames are read, so nothing more is computed or
/// queued for it. The work belongs to the connection: when the client is
/// gone, or a frame is unreadable or oversize, what is left is dropped.
/// Nothing is retried.
pub async fn handle(stream: UnixStream, hello: Arc<Hello>, mut events: broadcast::Receiver<skiff_host_api::Event>) {
    let (rd, mut wr) = stream.into_split();
    let slots = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    // Never full: every queued reply holds one of the permits.
    let (tx, mut rx) = mpsc::channel::<Write>(256);
    let writer = async move {
        while let Some(item) = rx.recv().await {
            let (line, admission) = match item { Write::Reply(line, admission) => (line, Some(admission)), Write::Event(line) => (line, None) };
            if wr.write_all(&line).await.is_err() {
                return;
            }
            drop(admission);
        }
    };
    let events_tx = tx.clone();
    let mut event_writer = tokio::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => skiff_host_api::Event::DataGap { sequence: 0 },
                Err(broadcast::error::RecvError::Closed) => return,
            };
            let mut line = serde_json::to_vec(&Message::Event { event }).unwrap_or_default();
            line.push(b'\n');
            if events_tx.send(Write::Event(line)).await.is_err() { return; }
        }
    });
    let reader = async move {
        let mut work = JoinSet::new();
        let mut reader = BufReader::new(rd);
        loop {
            // Finished requests leave the set; a read is never interrupted.
            while work.try_join_next().is_some() {}
            let mut line = Vec::new();
            let read = (&mut reader).take(MAX_FRAME as u64 + 1).read_until(b'\n', &mut line).await;
            match read {
                // The client is done sending: finish what it asked for.
                Ok(0) => break,
                Err(_) => return,
                Ok(_) if line.len() > MAX_FRAME || line.last() != Some(&b'\n') => return,
                Ok(_) => {}
            }
            let Ok(Envelope { id, request }) = serde_json::from_slice::<Envelope>(&line) else {
                return;
            };
            let Ok(permit) = slots.clone().acquire_owned().await else { return };
            let admission = Admission::new(permit);
            let (tx, hello) = (tx.clone(), hello.clone());
            work.spawn(async move {
                let result = answer(request, &hello).await;
                let _ = tx.send(Write::Reply(encode(id, result), admission)).await;
            });
        }
        while work.join_next().await.is_some() {}
    };
    tokio::pin!(writer);
    tokio::select! {
        // A closed reader stops events after admitted replies drain.
        _ = reader => {
            event_writer.abort();
            let _ = event_writer.await;
            writer.as_mut().await;
        },
        _ = &mut event_writer => {},
        _ = &mut writer => event_writer.abort(),
    }
}

enum Write { Reply(Vec<u8>, Admission), Event(Vec<u8>) }

fn encode(id: u64, result: Result<Response, Error>) -> Vec<u8> {
    let mut line = serde_json::to_vec(&Message::Reply { id, result }).unwrap_or_default();
    if line.len() >= MAX_FRAME {
        line = serde_json::to_vec(&Message::Reply {
            id,
            result: Err(Error::OperationFailed { message: "the reply is larger than a frame".into() }),
        })
        .unwrap_or_default();
    }
    line.push(b'\n');
    line
}

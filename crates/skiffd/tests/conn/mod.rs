//! A connection to the daemon's request handler over a socket pair, in the
//! test's own process.

use std::{sync::Arc, time::Duration};

use skiff_core::protocol::{Response, ServerMessage};
use skiffd::session::SessionPool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{unix::OwnedReadHalf, unix::OwnedWriteHalf, UnixStream},
};

pub struct Conn {
    rd: Lines<BufReader<OwnedReadHalf>>,
    wr: OwnedWriteHalf,
}

pub fn connect(pool: &Arc<SessionPool>) -> Conn {
    let (ours, theirs) = UnixStream::pair().unwrap();
    tokio::spawn(skiffd::server::handle(theirs, pool.clone()));
    let (rd, wr) = ours.into_split();
    Conn { rd: BufReader::new(rd).lines(), wr }
}

impl Conn {
    /// Sends one request and returns its reply, skipping events.
    pub async fn ask(&mut self, json: &str) -> Response {
        self.wr.write_all(format!("{json}\n").as_bytes()).await.unwrap();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(5), self.rd.next_line()).await.unwrap().unwrap().unwrap();
            if let ServerMessage::Reply(r) = serde_json::from_str(&line).unwrap() {
                return r.response;
            }
        }
    }
}

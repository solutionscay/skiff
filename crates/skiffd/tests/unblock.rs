//! `Unblock` over the socket: it ends a wait that a send set, types
//! nothing, and refuses any other wait. A request the daemon cannot read is
//! answered with "bad request", as an older daemon answers `Unblock`.

use std::{sync::Arc, time::Duration};

use skiff_core::{
    protocol::{Response, ServerMessage},
    session::{SessionSpec, SessionState},
};
use skiffd::session::SessionPool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{unix::OwnedReadHalf, unix::OwnedWriteHalf, UnixStream},
};

struct Conn {
    rd: Lines<BufReader<OwnedReadHalf>>,
    wr: OwnedWriteHalf,
}

fn connect(pool: &Arc<SessionPool>) -> Conn {
    let (ours, theirs) = UnixStream::pair().unwrap();
    tokio::spawn(skiffd::server::handle(theirs, pool.clone()));
    let (rd, wr) = ours.into_split();
    Conn { rd: BufReader::new(rd).lines(), wr }
}

impl Conn {
    /// Sends one request and returns its reply, skipping events.
    async fn ask(&mut self, json: &str) -> Response {
        self.wr.write_all(format!("{json}\n").as_bytes()).await.unwrap();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(5), self.rd.next_line()).await.unwrap().unwrap().unwrap();
            if let ServerMessage::Reply(r) = serde_json::from_str(&line).unwrap() {
                return r.response;
            }
        }
    }
}

#[tokio::test]
async fn unblock_ends_a_send_wait_and_refuses_a_person_wait() {
    let pool = SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], ..Default::default() }).unwrap();
    let id = info.id.clone();
    let mut c = connect(&pool);
    let state = |pool: &SessionPool| {
        let i = pool.get(&id).unwrap().info();
        (i.state, i.waiting_cause, i.input_gen)
    };

    let needs = |cause: &str| format!(r#"{{"id":1,"cmd":"needs_you","session":"{id}","cause":"{cause}"}}"#);
    let unblock = format!(r#"{{"id":2,"cmd":"unblock","session":"{id}"}}"#);

    assert!(matches!(c.ask(&needs("send not confirmed: paste (the input box changed)")).await, Response::Ok));
    assert_eq!(state(&pool).0, SessionState::Waiting);
    assert!(matches!(c.ask(&unblock).await, Response::Ok));
    assert_eq!(state(&pool), (SessionState::Idle, None, 0));

    // An approval prompt or a question: refused, kept, nothing typed.
    for cause in ["prompt", "report"] {
        assert!(matches!(c.ask(&needs(cause)).await, Response::Ok));
        match c.ask(&unblock).await {
            Response::Error { message } => assert!(message.contains("waits for the person"), "{message}"),
            other => panic!("{cause}: {other:?}"),
        }
        assert_eq!(state(&pool), (SessionState::Waiting, Some(cause.into()), 0), "{cause}");
    }

    // A daemon that does not know a request says "bad request".
    match c.ask(&format!(r#"{{"id":3,"cmd":"no_such_request","session":"{id}"}}"#)).await {
        Response::Error { message } => assert!(message.starts_with("bad request"), "{message}"),
        other => panic!("{other:?}"),
    }
    pool.kill(&id).unwrap();
}

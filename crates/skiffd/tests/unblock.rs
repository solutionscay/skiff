//! `Unblock` over the socket: it ends a wait that a send set, types
//! nothing, and refuses any other wait. A request the daemon cannot read is
//! answered with "bad request", as an older daemon answers `Unblock`.

use skiff_core::{
    protocol::Response,
    session::{SessionSpec, SessionState},
};
use skiffd::session::SessionPool;

mod conn;
use conn::connect;

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

use std::time::Duration;

use skiff_core::{
    protocol::Event,
    session::{SessionSpec, SessionState},
};
use skiffd::session::SessionPool;
use tokio::sync::broadcast::error::RecvError;

#[tokio::test]
async fn shell_session_streams_output_rings_bell_and_exits() {
    let pool = SessionPool::new();
    let mut events = pool.events.subscribe();

    // The user's shell runs as is. Any other program hands the terminal to
    // that shell when it exits, and the session would not end.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let spec = SessionSpec {
        command: Some(shell.clone()),
        args: vec![
            "-c".into(),
            "sleep 0.2; printf 'hello\\a'; sleep 0.2".into(),
        ],
        ..Default::default()
    };
    let info = pool.create(spec).unwrap();
    assert_eq!(Some(info.label.as_str()), shell.rsplit('/').next());
    assert_eq!(info.state, SessionState::Working);

    let session = pool.get(&info.id).unwrap();
    let mut output = session.output.subscribe();

    let mut bytes = Vec::new();
    let mut saw_waiting = false;
    let mut exit_code = None;
    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            chunk = output.recv() => match chunk {
                Ok(c) => bytes.extend_from_slice(&c),
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => {}
            },
            ev = events.recv() => match ev {
                Ok(Event::State { session, state: SessionState::Waiting }) if session == info.id => saw_waiting = true,
                Ok(Event::Exit { session, code }) if session == info.id => { exit_code = Some(code); break; }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            _ = &mut deadline => panic!("session did not exit in time"),
        }
    }

    assert!(String::from_utf8_lossy(&bytes).contains("hello"));
    assert!(saw_waiting, "BEL should set the waiting state");
    assert_eq!(exit_code, Some(Some(0)));
    assert_eq!(pool.get(&info.id).unwrap().info().state, SessionState::Done);
}

#[tokio::test]
async fn codex_approval_prompt_marks_session_waiting_without_a_bell() {
    let pool = SessionPool::new();
    let mut events = pool.events.subscribe();
    let spec = SessionSpec {
        command: Some("sh".into()),
        args: vec![
            "-c".into(),
            "printf 'Would you like to run the following command?\\nPress enter to confirm or esc to cancel'; sleep 2".into(),
        ],
        cols: 120,
        rows: 40,
        ..Default::default()
    };
    let info = pool.create(spec).unwrap();

    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(Event::State { session, state: SessionState::Waiting }) if session == info.id => break,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => panic!("event stream closed"),
            },
            _ = &mut deadline => panic!("Codex approval prompt did not mark the session waiting"),
        }
    }

    pool.kill(&info.id).unwrap();
}

#[tokio::test]
async fn write_reaches_the_process() {
    let pool = SessionPool::new();
    let spec = SessionSpec {
        command: Some("cat".into()),
        ..Default::default()
    };
    let info = pool.create(spec).unwrap();
    let mut output = pool.get(&info.id).unwrap().output.subscribe();
    pool.write(&info.id, b"ping\n").unwrap();

    let mut bytes = Vec::new();
    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    while !String::from_utf8_lossy(&bytes).contains("ping") {
        tokio::select! {
            chunk = output.recv() => if let Ok(c) = chunk { bytes.extend_from_slice(&c) },
            _ = &mut deadline => panic!("no echo from cat"),
        }
    }
    pool.kill(&info.id).unwrap();
}

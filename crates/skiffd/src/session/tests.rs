use super::{
    now_ms, prints, topic, Front, Screen, ScreenState, GRACE_MS, HOLD, MIN_TURN_MS,
    QUIET, SCAN, STALL_MS,
};
use std::time::Instant;

fn state() -> ScreenState {
    ScreenState {
        screen: Screen::new(40, 5),
        pending: Vec::new(),
        held_since: Instant::now(),
        last_push: Instant::now(),
        last_scan: Instant::now() - SCAN,
        unscanned: false,
        printed: false,
        printed_work: false,
        echo: None,
    }
}

fn print(st: &mut ScreenState, bytes: &[u8], echo: bool) {
    st.screen.feed(bytes);
    st.unscanned = true;
    st.printed = true;
    st.printed_work |= !echo;
}

#[test]
fn scan_counts_new_text_once() {
    let mut st = state();
    print(&mut st, b"hello", false);
    assert!(st.scan(Front::Program).work);
    assert!(!st.unscanned);
    print(&mut st, b"", false);
    assert!(!st.scan(Front::Program).work);
}

#[test]
fn echo_updates_the_baseline_without_work() {
    let mut st = state();
    print(&mut st, b"typed", true);
    assert!(!st.scan(Front::Program).work);
    print(&mut st, b"", false);
    assert!(!st.scan(Front::Program).work);
}

#[test]
fn held_output_is_due_after_a_quiet_moment_or_the_hold() {
    let mut st = state();
    assert!(st.flush_due().is_none());
    let first = Instant::now();
    st.held_since = first;
    st.last_push = first;
    st.pending.extend_from_slice(b"a");
    assert_eq!(st.flush_due(), Some(first + QUIET));
    st.last_push = first + HOLD;
    assert_eq!(st.flush_due(), Some(first + HOLD));
    st.unscanned = true;
    assert_eq!(st.scan_due(), Some(st.last_scan + SCAN));
}

#[test]
fn deterministic_attention_boundaries_and_event_order() {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    use std::sync::atomic::Ordering::Relaxed;
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    let mut events = pool.events.subscribe();
    let report = |body: &str, now| pool.apply_status_at(&s, vec![StatusSignal::Report(body.as_bytes().to_vec())], now);
    s.last_input.store(1000, Relaxed);
    for (elapsed, expected) in [(super::ECHO_MS - 1, true), (super::ECHO_MS, false), (super::ECHO_MS + 1, false)] {
        assert_eq!(s.echoing_at(1000 + elapsed), expected);
    }
    report("state=working", 1000);
    s.last_output.store(1000, Relaxed);
    s.approval_since.store(1000, Relaxed);
    for (elapsed, expected) in [(GRACE_MS - 1, SessionState::Working), (GRACE_MS, SessionState::Waiting)] {
        pool.status_tick_at(&s, 1000 + elapsed);
        assert_eq!(s.info().state, expected);
    }
    assert_eq!(s.info().waiting_cause.as_deref(), Some("prompt"));
    report("state=blocked", 4000);
    assert_eq!(s.info().waiting_cause.as_deref(), Some("prompt"));
    while events.try_recv().is_ok() {}
    report("state=done", 5000);
    assert_eq!(s.info().waiting_cause, None);
    assert!(matches!(events.try_recv().unwrap(), Event::State { state: SessionState::Idle, .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::TurnEnded { .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    assert!(events.try_recv().is_err());
    report("state=working", 6000);
    s.last_output.store(6000, Relaxed);
    for (elapsed, expected) in [(STALL_MS - 1, false), (STALL_MS, true)] {
        pool.status_tick_at(&s, 6000 + elapsed);
        assert_eq!(s.info().program_status.unwrap().stalled, expected);
    }
    pool.kill(&info.id).unwrap();
}

/// An agy start, seen by the pool: reports at `t` ms after the session began.
fn agy_start(reports: &[(&str, u64)], tick_at: Option<u64>) -> (skiff_core::session::SessionInfo, Vec<skiff_core::session::SessionState>) {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::SessionSpec};
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    let t0 = info.started_at;
    let mut events = pool.events.subscribe();
    for (body, at) in reports {
        pool.apply_status_at(&s, vec![StatusSignal::Report(body.as_bytes().to_vec())], t0 + at);
    }
    if let Some(at) = tick_at {
        pool.status_tick_at(&s, t0 + at);
    }
    let mut states = vec![];
    while let Ok(e) = events.try_recv() {
        if let Event::State { state, .. } = e { states.push(state); }
    }
    let out = s.info();
    pool.kill(&info.id).unwrap();
    (out, states)
}

const AUTH: &str = "state=blocked:app=antigravity:kind=auth:msg=U2lnbiBpbiByZXF1aXJlZA==";

#[test]
fn a_short_auth_block_at_start_never_shows_waiting() {
    use skiff_core::session::SessionState::*;
    let (info, states) = agy_start(&[(AUTH, 100), ("state=idle:app=antigravity", 700)], None);
    assert_eq!(info.state, Idle);
    assert!(!states.contains(&Waiting), "{states:?}");
}

#[test]
fn a_permission_block_after_the_auth_block_waits_at_once() {
    use skiff_core::session::SessionState::*;
    let (info, states) = agy_start(&[(AUTH, 100), ("state=blocked:app=antigravity:kind=permission:msg=VHJ1c3Q=", 600)], None);
    assert_eq!(info.state, Waiting);
    assert_eq!(states.last(), Some(&Waiting));
}

#[test]
fn an_auth_block_that_lasts_waits_after_the_hold() {
    use skiff_core::session::SessionState::*;
    let (held, _) = agy_start(&[(AUTH, 100)], Some(super::AUTH_HOLD_MS - 1));
    assert_eq!(held.state, Working);
    let (info, states) = agy_start(&[(AUTH, 100)], Some(super::AUTH_HOLD_MS));
    assert_eq!(info.state, Waiting);
    assert_eq!(states.last(), Some(&Waiting));
}

#[test]
fn deterministic_scan_and_flush_deadlines() {
    let mut st = state();
    let t = Instant::now();
    st.last_scan = t;
    st.held_since = t;
    st.last_push = t;
    st.pending.push(b'x');
    st.unscanned = true;
    for (at, scan, flush) in [(t + QUIET - std::time::Duration::from_nanos(1), false, false), (t + QUIET, false, true), (t + SCAN, true, true)] {
        assert_eq!(st.scan_due().is_some_and(|due| due <= at), scan);
        assert_eq!(st.flush_due().is_some_and(|due| due <= at), flush);
    }
    st.last_push = t + HOLD;
    assert_eq!(st.flush_due(), Some(t + HOLD));
}

#[test]
fn waiting_causes_and_front_event_order() {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    // The test sets every input by hand. A new agent in front wakes the
    // reader to scan the screen, and that scan would set a cause of its own.
    s.pause_reader(Instant::now() + std::time::Duration::from_secs(5)).unwrap();
    pool.set_front(&s, Front::Shell);
    pool.bell(&s);
    assert_eq!(s.info().state, SessionState::Idle);
    pool.set_front(&s, Front::Command);
    pool.bell(&s);
    assert_eq!(s.info().waiting_cause.as_deref(), Some("bell"));
    pool.set_state(&s, SessionState::Working);
    pool.set_front(&s, Front::Agent("codex"));
    pool.apply_title(&s, Some("[ ! ] Action Required"));
    assert_eq!(s.info().waiting_cause.as_deref(), Some("title"));
    pool.set_state(&s, SessionState::Working);
    pool.apply_scan(&s, super::output::Scan { scanned: true, approval: true, ..Default::default() });
    assert_eq!(s.info().waiting_cause.as_deref(), Some("prompt"));
    pool.set_state(&s, SessionState::Working);
    pool.apply_status_at(&s, vec![StatusSignal::Report(b"state=blocked".to_vec())], 10000);
    assert_eq!(s.info().waiting_cause.as_deref(), Some("report"));
    let mut events = pool.events.subscribe();
    pool.set_front(&s, Front::Shell);
    assert_eq!(s.info().waiting_cause, None);
    assert!(matches!(events.try_recv().unwrap(), Event::State { state: SessionState::Idle, .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::TurnEnded { .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    assert!(events.try_recv().is_err());
    s.resume_reader();
    pool.kill(&info.id).unwrap();
}

#[test]
fn status_group_and_tick_event_order() {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    use std::sync::atomic::Ordering::Relaxed;
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    let report = |body: &str, now| pool.apply_status_at(&s, vec![StatusSignal::Report(body.as_bytes().to_vec())], now);
    report("state=working", 1000);
    s.approval_since.store(1000, Relaxed);
    s.last_output.store(1000, Relaxed);
    let mut events = pool.events.subscribe();
    pool.status_tick_at(&s, 1000 + STALL_MS);
    assert!(matches!(events.try_recv().unwrap(), Event::State { state: SessionState::Waiting, .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    assert!(events.try_recv().is_err());
    s.records.lock().unwrap().group = 7;
    pool.status_group(&s, 8);
    assert!(matches!(events.try_recv().unwrap(), Event::State { state: SessionState::Working, .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    assert!(events.try_recv().is_err());
    assert_eq!(s.info().waiting_cause, None);
    pool.kill(&info.id).unwrap();
}

#[test]
fn retained_result_work_transition_event_order() {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    pool.apply_status_at(&s, vec![StatusSignal::Report(b"state=done".to_vec())], 1000);
    s.records.lock().unwrap().group = 7;
    pool.status_group(&s, 8);
    assert!(s.info().program_status.unwrap().ended);
    let mut events = pool.events.subscribe();
    pool.set_state(&s, SessionState::Working);
    assert!(matches!(events.try_recv().unwrap(), Event::State { state: SessionState::Working, .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    assert!(events.try_recv().is_err());
    assert!(s.info().program_status.is_none());
    pool.kill(&info.id).unwrap();
}

#[tokio::test]
async fn exit_event_order_with_a_live_report() {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    use std::time::Duration;
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sh".into()), args: vec!["-c".into(), "read line".into()], cols: 80, rows: 24, peek: true, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    pool.apply_status_at(&s, vec![StatusSignal::Report(b"state=working".to_vec())], 1000);
    let mut events = pool.events.subscribe();
    pool.write(&info.id, b"go\n").unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(events.recv().await.unwrap(), Event::State { state: SessionState::Done, .. }) { break; }
        }
        assert!(matches!(events.recv().await.unwrap(), Event::Exit { .. }));
        assert!(matches!(events.recv().await.unwrap(), Event::SessionUpdated { .. }));
    }).await.unwrap();
}

/// A killed session must not look like a program that failed with code 1.
#[tokio::test]
async fn an_exit_says_when_a_signal_ended_the_process() {
    use skiff_core::{protocol::Event, session::SessionSpec};
    use std::time::Duration;
    let pool = super::SessionPool::new();
    let spec = |script: &str| SessionSpec { command: Some("/bin/sh".into()), args: vec!["-c".into(), script.into()], cols: 80, rows: 24, peek: true, ..Default::default() };
    let mut events = pool.events.subscribe();
    let failed = pool.create(spec("exit 1")).unwrap();
    let killed = pool.create(spec("sleep 30")).unwrap();
    pool.kill(&killed.id).unwrap();
    let (mut a, mut b) = (None, None);
    tokio::time::timeout(Duration::from_secs(5), async {
        while a.is_none() || b.is_none() {
            if let Event::Exit { session, code, signaled } = events.recv().await.unwrap() {
                if session == failed.id { a = Some((code, signaled)); }
                if session == killed.id { b = Some((code, signaled)); }
            }
        }
    }).await.unwrap();
    assert_eq!(a, Some((Some(1), false)));
    assert_eq!(b, Some((Some(1), true)));
}

#[test]
fn deterministic_status_throttle() {
    use crate::screen::StatusSignal;
    use skiff_core::{protocol::Event, session::SessionSpec};
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    let mut events = pool.events.subscribe();
    let report = |body: &str, now| pool.apply_status_at(&s, vec![StatusSignal::Report(body.as_bytes().to_vec())], now);
    report("state=working:msg=YQ==", 10000);
    while events.try_recv().is_ok() {}
    report("state=working:msg=Yg==", 10000 + super::STATUS_EVERY_MS - 1);
    assert!(events.try_recv().is_err());
    report("state=working:msg=Yw==", 10000 + super::STATUS_EVERY_MS);
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    report("state=working:msg=ZA==", 10000 + super::STATUS_EVERY_MS + 1);
    assert!(events.try_recv().is_err());
    pool.status_tick_at(&s, 11002);
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    pool.kill(&info.id).unwrap();
}

#[test]
fn deterministic_idle_boundary_and_waiting_cause() {
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    use std::sync::atomic::Ordering::Relaxed;
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    pool.set_front(&s, Front::Program);
    s.last_work.store(1000, Relaxed);
    s.fresh.store(false, Relaxed);
    let mut events = pool.events.subscribe();
    for (elapsed, expected) in [(super::IDLE_AFTER.as_millis() as u64, SessionState::Working), (super::IDLE_AFTER.as_millis() as u64 + 1, SessionState::Idle)] {
        pool.idle_if_quiet_at(&s, 1000 + elapsed);
        assert_eq!(s.info().state, expected);
    }
    assert!(matches!(events.try_recv().unwrap(), Event::State { state: SessionState::Idle, .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::TurnEnded { .. }));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    pool.bell(&s);
    assert_eq!(s.info().waiting_cause.as_deref(), Some("bell"));
    pool.set_state(&s, SessionState::Working);
    assert_eq!(s.info().waiting_cause, None);
    { let mut info = s.info.lock().unwrap(); info.state = SessionState::Waiting; info.waiting_cause = None; }
    while events.try_recv().is_ok() {}
    pool.wait(&s, super::attention::REPORT);
    assert_eq!(s.info().waiting_cause.as_deref(), Some("report"));
    assert!(matches!(events.try_recv().unwrap(), Event::SessionUpdated { .. }));
    assert!(events.try_recv().is_err());
    pool.kill(&info.id).unwrap();
}

#[test]
fn topic_drops_the_status_glyph() {
    assert_eq!(topic("◐ Fix the flusher"), "Fix the flusher");
    assert_eq!(topic("◑ Fix the flusher"), "Fix the flusher");
    assert_eq!(topic("⠋ Codex"), "Codex");
    assert_eq!(topic("[ ! ] Action Required"), "Action Required");
    assert_eq!(topic("plain"), "plain");
}

#[test]
fn only_text_counts_as_printing() {
    assert!(!prints(b"\x1b(B\x0f\x1b[?2004h\x1b[?1000h\x1b[?1006h"));
    assert!(!prints(b"\x1b]0;\xe2\x9c\xb3 Claude Code\x07\r\n"));
    assert!(!prints(b"\x1b[?2026h\x1b[?25l\x1b[43;16H\x1b[?25h\x1b[?2026l"));
    assert!(prints(b"\x1b[38;2;215;119;87m*\x1b[39m"));
    assert!(prints("\x1b[3G✻".as_bytes()));
}

/// While an agent reports with OSC 7501, its reports decide the state:
/// not its approval prompt, its title, its bell or the quiet time. With
/// no report left, those rules decide again.
#[test]
fn an_agents_reports_win_over_the_screen_and_the_timer() {
    use skiff_core::{
        protocol::Event,
        session::{SessionSpec, SessionState},
    };
    use std::time::Duration;
    let pool = super::SessionPool::new();
    let info = pool
        .create(SessionSpec {
            command: Some("/bin/sleep".into()),
            args: vec!["30".into()],
            cols: 80,
            rows: 24,
            ..Default::default()
        })
        .unwrap();
    let session = pool.get(&info.id).unwrap();
    pool.set_front(&session, Front::Agent("codex"));
    let mut events = pool.events.subscribe();
    // Each feed is scanned, as a read after a quiet moment is.
    let feed = |bytes: &[u8]| {
        std::thread::sleep(SCAN);
        pool.on_output(&session, &info.id, bytes);
        session.info()
    };
    let prompt = b"\r\nWould you like to run the following command?\r\nPress enter to confirm or esc to cancel\r\n";

    let now = feed(b"\x1b]7501;state=working:app=codex\x1b\\");
    assert_eq!(now.state, SessionState::Working);
    assert_eq!(now.program_status.unwrap().app.as_deref(), Some("codex"));
    assert_eq!(
        feed(prompt).state,
        SessionState::Working,
        "the screen rules overrode the report"
    );
    assert_eq!(
        feed(b"\x1b]0;[ ! ] Action Required\x07\x07").state,
        SessionState::Working,
        "the title or bell overrode it"
    );
    session
        .last_work
        .store(0, std::sync::atomic::Ordering::Relaxed);
    pool.idle_if_quiet(&session);
    assert_eq!(
        session.info().state,
        SessionState::Working,
        "the idle timer overrode it"
    );

    let asked =
        feed(b"\x1b]7501;state=blocked:app=codex:kind=question:msg=UGljayBhIGJyYW5jaA==\x1b\\");
    assert_eq!(asked.state, SessionState::Waiting);
    assert_eq!(asked.waiting_cause.as_deref(), Some("report"));
    let status = asked.program_status.unwrap();
    assert_eq!(
        (status.kind.as_deref(), status.msg.as_deref()),
        (Some("question"), Some("Pick a branch"))
    );
    pool.write(&info.id, b"x").unwrap();
    assert_eq!(
        session.info().state,
        SessionState::Waiting,
        "input answered it as if it were a bell"
    );

    let done = feed(b"\x1b]7501;state=done:app=codex\x1b\\");
    assert_eq!((done.state, done.unread), (SessionState::Idle, true));
    assert_eq!(done.waiting_cause, None, "the cause outlived the wait");
    assert_eq!(
        done.program_status.unwrap().state,
        "done",
        "done is told apart from idle"
    );
    feed(b"\x1b]7501;state=done:app=codex\x1b\\");
    let ended = std::iter::from_fn(|| events.try_recv().ok())
        .filter(|e| matches!(e, Event::TurnEnded { .. }))
        .count();
    assert_eq!(ended, 1, "a repeated done ended a second turn");

    // Cleared, the prompt still on screen counts again.
    let cleared = feed(b"\x1b]7501;state=clear\x1b\\");
    assert!(cleared.program_status.is_none());
    let asked = feed(prompt);
    assert_eq!(asked.state, SessionState::Waiting);
    assert_eq!(asked.waiting_cause.as_deref(), Some("prompt"));

    // A full reset drops the reports, and so does the shell's prompt.
    assert!(feed(b"\x1b]7501;state=working\x07")
        .program_status
        .is_some());
    assert!(feed(b"\x1bc").program_status.is_none());
    feed(b"\x1b]7501;state=error\x07");
    pool.set_front(&session, Front::Shell);
    // The failure stays to be read, and decides nothing more.
    let kept = session.info();
    let status = kept.program_status.unwrap();
    assert_eq!((status.state.as_str(), status.ended), ("error", true));
    assert!(kept.unread && !session.claimed());
    pool.seen(&info.id).unwrap();
    assert!(session.info().program_status.is_none());
    std::thread::sleep(Duration::from_millis(10));
    pool.kill(&info.id).unwrap();
}

/// A program that never says done (OpenCode) ends a turn the user
/// started with idle: the pane is unread. Its startup is no turn.
#[test]
fn idle_after_a_typed_turn_ends_it_for_a_program_that_never_says_done() {
    use skiff_core::{
        protocol::Event,
        session::{SessionSpec, SessionState},
    };
    let pool = super::SessionPool::new();
    let info = pool
        .create(SessionSpec {
            command: Some("/bin/sleep".into()),
            args: vec!["30".into()],
            cols: 80,
            rows: 24,
            ..Default::default()
        })
        .unwrap();
    let session = pool.get(&info.id).unwrap();
    pool.set_front(&session, Front::Agent("opencode"));
    let feed = |bytes: &[u8]| {
        std::thread::sleep(SCAN);
        pool.on_output(&session, &info.id, bytes);
        session.info()
    };
    let mut events = pool.events.subscribe();
    // Its startup: idle, then work nobody asked for.
    feed(b"\x1b]7501;state=idle:app=opencode\x07");
    feed(b"\x1b]7501;state=working:app=opencode\x07starting");
    std::thread::sleep(std::time::Duration::from_millis(MIN_TURN_MS));
    assert!(
        !feed(b"\x1b]7501;state=idle:app=opencode\x07").unread,
        "its startup was a turn"
    );
    // A turn the user typed.
    pool.write(&info.id, b"say hi\r").unwrap();
    feed(b"\x1b]7501;state=working:app=opencode\x07");
    std::thread::sleep(std::time::Duration::from_millis(MIN_TURN_MS));
    feed(b"Hi! How can I help?\r\n");
    let done = feed(b"\x1b]7501;state=idle:app=opencode\x07");
    assert_eq!((done.state, done.unread), (SessionState::Idle, true));
    let ended = std::iter::from_fn(|| events.try_recv().ok())
        .filter(|e| matches!(e, Event::TurnEnded { .. }))
        .count();
    assert_eq!(ended, 1);
    pool.kill(&info.id).unwrap();
}

/// The refinements over a report: another program in front ends it, the
/// agent's own prompt outlasts a report that it works, a long silence is
/// a hint, and a message-only change waits a moment.
#[test]
fn reports_bind_to_their_program_and_yield_to_its_prompt() {
    use skiff_core::{
        protocol::Event,
        session::{SessionSpec, SessionState},
    };
    use std::sync::atomic::Ordering::Relaxed;
    let pool = super::SessionPool::new();
    let info = pool
        .create(SessionSpec {
            command: Some("/bin/sleep".into()),
            args: vec!["30".into()],
            cols: 80,
            rows: 24,
            ..Default::default()
        })
        .unwrap();
    let session = pool.get(&info.id).unwrap();
    pool.set_front(&session, Front::Agent("codex"));
    let feed = |bytes: &[u8]| {
        std::thread::sleep(SCAN);
        pool.on_output(&session, &info.id, bytes);
        session.info()
    };
    let prompt = b"\r\nWould you like to run the following command?\r\nPress enter to confirm or esc to cancel\r\n";

    // Another group in front: the reports were the old program's.
    feed(b"\x1b]7501;state=done:app=codex\x07");
    let group = session.records.lock().unwrap().group;
    assert_ne!(group, 0, "the report took no group");
    pool.status_group(&session, group);
    assert!(
        session.info().program_status.is_some(),
        "the same group ended it"
    );
    pool.status_group(&session, group + 1);
    let left = session.info().program_status.unwrap();
    assert!(left.ended && !session.claimed(), "{left:?}");

    // The prompt under a report that it works, past the grace.
    assert_eq!(
        feed(b"\x1b]7501;state=working\x07").state,
        SessionState::Working
    );
    assert!(!session.info().program_status.unwrap().ended);
    assert_eq!(feed(prompt).state, SessionState::Working);
    pool.status_tick(&session);
    assert_eq!(session.info().state, SessionState::Working, "no grace");
    session.approval_since.store(now_ms() - GRACE_MS, Relaxed);
    pool.status_tick(&session);
    assert_eq!(session.info().state, SessionState::Waiting);
    assert_eq!(session.info().waiting_cause.as_deref(), Some("prompt"));
    assert_eq!(
        feed(b"\x1b[2J\x1b[Hdone deciding\r\n").state,
        SessionState::Working
    );

    // Silence while it works.
    session.last_output.store(now_ms() - STALL_MS, Relaxed);
    pool.status_tick(&session);
    assert!(session.info().program_status.unwrap().stalled);
    assert!(!feed(b"x").program_status.unwrap().stalled);

    // A message-only change waits for the tick; a state change does not.
    let mut events = pool.events.subscribe();
    let updates = |events: &mut tokio::sync::broadcast::Receiver<Event>| {
        std::iter::from_fn(|| events.try_recv().ok())
            .filter(|e| matches!(e, Event::SessionUpdated { .. }))
            .count()
    };
    feed(b"\x1b]7501;state=working:msg=VGhpbmtpbmc=\x07");
    feed(b"\x1b]7501;state=working:msg=UmVzcG9uZGluZw==\x07");
    assert_eq!(
        updates(&mut events),
        0,
        "a message-only change went out at once"
    );
    assert_eq!(
        session.info().program_status.unwrap().msg.as_deref(),
        Some("Responding")
    );
    pool.status_tick(&session);
    assert_eq!(updates(&mut events), 1, "the tick did not send it");
    feed(b"\x1b]7501;state=blocked:kind=permission\x07");
    assert_eq!(updates(&mut events), 1);
    pool.kill(&info.id).unwrap();
}

/// An update sent from the info under its lock (here the shell back in
/// front) carries the time of the last output, not the session's start.
#[test]
fn an_update_carries_the_last_output_time() {
    use skiff_core::{protocol::Event, session::SessionSpec};
    let pool = super::SessionPool::new();
    let info = pool
        .create(SessionSpec {
            command: Some("/bin/sleep".into()),
            args: vec!["30".into()],
            ..Default::default()
        })
        .unwrap();
    let session = pool.get(&info.id).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    pool.on_output(&session, &info.id, b"x");
    let mut events = pool.events.subscribe();
    pool.set_front(&session, Front::Shell);
    let sent = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|e| match e {
            Event::SessionUpdated { session } => Some(session),
            _ => None,
        })
        .expect("no update");
    assert!(
        sent.last_output_at > info.started_at,
        "{} <= {}",
        sent.last_output_at,
        info.started_at
    );
    assert_eq!(sent.last_output_at, session.info().last_output_at);
    pool.kill(&info.id).unwrap();
}

/// The pty's size, read back from the kernel.
fn pty_size(session: &super::Session) -> (u16, u16) {
    let mut size = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: TIOCGWINSZ writes only `size`.
    assert_eq!(unsafe { libc::ioctl(session.pty.raw_fd(), libc::TIOCGWINSZ as _, &mut size as *mut _) }, 0);
    (size.ws_col, size.ws_row)
}

/// Two clients resize at once. The first has sized the pty and not yet
/// recorded it (the info lock is held here to keep it there). The second
/// must wait, or it sizes the pty and the first then announces its older
/// size last: the inventory and every client would disagree with the pty.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_resizes_announce_the_size_the_pty_has() {
    use skiff_core::{protocol::Event, session::SessionSpec};
    use std::time::Duration;
    let pool = super::SessionPool::new();
    let info = pool
        .create(SessionSpec { command: Some("/bin/cat".into()), cols: 80, rows: 24, ..Default::default() })
        .unwrap();
    let session = pool.get(&info.id).unwrap();
    let mut events = pool.events.subscribe();
    let resize = |cols, rows| {
        let (pool, id) = (pool.clone(), info.id.clone());
        tokio::spawn(async move { pool.resize(&id, cols, rows).await })
    };
    let held = session.info.lock().unwrap();
    let first = resize(100, 30);
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while pty_size(&session) != (100, 30) {
        assert!(std::time::Instant::now() < end, "the first resize never reached the pty");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second = resize(120, 40);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(pty_size(&session), (100, 30), "the second resize ran ahead of the first's announcement");
    drop(held);
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    let mut last = None;
    while let Ok(e) = events.try_recv() {
        if let Event::SessionUpdated { session: s } = e {
            if s.id == info.id {
                last = Some((s.cols, s.rows));
            }
        }
    }
    let now = pty_size(&session);
    let inventory = pool.get(&info.id).unwrap().info();
    assert_eq!(last, Some(now), "the last event");
    assert_eq!((inventory.cols, inventory.rows), now, "the inventory");
    assert_eq!(now, (120, 40));
    pool.kill(&info.id).unwrap();
}

/// With the rule behind it, each agent's prompt makes the wait a bare
/// `approval` made before: the same state and the same cause.
#[test]
fn each_agents_prompt_waits_as_it_did_before_the_rule() {
    use skiff_core::session::{SessionSpec, SessionState};
    let hint = "| x\r\n\u{2502} \u{2191}/\u{2193} to navigate \u{b7} enter to select \u{b7} esc to cancel";
    let copilot_tool = format!("No, and tell Copilot what to do differently {hint}");
    let copilot_trust = format!("Do you trust the files in this folder? {hint}");
    let prompts: [(&'static str, &str, &str); 10] = [
        ("claude", " Esc to cancel \u{b7} Tab to amend", "claude-esc-to-cancel"),
        ("codex", "Press enter to confirm or esc to cancel", "codex-confirm"),
        ("codex", "enter continue \u{b7} esc skip", "codex-update"),
        ("opencode", "Allow once   Allow always   Reject", "opencode-allow"),
        ("gemini", "\u{2191}/\u{2193} Navigate \u{b7} tab Amend", "gemini-navigate"),
        ("gemini", "Allow once\r\nNo, suggest changes (esc)", "gemini-allow"),
        ("copilot", &copilot_tool, "copilot-tool"),
        ("copilot", &copilot_trust, "copilot-trust"),
        ("grok", "1/4:select  \u{2502}  Tab:next option", "grok-select"),
        ("grok", "Allow once\r\nReject", "grok-allow"),
    ];
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    // The test sets every scan by hand. A new agent in front wakes the
    // reader to scan the blank screen, and that scan would end the wait.
    s.pause_reader(Instant::now() + std::time::Duration::from_secs(5)).unwrap();
    for (agent, screen, rule) in prompts {
        pool.set_front(&s, Front::Agent(agent));
        pool.set_state(&s, SessionState::Working);
        let mut st = state();
        st.screen = Screen::new(80, 16);
        print(&mut st, screen.as_bytes(), false);
        let scan = st.scan(Front::Agent(agent));
        assert_eq!(scan.prompt.as_ref().map(|p| p.rule), Some(rule));
        assert!(scan.approval, "{rule}");
        pool.apply_scan(&s, scan);
        let with_rule = s.info();
        assert_eq!((with_rule.state, with_rule.waiting_cause.as_deref()), (SessionState::Waiting, Some("prompt")), "{rule}");
        // The prompt goes, and a bare approval comes as before the rule.
        pool.apply_scan(&s, super::output::Scan { scanned: true, ..Default::default() });
        assert_eq!(s.info().state, SessionState::Working, "{rule}");
        pool.apply_scan(&s, super::output::Scan { scanned: true, approval: true, ..Default::default() });
        let bare = s.info();
        assert_eq!((bare.state, bare.waiting_cause), (with_rule.state, with_rule.waiting_cause), "{rule}");
    }
    s.resume_reader();
    pool.kill(&info.id).unwrap();
}

/// OSC 133;A, a new shell prompt, drops the `working` and `blocked` records
/// and keeps `done`, as with ssh in front after the agent behind it left.
/// It ends no turn.
#[test]
fn a_new_prompt_drops_blocked_and_keeps_done() {
    use skiff_core::{protocol::Event, session::{SessionSpec, SessionState}};
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    pool.set_front(&s, Front::Command);
    pool.on_output(&s, &info.id, b"\x1b]7501;state=done:id=build\x1b\\\x1b]7501;state=blocked:kind=question\x1b\\");
    assert_eq!(s.info().state, SessionState::Waiting);
    let mut events = pool.events.subscribe();
    pool.on_output(&s, &info.id, b"\x1b]133;A\x07$ ");
    let records = s.records.lock().unwrap().clone();
    assert_eq!(records.root(), None);
    assert_eq!(records.get("build").map(|r| r.state), Some(crate::program_status::State::Done));
    assert_ne!(s.info().state, SessionState::Waiting);
    while let Ok(e) = events.try_recv() {
        assert!(!matches!(e, Event::TurnEnded { .. }), "a new prompt ended a turn");
    }
    pool.kill(&info.id).unwrap();
}

/// A shell command an agent runs (Claude Code's `!`) must not end the
/// agent's work. Were its OSC 133;A to reach the pane, the agent in front
/// still owns it.
#[test]
fn a_new_prompt_under_an_agent_keeps_its_work() {
    use skiff_core::session::{SessionSpec, SessionState};
    let pool = super::SessionPool::new();
    let info = pool.create(SessionSpec { command: Some("/bin/sleep".into()), args: vec!["30".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    pool.set_front(&s, Front::Agent("claude"));
    pool.on_output(&s, &info.id, b"\x1b]7501;state=working:app=claude-code\x07");
    pool.on_output(&s, &info.id, b"\x1b]133;A;cl=m;aid=7\x1b\\% \x1b]133;B\x1b\\");
    let root = s.records.lock().unwrap().root().map(|r| r.state);
    assert_eq!(root, Some(crate::program_status::State::Working));
    assert_eq!(s.info().state, SessionState::Working);
    pool.kill(&info.id).unwrap();
}

/// A program's first report can come before the foreground watcher has
/// seen it start, while the shell is still taken to be in front. Another
/// group than the shell's is in front, so the report counts.
#[test]
fn a_first_report_before_the_watcher_sees_its_program_counts() {
    use crate::screen::StatusSignal;
    use skiff_core::session::{SessionSpec, SessionState};
    use std::time::{Duration, Instant};
    let pool = super::SessionPool::new();
    // `set -m`: sleep gets a group of its own and the terminal, as a program
    // an interactive shell starts does.
    let info = pool.create(SessionSpec { command: Some("/bin/sh".into()), args: vec!["-c".into(), "set -m; sleep 30; :".into()], cols: 80, rows: 24, ..Default::default() }).unwrap();
    let s = pool.get(&info.id).unwrap();
    pool.set_front(&s, Front::Shell);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.foreground_group().is_none_or(|g| Some(g) == info.pid) {
        assert!(Instant::now() < deadline, "no program came in front");
        std::thread::sleep(Duration::from_millis(20));
    }
    pool.apply_status_at(&s, vec![StatusSignal::Report(b"state=working:app=claude-code".to_vec())], now_ms());
    let root = s.records.lock().unwrap().root().map(|r| r.state);
    assert_eq!(root, Some(crate::program_status::State::Working));
    assert_eq!(s.info().state, SessionState::Working);
    pool.kill(&info.id).unwrap();
}

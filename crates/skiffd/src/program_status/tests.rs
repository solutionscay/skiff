use super::*;

fn set(body: &str) -> (Option<String>, Record) {
    match parse(body.as_bytes()) {
        Some(Report::Set { id, record }) => (id, record),
        other => panic!("{body}: {other:?}"),
    }
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

#[test]
fn idle_turn_duration_boundary() {
    use crate::session::MIN_TURN_MS;

    for (elapsed, printed, input, expected) in [
        (MIN_TURN_MS - 1, true, true, false),
        (MIN_TURN_MS, true, true, true),
        (MIN_TURN_MS + 1, true, true, true),
        (MIN_TURN_MS, false, true, false),
        (MIN_TURN_MS, true, false, false),
    ] {
        let mut turns = Turns::default();
        assert!(!turns.step(None, Some(State::Idle), 100, 0, 0));
        assert!(!turns.step(Some(State::Idle), Some(State::Working), 200, input as u64, 0));
        assert_eq!(
            turns.step(Some(State::Working), Some(State::Idle), 200 + elapsed, input as u64, printed as u64),
            expected,
            "elapsed={elapsed}, printed={printed}, input={input}",
        );
    }
}

#[test]
fn reads_the_agents_reports() {
    let (id, r) = set("state=working:app=codex");
    assert_eq!(
        (id, r.state, r.app.as_deref()),
        (None, State::Working, Some("codex"))
    );
    let msg = "YXBwcm92ZSBCYXNoOiBta2RpciAvdG1wL29zYzc1MDEtZGVueS10ZXN0";
    let (_, r) = set(&format!(
        "state=blocked:app=claude-code:kind=permission:msg={msg}"
    ));
    assert_eq!(r.state, State::Blocked);
    assert_eq!(r.kind, Some(Kind::Permission));
    assert_eq!(
        r.msg.as_deref(),
        Some("approve Bash: mkdir /tmp/osc7501-deny-test")
    );
    assert_eq!(parse(b"state=clear"), Some(Report::Clear { id: None }));
    assert_eq!(parse(b"?"), Some(Report::Probe));
}

#[test]
fn ignores_what_the_spec_ignores() {
    // No state, an unknown state, a bad id, a report over its size.
    assert_eq!(parse(b"app=codex"), None);
    assert_eq!(parse(b"state=sleeping"), None);
    assert_eq!(parse(b"state=working:id=a//b"), None);
    assert_eq!(parse(b"state=working:id=a b"), None);
    assert_eq!(
        parse(&[b"state=working:x=".as_slice(), &[b'a'; MAX_BODY]].concat()),
        None
    );
    assert_eq!(parse(b""), None);
    assert_eq!(parse(b"\xff\xfe"), None);
    // A bad pair is skipped and the rest still counts; the last of a repeated key wins.
    let (_, r) = set("bogus:=x:state=idle:Kind=x:app=a;b:state=working:future=1");
    assert_eq!((r.state, r.app), (State::Working, None));
    // kind only with blocked, progress only with working or blocked, both in range.
    let (_, r) = set("state=done:kind=permission:progress=50");
    assert_eq!((r.kind, r.progress), (None, None));
    assert_eq!(set("state=working:progress=101").1.progress, None);
    assert_eq!(set("state=working:progress=+5").1.progress, None);
    assert_eq!(set("state=working:progress=42").1.progress, Some(42));
    assert_eq!(set("state=blocked:kind=telepathy").1.kind, None);
    assert_eq!(
        set(&format!("state=idle:app={}", "a".repeat(33))).1.app,
        None
    );
}

#[test]
fn cleans_and_caps_the_message() {
    let (_, r) = set(&format!(
        "state=blocked:msg={}",
        b64("rm -rf\n\x1b[31mbuild\u{202e}txt.exe\x07")
    ));
    assert_eq!(r.msg.as_deref(), Some("rm -rf [31mbuildtxt.exe"));
    // Not base64: the field goes, the report stays.
    let (_, r) = set("state=blocked:kind=question:msg=@@@");
    assert_eq!(
        (r.state, r.kind, r.msg),
        (State::Blocked, Some(Kind::Question), None)
    );
    // Unpadded base64 reads too.
    assert_eq!(set("state=idle:title=aGk").1.title.as_deref(), Some("hi"));
    let long = "é".repeat(1500);
    let msg = set(&format!("state=blocked:msg={}", b64(&long)))
        .1
        .msg
        .unwrap();
    assert!(
        msg.len() <= MAX_MSG && msg.chars().all(|c| c == 'é'),
        "{}",
        msg.len()
    );
    assert_eq!(set(&format!("state=idle:msg={}", b64(" \n "))).1.msg, None);
}

#[test]
fn records_by_id_and_clear() {
    let mut r = Records::default();
    for body in [
        "state=working",
        "state=working:id=build",
        "state=done:id=build/test",
        "state=error:id=lint",
        "state=idle:id=buildx",
    ] {
        r.apply(parse(body.as_bytes()).unwrap());
    }
    assert_eq!(r.len(), 5);
    // A report replaces its record whole.
    r.apply(parse(b"state=blocked:id=lint").unwrap());
    assert_eq!(r.get("lint").unwrap().state, State::Blocked);
    // Clear takes the record and those under it, not a sibling that shares a prefix.
    r.apply(parse(b"state=clear:id=build").unwrap());
    assert!(r.get("build").is_none() && r.get("build/test").is_none());
    assert!(r.get("buildx").is_some() && r.root().is_some());
    r.apply(parse(b"state=clear:id=").unwrap_or(Report::Probe));
    assert_eq!(r.len(), 3, "an empty id is a bad id: the report is ignored");
    r.apply(parse(b"state=clear").unwrap());
    assert!(r.is_empty());
}

#[test]
fn a_live_report_decides_the_state() {
    use SessionState::*;
    for now in [Working, Waiting, Idle] {
        assert_eq!(session_state(Some(State::Working), now), Working);
        assert_eq!(session_state(Some(State::Blocked), now), Waiting);
        for s in [State::Idle, State::Done, State::Error] {
            assert_eq!(session_state(Some(s), now), Idle);
        }
    }
    // With no root record, the other rules decide from where it stands.
    assert_eq!(session_state(None, Waiting), Working);
    assert_eq!(session_state(None, Idle), Idle);
    assert_eq!(session_state(None, Working), Working);
    // done and error end a turn once; idle ends none.
    assert!(ends_turn(Some(State::Working), Some(State::Done)));
    assert!(ends_turn(None, Some(State::Error)));
    assert!(ends_turn(Some(State::Done), Some(State::Error)));
    assert!(!ends_turn(Some(State::Done), Some(State::Done)));
    assert!(!ends_turn(Some(State::Blocked), Some(State::Idle)));
    assert!(!ends_turn(Some(State::Done), None));
}

#[test]
fn drops_the_least_recently_updated_record() {
    let mut r = Records::default();
    r.apply(parse(b"state=working").unwrap());
    for i in 0..MAX_RECORDS {
        r.apply(parse(format!("state=working:id=t{i}").as_bytes()).unwrap());
    }
    assert_eq!(r.len(), MAX_RECORDS);
    assert!(r.root().is_none() && r.get("t0").is_some());
}

#[test]
fn the_view_lists_children_and_only_a_result_outlives_its_program() {
    let mut r = Records::default();
    for body in [
        "state=working:app=grok",
        "state=blocked:id=s2:kind=question",
        "state=working:id=s1",
    ] {
        r.apply(parse(body.as_bytes()).unwrap());
    }
    let view = r.view().unwrap();
    let ids: Vec<_> = view
        .children
        .iter()
        .map(|c| (c.id.as_str(), c.state.as_str()))
        .collect();
    assert_eq!(ids, [("s1", "working"), ("s2", "blocked")]);
    assert_eq!(view.children[1].kind.as_deref(), Some("question"));
    assert_eq!(
        r.clone().end(),
        None,
        "a working program left nothing to read"
    );
    r.apply(parse(b"state=error:msg=Ym9vbQ==").unwrap());
    let kept = r.end().unwrap();
    assert_eq!(
        (kept.state.as_str(), kept.ended, kept.msg.as_deref()),
        ("error", true, Some("boom"))
    );
    assert!(kept.children.is_empty() && r.is_empty());
    let mut a = view.clone();
    a.msg = Some("Thinking".into());
    assert!(Records::minor_change(&view, &a));
    a.state = "idle".into();
    assert!(!Records::minor_change(&view, &a));
}

/// A program that never says done ends a turn with idle, when the user
/// started it and it worked long enough to print. Not its startup, not
/// a prompt turned down, and never once it has said done.
#[test]
fn idle_ends_a_turn_only_for_a_program_that_never_says_done() {
    use State::*;
    let (i, w, b, d) = (Some(Idle), Some(Working), Some(Blocked), Some(Done));
    // (old, new, ms, input, output) in order; the turn ends it tells.
    type Step = (Option<State>, Option<State>, u64, u64, u64);
    let run = |steps: &[Step]| {
        let mut t = Turns::default();
        steps
            .iter()
            .map(|&(o, n, at, inp, out)| t.step(o, n, at, inp, out))
            .collect::<Vec<_>>()
    };
    // Started by the user, 2 s of output: a turn.
    assert_eq!(
        run(&[(None, i, 0, 5, 0), (i, w, 100, 6, 10), (w, i, 2100, 6, 900)]),
        [false, false, true]
    );
    // Its own startup work, with no input since it first reported.
    assert_eq!(
        run(&[(None, i, 0, 5, 0), (i, w, 100, 5, 10), (w, i, 2600, 5, 900)]),
        [false, false, false]
    );
    // Too short, or nothing printed.
    assert!(!run(&[(None, i, 0, 5, 0), (i, w, 100, 6, 10), (w, i, 900, 6, 900)])[2]);
    assert!(!run(&[(None, i, 0, 5, 0), (i, w, 100, 6, 10), (w, i, 2100, 6, 10)])[2]);
    // A first turn denied: straight to idle, or through a short working.
    assert!(
        !run(&[
            (None, i, 0, 5, 0),
            (i, w, 100, 6, 10),
            (w, b, 2100, 6, 500),
            (b, i, 4000, 7, 600)
        ])[3]
    );
    assert!(
        !run(&[
            (None, i, 0, 5, 0),
            (i, w, 100, 6, 10),
            (w, b, 2100, 6, 500),
            (b, w, 4000, 7, 600),
            (w, i, 4010, 7, 620)
        ])[4]
    );
    // Once it said done, idle ends no turn again.
    assert_eq!(
        run(&[
            (None, i, 0, 5, 0),
            (i, w, 100, 6, 10),
            (w, d, 2100, 6, 900),
            (d, w, 3000, 7, 900),
            (w, i, 6000, 7, 2000)
        ]),
        [false, false, true, false, false]
    );
}

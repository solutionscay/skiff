//! The ledger's tests, on private ledgers in a scratch folder.

use super::*;

#[test]
fn add_defaults_a_task_to_pending_and_a_plan_to_sent() {
    assert_eq!(add_state(None, false, false).unwrap(), "pending");
    assert_eq!(add_state(None, true, false).unwrap(), "sent");
    assert_eq!(add_state(Some("done"), false, false).unwrap(), "done");
    assert_eq!(add_state(None, false, true).unwrap(), "pending");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("skiff-ledger-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn task(t: &str) -> New {
    New {
        plan: false,
        parent: None,
        pane_id: Some("abc123456789".into()),
        pane_gen: Some(1000),
        project: "app".into(),
        task: t.into(),
        title: None,
        state: "sent".into(),
        result: None,
        reference: None,
        task_type: None,
        socket: Some("/tmp/here.sock".into()),
        owner: GRAND.into(),
        handoff: false,
        expects: None,
    }
}

#[test]
fn supported_ledger_schemas_keep_their_columns_and_version() {
    let columns = [
        "id", "uid", "kind", "parent", "pane_id", "pane_gen", "project", "task", "sent_at", "state", "result_path", "ref", "updated_at", "type", "socket", "reported_at", "title", "closed_at", "gone_at", "owner", "reopened_at", "work_dir", "flushed_at", "outcome", "expects", "commit_ref",
    ];
    let v11 = TASKS.replace(",\n  outcome     TEXT,\n  expects     TEXT,\n  commit_ref  TEXT", "");
    let fixtures = [
        (3, v11.replace(",\n  socket      TEXT,\n  reported_at TEXT,\n  title       TEXT,\n  closed_at   TEXT,\n  gone_at     TEXT,\n  owner       TEXT NOT NULL DEFAULT 'grand',\n  reopened_at TEXT,\n  work_dir    TEXT,\n  flushed_at  TEXT", "")),
        (4, v11.replace(",\n  reported_at TEXT,\n  title       TEXT,\n  closed_at   TEXT,\n  gone_at     TEXT,\n  owner       TEXT NOT NULL DEFAULT 'grand',\n  reopened_at TEXT,\n  work_dir    TEXT,\n  flushed_at  TEXT", "")),
        (6, v11.replace(",\n  closed_at   TEXT,\n  gone_at     TEXT,\n  owner       TEXT NOT NULL DEFAULT 'grand',\n  reopened_at TEXT,\n  work_dir    TEXT,\n  flushed_at  TEXT", "")),
        (8, v11.replace("('task', 'plan', 'handoff')", "('task', 'plan')").replace(",\n  owner       TEXT NOT NULL DEFAULT 'grand',\n  reopened_at TEXT,\n  work_dir    TEXT,\n  flushed_at  TEXT", "")),
        (9, v11.replace(",\n  reopened_at TEXT,\n  work_dir    TEXT,\n  flushed_at  TEXT", "")),
        (10, v11.replace(",\n  work_dir    TEXT,\n  flushed_at  TEXT", "")),
        (11, v11.clone()),
        (12, TASKS.to_string()),
    ];
    for (version, schema) in fixtures {
        let dir = scratch(&format!("schema-v{version}"));
        let file = dir.join("ledger.db");
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&format!("{schema}; PRAGMA user_version = {version};")).unwrap();
        drop(db);
        let db = open_at(&file).unwrap();
        let got: Vec<String> = db
            .prepare("SELECT name FROM pragma_table_info('tasks') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(got, columns);
        let deliveries: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'deliveries'", [], |r| r.get(0)).unwrap();
        assert_eq!(deliveries, 1);
        assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), VERSION);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn reads_only_open_rows_and_stores_the_type() {
    let dir = scratch("open");
    let file = dir.join("ledger.db");
    assert!(open_tasks_at(&file).unwrap().is_empty() && !file.exists());
    let mut db = open_at(&file).unwrap();
    insert(&mut db, &file, task("fix bug")).unwrap();
    insert(&mut db, &file, New { state: "done".into(), ..task("seo") }).unwrap();
    let (id, _, _, _) = insert(&mut db, &file, New { state: "pending".into(), task_type: Some("review".into()), ..task("docs") }).unwrap();
    let t = open_tasks_at(&file).unwrap();
    assert_eq!(t.iter().map(|r| (r.task.as_str(), r.state.as_str())).collect::<Vec<_>>(), [("fix bug", "sent"), ("docs", "pending")]);
    let r = one(&db, &id.to_string()).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap()["type"], "review");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_3_ledger_takes_sockets_in_place_and_keeps_its_rows() {
    let dir = scratch("v3");
    let file = dir.join("ledger.db");
    {
        let v3 = TASKS.replace("type        TEXT,\n  socket      TEXT,\n  reported_at TEXT,\n  title       TEXT,\n  closed_at   TEXT,\n  gone_at     TEXT", "type        TEXT");
        assert!(!v3.contains("socket"));
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&v3).unwrap();
        db.execute_batch(UIDS).unwrap();
        db.execute_batch(
            "INSERT INTO tasks (uid, kind, pane_id, task, sent_at, state, updated_at) VALUES ('a1b2c3d4', 'task', 'abc', 'live work', 'x', 'sent', 'x');
             INSERT INTO tasks (uid, kind, task, sent_at, state, updated_at) VALUES ('e5f6a7b8', 'plan', 'a plan', 'x', 'sent', 'x');
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }
    let db = open_at(&file).unwrap();
    let list = rows(&db, "", None).unwrap();
    assert_eq!(list.iter().map(|r| r.uid.as_str()).collect::<Vec<_>>(), ["a1b2c3d4", "e5f6a7b8"]);
    // The task went out on the live daemon; a plan has no pane.
    assert_eq!(list[0].socket.as_deref(), Some(state::live_socket().to_str().unwrap()));
    assert_eq!(list[1].socket, None);
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), VERSION);
    assert_eq!(owner(&db).unwrap(), None);
    assert!(!dir.join("ledger.db.old").exists());
    // A newer skiff's stamp stays.
    db.pragma_update(None, "user_version", VERSION + 1).unwrap();
    drop(db);
    let db = open_at(&file).unwrap();
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), VERSION + 1);
    // A second open changes nothing.
    drop(db);
    assert_eq!(rows(&open_at(&file).unwrap(), "", None).unwrap().len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_4_ledger_takes_reported_at_and_title_in_place_and_keeps_its_rows() {
    let dir = scratch("v4");
    let file = dir.join("ledger.db");
    {
        let v4 = TASKS.replace(",\n  reported_at TEXT,\n  title       TEXT,\n  closed_at   TEXT,\n  gone_at     TEXT", "");
        assert!(!v4.contains("reported_at") && !v4.contains("title"));
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&v4).unwrap();
        db.execute_batch(UIDS).unwrap();
        db.execute_batch(OWNER).unwrap();
        db.execute_batch(
            "INSERT INTO tasks (uid, kind, pane_id, task, sent_at, state, updated_at, socket) VALUES ('a1b2c3d4', 'task', 'abc', 'old work', 'x', 'done', 'x', '/tmp/s.sock');
             INSERT INTO tasks (uid, kind, pane_id, task, sent_at, state, updated_at, socket) VALUES ('e5f6a7b8', 'task', 'abc', 'live work', 'x', 'sent', 'x', '/tmp/s.sock');
             PRAGMA user_version = 4;",
        )
        .unwrap();
    }
    let db = open_at(&file).unwrap();
    let list = rows(&db, "", None).unwrap();
    assert_eq!(list.iter().map(|r| (r.uid.as_str(), r.reported_at.as_deref())).collect::<Vec<_>>(), [("a1b2c3d4", None), ("e5f6a7b8", None)]);
    // The v4 socket stays as it was.
    assert_eq!(list[0].socket.as_deref(), Some("/tmp/s.sock"));
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), VERSION);
    assert_eq!(unreported_at(&file, GRAND).unwrap(), 1);
    assert!(list.iter().all(|r| r.title.is_none()));
    db.execute("UPDATE tasks SET title = 'Old work' WHERE uid = 'a1b2c3d4'", []).unwrap();
    assert_eq!(one(&db, "a1b2c3d4").unwrap().title.as_deref(), Some("Old work"));
    // A second open adds nothing.
    drop(db);
    let db = open_at(&file).unwrap();
    let n: i64 = db.query_row("SELECT count(*) FROM pragma_table_info('tasks') WHERE name = 'reported_at'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    assert_eq!(rows(&db, "", None).unwrap().len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The uids `ls` lists in `view`, with `state` if given, as `run` does it.
fn listed(db: &Connection, view: View, state: Option<&str>) -> Vec<String> {
    let mut list = rows(db, "", None).unwrap();
    roll_up(&mut list);
    keep(&mut list, view);
    if let Some(s) = state {
        list.retain(|r| r.shown_state() == s);
    }
    tree(list).into_iter().map(|(r, _)| r.uid).collect()
}

#[test]
fn ls_lists_open_work_closed_work_and_untold_tasks() {
    let dir = scratch("view");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let done = |t: &str| New { state: "done".into(), ..task(t) };
    let (_, open, _, _) = insert(&mut db, &file, task("open")).unwrap();
    let (_, told, _, _) = insert(&mut db, &file, done("told")).unwrap();
    let (_, untold, _, _) = insert(&mut db, &file, done("untold")).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("half") }).unwrap();
    let (_, p_open, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..task("a") }).unwrap();
    let (_, p_told, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..done("b") }).unwrap();
    let (_, q, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("over") }).unwrap();
    let (_, q_done, _, _) = insert(&mut db, &file, New { parent: Some(q.clone()), ..done("c") }).unwrap();
    assert_eq!(report(&mut db, &Caller::Person, &[told.clone(), p_told.clone()], false).unwrap(), 2);
    assert_eq!(close(&mut db, &Caller::Person, &[told.clone(), q.clone()]).unwrap(), 2);
    let s = |v: &[&String]| v.iter().map(|u| u.to_string()).collect::<Vec<_>>();

    // Done work stays listed until it is closed, told or not.
    assert_eq!(listed(&db, View::Open, None), s(&[&open, &untold, &p, &p_open, &p_told]));
    assert_eq!(listed(&db, View::Closed, None), s(&[&told, &q, &q_done]));
    assert_eq!(listed(&db, View::All, None), s(&[&open, &told, &untold, &p, &p_open, &p_told, &q, &q_done]));
    // Close told the person of c.
    assert_eq!(listed(&db, View::Unreported, None), s(&[&untold]));
    assert_eq!(listed(&db, View::Open, Some("done")), s(&[&untold, &p_told]));

    // Interrupted and gone work is open.
    let mut list = rows(&db, "", None).unwrap();
    list.iter_mut().find(|r| r.uid == open).unwrap().live = Some(false);
    keep(&mut list, View::Open);
    assert!(list.iter().any(|r| r.uid == open && r.shown_state() == GONE));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn done_and_reported_work_closes_by_itself_until_reopened() {
    let dir = scratch("autoclose");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let closed = |db: &Connection, uid: &str| one(db, uid).unwrap().closed_at.is_some();
    let done = |t: &str| New { state: "done".into(), ..task(t) };
    let (_, lone, _, _) = insert(&mut db, &file, done("lone")).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, a, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..done("a") }).unwrap();
    let (_, h, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), handoff: true, ..done("h") }).unwrap();
    let (_, k, _, _) = insert(&mut db, &file, New { parent: Some(h.clone()), owner: "web".into(), ..task("k") }).unwrap();
    let (_, empty, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("empty") }).unwrap();
    // Done is not enough: the person was not told yet.
    assert!(auto_close(&mut db).unwrap().is_empty());
    report(&mut db, &Caller::Person, &[], true).unwrap();
    // The hand-off's task is still sent: the plan stays open. A plan with no task stays open.
    assert_eq!(auto_close(&mut db).unwrap().iter().map(|r| r.uid.clone()).collect::<Vec<_>>(), [lone.clone()]);
    assert!(!closed(&db, &p) && !closed(&db, &empty));
    db.execute("UPDATE tasks SET state = 'done' WHERE uid = ?1", [&k]).unwrap();
    report(&mut db, &Caller::Person, &[], true).unwrap();
    auto_close(&mut db).unwrap();
    assert!(closed(&db, &p));
    // Reopened work stays open, until a task in it changes state.
    reopen(&mut db, &Caller::Person, &[p.clone(), lone.clone()]).unwrap();
    assert!(auto_close(&mut db).unwrap().is_empty());
    assert!(!closed(&db, &p) && !closed(&db, &lone));
    db.execute("UPDATE tasks SET state = 'sent' WHERE uid = ?1", [&a]).unwrap();
    db.execute("UPDATE tasks SET state = 'done' WHERE uid = ?1", [&a]).unwrap();
    auto_close(&mut db).unwrap();
    assert!(closed(&db, &p) && !closed(&db, &lone));
    // A hand-off's task clears the mark two steps up.
    reopen(&mut db, &Caller::Person, std::slice::from_ref(&p)).unwrap();
    db.execute("UPDATE tasks SET state = 'pending' WHERE uid = ?1", [&k]).unwrap();
    assert!(one(&db, &p).unwrap().closed_at.is_none());
    let held: Option<String> = db.query_row("SELECT reopened_at FROM tasks WHERE uid = ?1", [&p], |r| r.get(0)).unwrap();
    assert!(held.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_version_9_ledger_takes_reopened_at_and_keeps_its_rows() {
    let dir = scratch("v9");
    let file = dir.join("ledger.db");
    let db = Connection::open(&file).unwrap();
    db.execute_batch(&TASKS.replace(",\n  reopened_at TEXT", "")).unwrap();
    db.execute_batch(&format!("INSERT INTO tasks (uid, task, sent_at, state, updated_at) VALUES ('a1b2c3d4', 't', 'x', 'done', 'x'); PRAGMA user_version = 9;")).unwrap();
    drop(db);
    let db = open_at(&file).unwrap();
    assert_eq!(rows(&db, "", None).unwrap().len(), 1);
    assert!(skiff_client::ledger::open_read(&file).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_is_working_while_an_agent_is_on_a_task_else_open_until_closed() {
    let dir = scratch("buckets");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, lone, _, _) = insert(&mut db, &file, task("lone")).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, p_on, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..task("a") }).unwrap();
    insert(&mut db, &file, New { parent: Some(p.clone()), state: "pending".into(), ..task("b") }).unwrap();
    let (_, q, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("q") }).unwrap();
    close(&mut db, &Caller::Person, std::slice::from_ref(&q)).unwrap();
    let mut list = rows(&db, "", None).unwrap();
    list.iter_mut().find(|r| r.uid == p_on).unwrap().agent_on = true;
    skiff_client::ledger::buckets(&mut list);
    let b = |uid: &str| list.iter().find(|r| r.uid == uid).unwrap().bucket.unwrap();
    // A plan and every task in it share one bucket.
    assert!(list.iter().filter(|r| r.work() == p).all(|r| r.bucket == Some(WORKING)));
    assert_eq!((b(&lone), b(&q)), (OPEN, skiff_client::ledger::FINISHED));
    assert!(rank(Some(WORKING)) < rank(Some(OPEN)));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn close_takes_done_work_on_its_head_and_reopen_clears_it() {
    let dir = scratch("close");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let done = |t: &str| New { state: "done".into(), ..task(t) };
    let (_, a, _, _) = insert(&mut db, &file, done("a")).unwrap();
    let (_, open, _, _) = insert(&mut db, &file, New { state: "pending".into(), ..task("open") }).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, p_done, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..done("c") }).unwrap();
    let closed = |db: &Connection, uid: &str| one(db, uid).unwrap().closed_at;

    // Work with a task that is not done refuses the lot.
    let err = close(&mut db, &Caller::Person, &[a.clone(), open.clone()]).unwrap_err().to_string();
    assert_eq!(err, format!("task {open} is pending, not done. Set it done or remove it first. Nothing changed"));
    assert!(closed(&db, &a).is_none());
    // A task in a plan is not a work head.
    let err = close(&mut db, &Caller::Person, std::slice::from_ref(&p_done)).unwrap_err().to_string();
    assert_eq!(err, format!("task {p_done} is in plan {p}. Name the plan. Nothing changed"));
    // The stamp goes on the head, and the person was told of its tasks.
    assert_eq!(close(&mut db, &Caller::Person, &[a.clone(), p.clone()]).unwrap(), 2);
    assert!(closed(&db, &p).is_some() && closed(&db, &p_done).is_none());
    assert!(one(&db, &p_done).unwrap().reported_at.is_some() && one(&db, &a).unwrap().reported_at.is_some());
    assert!(one(&db, &p).unwrap().reported_at.is_none());
    // A first stamp stays.
    db.execute("UPDATE tasks SET closed_at = '2020-01-01T00:00:00Z' WHERE uid = ?1", [&a]).unwrap();
    assert_eq!(close(&mut db, &Caller::Person, std::slice::from_ref(&a)).unwrap(), 0);
    assert_eq!(closed(&db, &a).as_deref(), Some("2020-01-01T00:00:00Z"));

    // Open work takes the stamp too, so it does not close by itself.
    assert_eq!(reopen(&mut db, &Caller::Person, &[a.clone(), open.clone()]).unwrap(), 2);
    assert!(closed(&db, &a).is_none());
    // A task set back from done opens its work again.
    close(&mut db, &Caller::Person, std::slice::from_ref(&a)).unwrap();
    restate(&db, &one(&db, &p_done).unwrap(), Some("sent")).unwrap();
    assert!(closed(&db, &p).is_none() && one(&db, &p_done).unwrap().reported_at.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_follow_up_joins_its_work_and_a_lone_task_gets_a_plan() {
    let dir = scratch("join");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let first = New { state: "done".into(), title: Some("Build 177".into()), reference: Some("#177".into()), task_type: Some("build".into()), ..task("Build issue 177") };
    let (_, lone, lone_path, none) = insert(&mut db, &file, first).unwrap();
    assert_eq!(none, None);
    close(&mut db, &Caller::Person, std::slice::from_ref(&lone)).unwrap();

    // The follow-up wraps the lone task in a plan, and the closed work opens.
    let (_, check, check_path, plan) = insert(&mut db, &file, New { parent: Some(lone.clone()), ..task("Check the app") }).unwrap();
    let p = one(&db, plan.as_deref().unwrap()).unwrap();
    let t = one(&db, &lone).unwrap();
    assert!(p.is_plan() && p.closed_at.is_none() && t.closed_at.is_none());
    assert_eq!((p.task.as_str(), p.title.as_deref(), p.reference.as_deref(), p.task_type.as_deref()), ("Build issue 177", Some("Build 177"), Some("#177"), Some("build")));
    assert_eq!((t.parent.as_deref(), t.result_path.as_deref()), (Some(p.uid.as_str()), Some(lone_path.as_str())));
    // The new plan has its own folder, named from the title. The lone task's folder stays.
    let folder = dir.join("results").join(format!("{}-build-177", p.uid));
    assert_eq!((p.work_dir.as_deref(), p.result_path.as_deref()), (folder.to_str(), Some(folder.join("plan.md").to_str().unwrap())));
    assert_eq!(check_path, folder.join(format!("{check}-check-the-app")).join("result.md").display().to_string());
    assert_eq!(t.work_dir, Some(dir.join("results").join(format!("{lone}-build-177")).display().to_string()));
    assert!(folder.is_dir() && dir.join("results").join(format!("{lone}-build-177")).is_dir());

    // A second follow-up names the plan, and a closed plan opens.
    db.execute("UPDATE tasks SET state = 'done' WHERE uid = ?1", [&check]).unwrap();
    close(&mut db, &Caller::Person, std::slice::from_ref(&p.uid)).unwrap();
    let (_, _, _, again) = insert(&mut db, &file, New { parent: Some(p.uid.clone()), ..task("Commit it") }).unwrap();
    assert_eq!(again.as_deref(), Some(p.uid.as_str()));
    assert!(one(&db, &p.uid).unwrap().closed_at.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_6_ledger_takes_closed_at_in_place_and_all_work_stays_open() {
    let dir = scratch("v6");
    let file = dir.join("ledger.db");
    {
        let v6 = TASKS.replace(",\n  closed_at   TEXT,\n  gone_at     TEXT", "");
        assert!(!v6.contains("closed_at"));
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&v6).unwrap();
        db.execute_batch(UIDS).unwrap();
        db.execute_batch(OWNER).unwrap();
        db.execute_batch(
            "INSERT INTO tasks (uid, kind, pane_id, task, sent_at, state, updated_at, socket, reported_at, title) VALUES ('a1b2c3d4', 'task', 'abc', 'told', 'x', 'done', 'x', '/tmp/s.sock', 'y', 'Told');
             INSERT INTO tasks (uid, kind, pane_id, task, sent_at, state, updated_at, socket) VALUES ('e5f6a7b8', 'task', 'abc', 'live', 'x', 'sent', 'x', '/tmp/s.sock');
             PRAGMA user_version = 6;",
        )
        .unwrap();
    }
    let db = open_at(&file).unwrap();
    let list = rows(&db, "", None).unwrap();
    assert_eq!(list.iter().map(|r| (r.uid.as_str(), r.closed_at.as_deref())).collect::<Vec<_>>(), [("a1b2c3d4", None), ("e5f6a7b8", None)]);
    assert_eq!((list[0].reported_at.as_deref(), list[0].title.as_deref()), (Some("y"), Some("Told")));
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), VERSION);
    assert_eq!(listed(&db, View::Open, None).len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn report_stamps_done_rows_once_and_refuses_open_ones() {
    let dir = scratch("report");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let done = |t: &str| New { state: "done".into(), ..task(t) };
    let (_, a, _, _) = insert(&mut db, &file, done("a")).unwrap();
    let (_, b, _, _) = insert(&mut db, &file, done("b")).unwrap();
    let (_, open, _, _) = insert(&mut db, &file, task("open")).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, p_done, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..done("c") }).unwrap();
    let (_, p_open, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..task("d") }).unwrap();
    let stamp = |db: &Connection, uid: &str| one(db, uid).unwrap().reported_at;
    assert_eq!(unreported_at(&file, GRAND).unwrap(), 3);

    assert_eq!(report(&mut db, &Caller::Person, std::slice::from_ref(&a), false).unwrap(), 1);
    assert!(stamp(&db, &a).is_some());
    // A second run keeps the first stamp.
    db.execute("UPDATE tasks SET reported_at = '2020-01-01T00:00:00Z' WHERE uid = ?1", [&a]).unwrap();
    assert_eq!(report(&mut db, &Caller::Person, std::slice::from_ref(&a), false).unwrap(), 0);
    assert_eq!(stamp(&db, &a).as_deref(), Some("2020-01-01T00:00:00Z"));
    // An open task refuses the lot.
    let err = report(&mut db, &Caller::Person, &[b.clone(), open.clone()], false).unwrap_err().to_string();
    assert_eq!(err, format!("task {open} is sent, not done. Nothing changed"));
    assert!(stamp(&db, &b).is_none());
    // A plan stamps its done tasks only.
    assert_eq!(report(&mut db, &Caller::Person, std::slice::from_ref(&p), false).unwrap(), 1);
    assert!(stamp(&db, &p_done).is_some() && stamp(&db, &p_open).is_none() && stamp(&db, &p).is_none());
    // --all stamps every done task not reported, and only those.
    assert_eq!(report(&mut db, &Caller::Person, &[], true).unwrap(), 1);
    assert!(stamp(&db, &b).is_some() && stamp(&db, &open).is_none());
    assert_eq!(stamp(&db, &a).as_deref(), Some("2020-01-01T00:00:00Z"));
    assert_eq!(unreported_at(&file, GRAND).unwrap(), 0);

    // A task that opens again loses its stamp.
    restate(&db, &one(&db, &b).unwrap(), Some("done")).unwrap();
    assert!(stamp(&db, &b).is_some());
    restate(&db, &one(&db, &b).unwrap(), None).unwrap();
    assert!(stamp(&db, &b).is_some());
    restate(&db, &one(&db, &b).unwrap(), Some("pending")).unwrap();
    assert!(stamp(&db, &b).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn prune_takes_only_work_closed_long_ago_with_its_tasks() {
    let dir = scratch("prune");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let done = |t: &str| New { state: "done".into(), ..task(t) };
    let (_, old, _, _) = insert(&mut db, &file, done("old")).unwrap();
    let (_, new, _, _) = insert(&mut db, &file, done("new")).unwrap();
    let (_, told, _, _) = insert(&mut db, &file, done("told, still open")).unwrap();
    let (_, open, _, _) = insert(&mut db, &file, task("open")).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, p_a, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..done("a") }).unwrap();
    let (_, p_b, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..done("b") }).unwrap();
    close(&mut db, &Caller::Person, &[old.clone(), new.clone(), p.clone()]).unwrap();
    report(&mut db, &Caller::Person, std::slice::from_ref(&told), false).unwrap();
    for uid in [&old, &p] {
        db.execute("UPDATE tasks SET closed_at = '2020-01-01T00:00:00Z' WHERE uid = ?1", [uid]).unwrap();
    }
    db.execute("UPDATE tasks SET reported_at = '2020-01-01T00:00:00Z' WHERE uid = ?1", [&told]).unwrap();
    let uids = |rows: Vec<Row>| rows.into_iter().map(|r| r.uid).collect::<Vec<_>>();
    let all = || uids(rows(&open_at(&file).unwrap(), "", None).unwrap());
    let before = all();

    // A dry run lists and changes nothing.
    let want = vec![old.clone(), p.clone(), p_a.clone(), p_b.clone()];
    assert_eq!(uids(prune(&mut db, &Caller::Person, 7 * 86_400, false).unwrap()), want);
    assert_eq!(all(), before);
    assert_eq!(uids(prune(&mut db, &Caller::Person, 7 * 86_400, true).unwrap()), want);
    // Work told long ago but not closed stays.
    assert_eq!(all(), [new.clone(), told.clone(), open.clone()]);
    // A short window takes the new one too.
    db.execute("UPDATE tasks SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 hours') WHERE uid = ?1", [&new]).unwrap();
    assert!(prune(&mut db, &Caller::Person, age("7d").unwrap(), true).unwrap().is_empty());
    assert_eq!(uids(prune(&mut db, &Caller::Person, age("1h").unwrap(), true).unwrap()), [new]);
    assert_eq!(age("30m").unwrap(), 1800);
    assert!(age("7").is_err() && age("xd").is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_task_on_another_socket_is_never_gone_and_is_not_sent_from_here() {
    let dir = scratch("elsewhere");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, mine, _, _) = insert(&mut db, &file, task("mine")).unwrap();
    let (_, theirs, _, _) = insert(&mut db, &file, New { socket: Some("/tmp/other.sock".into()), state: "pending".into(), ..task("theirs") }).unwrap();
    let here = Path::new("/tmp/here.sock");
    let mut list = rows(&db, "", None).unwrap();
    // No daemon answers here, as for a pane on another socket.
    mark_live(here, None, &mut list);
    let r = |uid: &str| list.iter().find(|r| r.uid == uid).unwrap().clone();
    assert!(!r(&mine).elsewhere);
    assert!(r(&theirs).elsewhere);
    assert_eq!(r(&theirs).socket_other_than(here), Some("/tmp/other.sock"));
    // Even a daemon that does not know the pane does not make it gone.
    let mut t = r(&theirs);
    t.live = None;
    assert_eq!(t.shown_state(), "pending");
    let err = send_row(here, &db, &r(&theirs), SendOpts { text: vec![], enter: true, shell: false }).await.unwrap_err().to_string();
    assert!(err.contains("/tmp/other.sock") && !err.contains("gone"), "{err}");
    assert_eq!(one(&db, &theirs).unwrap().state, "pending");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_task_whose_pane_is_gone_closes_done_and_stays_unreported() {
    let dir = scratch("sweep");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let here = Path::new("/tmp/here.sock");
    let (_, gone, result, _) = insert(&mut db, &file, task("lost")).unwrap();
    let (_, waits, _, _) = insert(&mut db, &file, New { state: "pending".into(), pane_id: Some("dead00000000".into()), ..task("queued") }).unwrap();
    let (_, kept, _, _) = insert(&mut db, &file, New { pane_id: Some("feed00000000".into()), ..task("live pane") }).unwrap();
    let (_, prefix, _, _) = insert(&mut db, &file, New { pane_id: Some("feed".into()), ..task("imported prefix") }).unwrap();
    let (_, theirs, _, _) = insert(&mut db, &file, New { socket: Some("/tmp/other.sock".into()), ..task("elsewhere") }).unwrap();
    let (_, done, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("finished") }).unwrap();
    let (_, plan, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, child, _, _) = insert(&mut db, &file, New { parent: Some(plan.clone()), ..task("in plan") }).unwrap();
    // A restarted pane still exists: its task is interrupted, not gone.
    let all = [pane("feed00000000", "agent", false)];
    let closed = close_gone_in(&mut db, here, &all).unwrap();
    let mut uids: Vec<&str> = closed.iter().map(|r| r.uid.as_str()).collect();
    uids.sort_unstable();
    let mut want = [gone.as_str(), waits.as_str(), child.as_str()];
    want.sort_unstable();
    assert_eq!(uids, want);
    let r = one(&db, &gone).unwrap();
    assert_eq!((r.state.as_str(), r.shown_state(), r.reported_at.as_deref()), ("done", "gone", None));
    assert!(r.gone_at.is_some());
    // Nobody saw it end: unobserved, never succeeded.
    assert_eq!(r.outcome.as_deref(), Some("unobserved"));
    assert_eq!(r.result_path.as_deref(), Some(result.as_str()));
    for uid in [&kept, &prefix, &theirs] {
        assert_ne!(one(&db, uid).unwrap().state, "done", "{uid}");
    }
    // A done task whose session closed needs no report, and is not "gone".
    let r = one(&db, &done).unwrap();
    assert!(r.gone_at.is_none() && r.reported_at.is_some());
    // The plan rolls up to done. The next auto close closes it.
    let mut list = rows(&db, "", None).unwrap();
    roll_up(&mut list);
    let p = list.iter().find(|r| r.uid == plan).unwrap();
    assert_eq!((p.state.as_str(), p.closed_at.as_deref()), ("done", None));
    assert_eq!(unreported_at(&file, GRAND).unwrap(), 3);
    // A second sweep finds nothing.
    assert!(close_gone_in(&mut db, here, &all).unwrap().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

fn pane(id: &str, role: &str, coordinator: bool) -> SessionInfo {
    let mut s: SessionInfo = serde_json::from_value(serde_json::json!({
        "id": id, "label": "x", "role": role, "cwd": "/", "command": "claude",
        "args": [], "state": "idle", "cols": 80, "rows": 24, "pid": null, "exit_code": null,
    }))
    .unwrap();
    s.coordinator = coordinator;
    s
}

#[test]
fn only_the_owner_a_person_or_the_live_agent_changes_the_ledger() {
    let live = state::live_socket();
    let test = Path::new("/tmp/test.sock");
    let on = |session: &str, socket: &Path| Owner { session: session.into(), socket: socket.display().to_string() };
    let owner = on("96cb7de8aaaa", &live);
    let skipper = pane("96cb7de8aaaa", "agent", true);
    let worker = pane("02900b71aaaa", "agent", false);
    let shell = pane("70808970aaaa", "shell", false);
    let w = |me: &SessionInfo, cur: Option<&Owner>, socket: &Path| writer(Some(&me.id), Some(me), cur, socket, false);
    // The owner, a person outside Skiff and a shell pane may.
    assert_eq!(w(&skipper, Some(&owner), &live), Writer::Yes(Caller::Grand));
    assert_eq!(writer(None, None, Some(&owner), &live, true), Writer::Yes(Caller::Person));
    assert_eq!(w(&shell, Some(&owner), &live), Writer::Yes(Caller::Person));
    // Another agent pane may not, on any socket.
    assert_eq!(w(&worker, Some(&owner), &live), Writer::No);
    assert_eq!(w(&worker, None, &live), Writer::No);
    // A second Skiff agent on a test daemon may not take the live ledger.
    let dev = pane("4d43fa26aaaa", "agent", true);
    assert_eq!(w(&dev, Some(&owner), test), Writer::No);
    // A session the daemon does not list is not a person.
    assert_eq!(writer(Some("deadbeef"), None, Some(&owner), &live, false), Writer::No);
    // A shell outside Skiff without --person may be an agent: refused.
    assert_eq!(writer(None, None, Some(&owner), &live, false), Writer::No);
    assert_eq!(writer(None, None, None, &live, false), Writer::No);
    // --person does not let an agent pane through.
    assert_eq!(writer(Some(&worker.id), Some(&worker), Some(&owner), &live, true), Writer::No);
    // Ownership moves: a new Skiff agent on the owner's daemon takes it,
    // the live agent takes it from any socket, and a first one takes it.
    let next = pane("f0afaa59aaaa", "agent", true);
    assert_eq!(w(&next, Some(&owner), &live), Writer::Claim);
    assert_eq!(w(&skipper, Some(&on("4d43fa26aaaa", test)), &live), Writer::Claim);
    assert_eq!(w(&dev, None, test), Writer::Claim);
    assert_eq!(refusal(Some(&owner), true), format!("this ledger is Admiral's on {}. Ask Admiral there to make this change", live.display()));
    assert_eq!(refusal(Some(&owner), false), "only Admiral and the project and fleet Skippers change the ledger. Ask Admiral to make this change");
    assert!(refusal(None, false).starts_with("this ledger has no Admiral yet."));
}

#[test]
fn a_project_skipper_changes_only_its_rows_and_reports_its_hand_off() {
    let live = state::live_socket();
    let owner = Owner { session: "96cb7de8aaaa".into(), socket: live.display().to_string() };
    let mut gesher = pane("5e5e5e5eaaaa", "agent", false);
    gesher.skipper = Some("gesher".into());
    // The CLI knows the caller from its session.
    let me = writer(Some(&gesher.id), Some(&gesher), Some(&owner), &live, false);
    assert_eq!(me, Writer::Yes(Caller::Project("gesher".into())));
    let Writer::Yes(me) = me else { unreachable!() };

    let dir = scratch("owners");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    // The grand Skipper's plan, its own task, and a hand-off to gesher.
    let (_, plan, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("Ship it") }).unwrap();
    let (_, mine_g, _, _) = insert(&mut db, &file, New { parent: Some(plan.clone()), ..task("grand work") }).unwrap();
    let ho = New { parent: Some(plan.clone()), project: "gesher".into(), handoff: true, ..task("Chapter polish") };
    let (_, hand, hand_path, _) = insert(&mut db, &file, ho).unwrap();
    // The hand-off's result is in the project Skipper's folder.
    assert!(hand_path.starts_with(dir.join("projects/gesher/results").to_str().unwrap()), "{hand_path}");
    // gesher adds its task under the hand-off: the row is gesher's.
    let parent = container(&db, &hand).unwrap();
    let sub_owner = me.check_parent(&parent).unwrap();
    assert_eq!(sub_owner, "gesher");
    let (_, sub, sub_path, _) = insert(&mut db, &file, New { parent: Some(hand.clone()), owner: sub_owner, ..task("chapter nav") }).unwrap();
    assert!(sub_path.contains(&format!("projects/gesher/results/")) && sub_path.contains(&hand), "{sub_path}");
    let sub = one(&db, &sub).unwrap();
    assert_eq!(sub.parent.as_deref(), Some(hand.as_str()));

    // gesher changes its row, reports the hand-off, and nothing of the grand Skipper's.
    assert!(me.check(&sub).is_ok());
    let hand_row = one(&db, &hand).unwrap();
    assert!(me.check_report(&hand_row).is_ok());
    assert!(me.check(&hand_row).is_err());
    let err = me.check(&one(&db, &mine_g).unwrap()).unwrap_err().to_string();
    assert_eq!(err, format!("{mine_g} is Admiral's task. Ask Admiral to change it"));
    assert!(me.check_parent(&one(&db, &plan).unwrap()).is_err());
    // Another project Skipper reports no hand-off of gesher's.
    let other = Caller::Project("skiff".into());
    assert!(other.check_report(&hand_row).is_err() && other.check(&sub).is_err());
    assert_eq!(other.check(&sub).unwrap_err().to_string(), format!("{} is the gesher project Skipper's task. Ask that Skipper to change it", sub.uid));
    let err = Caller::Grand.check_parent(&hand_row).unwrap_err().to_string();
    assert_eq!(err, format!("{hand} is a hand-off to the gesher project Skipper. Ask that Skipper to add tasks under it"));
    // The grand Skipper cannot change gesher's task or add under the hand-off; the person can.
    assert!(Caller::Grand.check(&sub).is_err());
    assert!(Caller::Grand.check_parent(&hand_row).is_err());
    assert!(Caller::Grand.check(&hand_row).is_ok());
    assert!(Caller::Person.check(&sub).is_ok());
    assert_eq!(Caller::Person.check_parent(&hand_row).unwrap(), "gesher");
    // report --all from gesher stamps only its rows.
    db.execute("UPDATE tasks SET state = 'done'", []).unwrap();
    assert_eq!(report(&mut db, &me, &[], true).unwrap(), 1);
    assert!(one(&db, &mine_g).unwrap().reported_at.is_none());
    assert!(report(&mut db, &me, std::slice::from_ref(&mine_g), false).is_err());
    // The tree nests three deep, and the whole work is the plan's.
    let list = rows(&db, "", None).unwrap();
    let depth: Vec<(String, usize)> = tree(list.clone()).into_iter().map(|(r, d)| (r.uid, d)).collect();
    assert_eq!(depth, [(plan.clone(), 0), (mine_g.clone(), 1), (hand.clone(), 1), (sub.uid.clone(), 2)]);
    assert!(heads(&list).values().all(|h| *h == plan));
    // Closing the plan takes the hand-off's task too; gesher cannot close it.
    assert!(close(&mut db, &me, std::slice::from_ref(&plan)).is_err());
    assert_eq!(close(&mut db, &Caller::Grand, std::slice::from_ref(&plan)).unwrap(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_8_ledger_takes_owners_in_one_step_and_keeps_its_rows() {
    let dir = scratch("v8");
    let file = dir.join("ledger.db");
    {
        let v8 = TASKS.replace(",\n  owner       TEXT NOT NULL DEFAULT 'grand'", "").replace(", 'handoff'", "");
        assert!(!v8.contains("owner") && !v8.contains("handoff"));
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&v8).unwrap();
        db.execute_batch(UIDS).unwrap();
        db.execute_batch(OWNER).unwrap();
        db.execute_batch(
            "INSERT INTO tasks (id, uid, kind, task, sent_at, state, updated_at) VALUES (7, 'e5f6a7b8', 'plan', 'a plan', 'x', 'sent', 'x');
             INSERT INTO tasks (id, uid, kind, parent, pane_id, task, sent_at, state, updated_at) VALUES (9, 'a1b2c3d4', 'task', 'e5f6a7b8', 'abc', 'work', 'x', 'sent', 'x');
             PRAGMA user_version = 8;",
        )
        .unwrap();
    }
    let mut db = open_at(&file).unwrap();
    let list = rows(&db, "", None).unwrap();
    assert_eq!(list.iter().map(|r| (r.id, r.uid.as_str(), r.owner.as_str())).collect::<Vec<_>>(), [(7, "e5f6a7b8", GRAND), (9, "a1b2c3d4", GRAND)]);
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), VERSION);
    // The new shape takes a hand-off, and ids go on from the old ones.
    let (id, _, _, _) = insert(&mut db, &file, New { handoff: true, project: "gesher".into(), ..task("polish") }).unwrap();
    assert_eq!(id, 10);
    assert!(skiff_client::ledger::open_read(&file).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn claim_records_the_owner_and_keeps_the_live_one() {
    let dir = scratch("claim");
    let file = dir.join("ledger.db");
    let live = state::live_socket();
    let test = Path::new("/tmp/test.sock");
    claim_at(&file, &live, "96cb7de8aaaa").unwrap();
    let get = || owner(&open_at(&file).unwrap()).unwrap().unwrap();
    assert_eq!(get(), Owner { session: "96cb7de8aaaa".into(), socket: live.display().to_string() });
    // A Skiff agent on another socket does not take it.
    let err = claim_at(&file, test, "4d43fa26aaaa").unwrap_err().to_string();
    assert_eq!(err, format!("this ledger is Admiral's on {}. Do ledger work from Admiral there", live.display()));
    assert_eq!(get().session, "96cb7de8aaaa");
    // A restart on the live daemon moves it.
    claim_at(&file, &live, "f0afaa59aaaa").unwrap();
    assert_eq!(get().session, "f0afaa59aaaa");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_old_ledger_moves_aside_and_a_new_one_starts_empty() {
    let dir = scratch("old");
    let file = dir.join("ledger.db");
    Connection::open(&file).unwrap().execute_batch("CREATE TABLE tasks (id INTEGER PRIMARY KEY, pane_id TEXT, task TEXT);").unwrap();
    let db = open_at(&file).unwrap();
    assert!(rows(&db, "", None).unwrap().is_empty());
    assert!(dir.join("ledger.db.old").is_file());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn delete_and_add_reuse_no_id_and_no_uid() {
    let dir = scratch("reuse");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let mut seen = std::collections::HashSet::new();
    let mut last = 0;
    for i in 0..20 {
        let (id, uid, _, _) = insert(&mut db, &file, task(&format!("t{i}"))).unwrap();
        assert!(id > last, "id {id} after {last}");
        assert!(seen.insert(uid.clone()), "uid {uid} again");
        assert!(uid.len() == 8 && uid.bytes().all(|b| b.is_ascii_hexdigit()) && !uid.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(one(&db, &uid).unwrap().id, id);
        assert_eq!(one(&db, &id.to_string()).unwrap().uid, uid);
        last = id;
        // The newest row goes, as `skiff ledger rm` does it.
        db.execute("DELETE FROM tasks WHERE id = ?1", [id]).unwrap();
    }
    assert!(one(&db, &last.to_string()).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_plan_lists_its_tasks_under_it_with_paths_in_its_folder() {
    let dir = scratch("plan");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, lone, lone_path, _) = insert(&mut db, &file, task("Fix the login bug!")).unwrap();
    let plan_row = New { plan: true, pane_id: None, pane_gen: None, reference: Some("JIRA-12".into()), ..task("Release 0.7: notes & builds") };
    let (pid, puid, plan_path, _) = insert(&mut db, &file, plan_row).unwrap();
    let (_, a, a_path, _) = insert(&mut db, &file, New { parent: Some(puid.clone()), ..task("Write the release notes") }).unwrap();
    let (_, b, b_path, _) = insert(&mut db, &file, New { parent: Some(pid.to_string()), ..task("Build the Linux packages") }).unwrap();
    let results = dir.join("results");
    assert_eq!(lone_path, results.join(format!("{lone}-fix-the-login-bug")).join("result.md").display().to_string());
    let folder = results.join(format!("{puid}-release-0-7-notes-builds"));
    assert_eq!(plan_path, folder.join("plan.md").display().to_string());
    assert_eq!(a_path, folder.join(format!("{a}-write-the-release-notes")).join("result.md").display().to_string());
    assert_eq!(b_path, folder.join(format!("{b}-build-the-linux-packages")).join("result.md").display().to_string());
    // `add` makes each work folder, and no file.
    for d in [results.join(format!("{lone}-fix-the-login-bug")), folder.join(format!("{a}-write-the-release-notes"))] {
        assert!(d.is_dir() && std::fs::read_dir(&d).unwrap().next().is_none(), "{}", d.display());
    }
    assert_eq!(one(&db, &a).unwrap().work_dir, Some(folder.join(format!("{a}-write-the-release-notes")).display().to_string()));
    // A task in a plan names its plan.
    let (_, x, _, plan) = insert(&mut db, &file, New { parent: Some(a.clone()), ..task("x") }).unwrap();
    assert_eq!(plan.as_deref(), Some(puid.as_str()));
    db.execute("DELETE FROM tasks WHERE uid = ?1", [&x]).unwrap();

    let shown: Vec<_> = tree(rows(&db, "", None).unwrap()).into_iter().map(|(r, child)| (r.uid, child)).collect();
    assert_eq!(shown, [(lone.clone(), 0), (puid.clone(), 0), (a, 1), (b, 1)]);
    let p = one(&db, &puid).unwrap();
    assert!(p.is_plan() && p.pane_id.is_none() && p.reference.as_deref() == Some("JIRA-12"));
    // The watcher gets the open tasks, not the plan.
    assert_eq!(open_tasks_at(&file).unwrap().len(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_hand_off_gets_a_folder_with_its_project_and_an_old_plan_keeps_its_folder() {
    let dir = scratch("handoff-dir");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, title: Some("Ship it".into()), ..task("p") }).unwrap();
    // A hand-off under Admiral's plan goes with the project Skipper's results, not in the plan's folder.
    let (_, h, h_path, _) = insert(&mut db, &file, New { parent: Some(p.clone()), handoff: true, project: "web".into(), title: Some("Fix web".into()), ..task("h") }).unwrap();
    let h_dir = dir.join("projects/web/results").join(format!("{h}-fix-web"));
    assert_eq!(h_path, h_dir.join("result.md").display().to_string());
    // The project Skipper's task under it nests in it.
    let (_, k, k_path, _) = insert(&mut db, &file, New { parent: Some(h.clone()), owner: "web".into(), ..task("Do the fix") }).unwrap();
    assert_eq!(k_path, h_dir.join(format!("{k}-do-the-fix")).join("result.md").display().to_string());
    assert!(h_dir.join(format!("{k}-do-the-fix")).is_dir());
    // A named result path stays, and the folder is still made.
    let (_, n, n_path, _) = insert(&mut db, &file, New { result: Some("/elsewhere/r.md".into()), ..task("named") }).unwrap();
    assert_eq!(n_path, "/elsewhere/r.md");
    assert!(dir.join("results").join(format!("{n}-named")).is_dir());
    // A plan from before work folders: a new task goes in the folder it had.
    db.execute("UPDATE tasks SET work_dir = NULL, result_path = ?2 WHERE uid = ?1", params![p, dir.join("results/2026-10-01-old-plan/plan.md").display().to_string()]).unwrap();
    let (_, t, t_path, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..task("late") }).unwrap();
    assert_eq!(t_path, dir.join("results/2026-10-01-old-plan").join(format!("{t}-late")).join("result.md").display().to_string());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_10_ledger_takes_work_dir_and_flushed_at_in_place_and_keeps_its_rows() {
    let dir = scratch("v10");
    let file = dir.join("ledger.db");
    {
        let v10 = TASKS.replace(",\n  work_dir    TEXT,\n  flushed_at  TEXT", "");
        assert_ne!(v10, TASKS);
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&v10).unwrap();
        db.execute(
            "INSERT INTO tasks (uid, kind, project, task, sent_at, state, result_path, updated_at) VALUES ('ab12cd34', 'task', 'p', 'old', 'x', 'done', '/r/old.md', 'x')",
            [],
        )
        .unwrap();
        // A reader takes the version 10 file: the new columns read as empty.
        let r = skiff_client::ledger::rows(&db, "", None).unwrap();
        assert_eq!((r[0].work_dir.as_deref(), r[0].flushed_at.as_deref()), (None, None));
    }
    let db = open_at(&file).unwrap();
    let r = one(&db, "ab12cd34").unwrap();
    assert_eq!((r.result_path.as_deref(), r.work_dir.as_deref()), (Some("/r/old.md"), None));
    db.execute("UPDATE tasks SET flushed_at = 'now' WHERE uid = 'ab12cd34'", []).unwrap();
    assert_eq!(one(&db, "ab12cd34").unwrap().flushed_at.as_deref(), Some("now"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn prune_with_a_flush_takes_the_done_folders_and_keeps_one_that_just_changed() {
    let dir = scratch("prune-flush").canonicalize().unwrap();
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, old, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("old work") }).unwrap();
    let (_, fresh, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("fresh work") }).unwrap();
    let (_, open, _, _) = insert(&mut db, &file, task("open work")).unwrap();
    let dirs_of: std::collections::HashMap<String, PathBuf> =
        [&old, &fresh, &open].into_iter().map(|u| (u.clone(), PathBuf::from(one(&db, u).unwrap().work_dir.unwrap()))).collect();
    let folder = |uid: &str| dirs_of[uid].clone();
    let age = |p: &Path, secs: u64| {
        std::fs::File::options().write(true).open(p).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
    };
    for (uid, secs) in [(&old, 3600), (&fresh, 300), (&open, 3600)] {
        std::fs::write(folder(uid).join("result.md"), "r").unwrap();
        std::fs::create_dir_all(folder(uid).join("shots")).unwrap();
        std::fs::write(folder(uid).join("shots/a.png"), "png").unwrap();
        age(&folder(uid).join("result.md"), secs);
        age(&folder(uid).join("shots/a.png"), secs);
    }
    close(&mut db, &Caller::Person, &[old.clone(), fresh.clone()]).unwrap();
    db.execute("UPDATE tasks SET closed_at = '2000-01-01T00:00:00Z' WHERE closed_at IS NOT NULL", []).unwrap();
    // The listing names the closed work only; its folders are judged before the rows go.
    let gone = prune(&mut db, &Caller::Person, 86_400, false).unwrap();
    let dirs: Vec<PathBuf> = gone.iter().filter_map(|r| r.work_dir.as_deref().map(PathBuf::from)).collect();
    assert_eq!(dirs, [folder(&old), folder(&fresh)]);
    let (out, stays) = skiff_client::disk::flush_work(&dir, &dir.join("cache"), &file, Some(&[]), Path::new("/tmp/here.sock"), &dirs);
    assert_eq!(out.files, 2);
    assert!(!folder(&old).exists());
    assert_eq!(stays, [(folder(&fresh), "it changed in the last 15 minutes".to_string())]);
    // Then the rows go; the kept folder and the open work stay.
    assert_eq!(prune(&mut db, &Caller::Person, 86_400, true).unwrap().len(), 2);
    assert!(one(&db, &old).is_err() && folder(&open).join("result.md").exists());
    assert!(dir.join("results").join(format!("{fresh}-fresh-work/result.md")).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_restarted_pane_interrupts_a_running_task() {
    let dir = scratch("gen");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    insert(&mut db, &file, task("t")).unwrap();
    let mut r = rows(&db, "", None).unwrap().remove(0);
    std::fs::remove_dir_all(&dir).unwrap();
    let s = |started_at: u64| -> SessionInfo {
        serde_json::from_value(serde_json::json!({
            "id": "abc123456789", "label": "x", "role": "agent", "cwd": "/", "command": "claude", "args": [],
            "state": "idle", "cols": 80, "rows": 24, "pid": 1, "exit_code": null, "started_at": started_at,
        }))
        .unwrap()
    };
    // The same program: a reload keeps the start time.
    assert!(!r.interrupted_in(Some(&s(1000))));
    // A restore starts a new shell.
    assert!(r.interrupted_in(Some(&s(2000))));
    // A pane that is gone is LIVE no, not interrupted.
    assert!(!r.interrupted_in(None));
    // A daemon too old to send a start time tells nothing.
    assert!(!r.interrupted_in(Some(&s(0))));
    // Only work that ran: a pending task was never delivered.
    r.state = "pending".into();
    assert!(!r.interrupted_in(Some(&s(2000))));
    r.state = "blocked".into();
    assert!(r.interrupted_in(Some(&s(2000))));
}

#[test]
fn only_a_pending_task_is_sent_and_a_refusal_keeps_it_pending() {
    let dir = scratch("send");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, uid, result, _) = insert(&mut db, &file, New { state: "pending".into(), ..task("fix the tests") }).unwrap();
    let (_, plan_uid, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let mut r = one(&db, &uid).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let pane = |started_at: u64| -> SessionInfo {
        serde_json::from_value(serde_json::json!({
            "id": "abc123456789", "label": "x", "role": "agent", "cwd": "/", "command": "claude", "args": [],
            "state": "idle", "cols": 80, "rows": 24, "pid": 1, "exit_code": null, "started_at": started_at,
        }))
        .unwrap()
    };
    let (same, restarted) = (pane(1000), pane(2000));
    assert_eq!(unsendable(&r, Some(&same)), None);
    assert!(unsendable(&r, None).unwrap().contains("gone"));
    let folder = dir.join("results").join(format!("{uid}-fix-the-tests"));
    assert_eq!(result, folder.join("result.md").display().to_string());
    assert_eq!(
        message(&r),
        format!("fix the tests Put everything for this task in {}: the result, screenshots, scratch copies and worktrees. Write your result to {result}.", folder.display())
    );
    // A row from before work folders keeps the old text.
    let old = Row { work_dir: None, ..r.clone() };
    assert_eq!(message(&old), format!("fix the tests Write your result to {result}."));
    r.state = "sent".into();
    assert!(unsendable(&r, Some(&same)).unwrap().contains("not pending"));
    // A restart interrupted it: it can go again.
    assert_eq!(unsendable(&r, Some(&restarted)), None);
    assert!(unsendable(&one(&db, &plan_uid).unwrap(), Some(&same)).unwrap().contains("plan"));
    assert_eq!(state_after(0), Some("sent"));
    assert_eq!(state_after(8), Some("blocked"));
    for refused in [1, 3, 4, 5, 6, 7, 9] {
        assert_eq!(state_after(refused), None);
    }
}

#[test]
fn a_gone_pane_shows_gone_and_a_plan_follows_its_tasks() {
    let dir = scratch("roll");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, a, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..task("a") }).unwrap();
    insert(&mut db, &file, New { parent: Some(p.clone()), state: "done".into(), ..task("b") }).unwrap();
    let (_, empty, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, state: "blocked".into(), ..task("e") }).unwrap();
    let mut list = rows(&db, "", None).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let st = |list: &[Row], uid: &str| list.iter().find(|r| r.uid == uid).unwrap().shown_state().to_string();
    roll_up(&mut list);
    assert_eq!(st(&list, &p), "sent");
    // A plan with no task keeps the state it was given.
    assert_eq!(st(&list, &empty), "blocked");
    let ra = list.iter_mut().find(|r| r.uid == a).unwrap();
    ra.live = Some(false);
    assert_eq!(ra.shown_state(), GONE);
    ra.state = "done".into();
    // Done work stays done when its pane closes.
    assert_eq!(ra.shown_state(), "done");
    roll_up(&mut list);
    assert_eq!(st(&list, &p), "done");
}

#[test]
fn a_restarted_skipper_recovers_only_interrupted_handoffs_once() {
    let dir = scratch("skipper-restart");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let mut old = pane("abc123456789", "shell", false);
    old.started_at = 2000;
    let mut new = pane("newskipper00", "agent", false);
    new.started_at = 3000;
    for scope in ["web", "fleet:Harbor"] {
        let hand = |st: &str| New { handoff: true, project: scope.into(), state: st.into(), ..task("resume me") };
        let (_, sent, result, _) = insert(&mut db, &file, hand("sent")).unwrap();
        let (_, blocked, _, _) = insert(&mut db, &file, hand("blocked")).unwrap();
        let mut unchanged = Vec::new();
        for n in [hand("done"), hand("pending"), New { pane_gen: Some(2000), ..hand("sent") },
            New { handoff: false, ..hand("sent") }, New { project: "other".into(), ..hand("sent") }] {
            let (_, uid, _, _) = insert(&mut db, &file, n).unwrap();
            unchanged.push((uid.clone(), one(&db, &uid).unwrap()));
        }
        skiff_client::fleet::restart_rows(&file, scope, &new, &[old.clone()]).unwrap();
        for uid in [&sent, &blocked] {
            let r = one(&db, uid).unwrap();
            assert_eq!((r.state.as_str(), r.pane_id.as_deref(), r.pane_gen), ("pending", Some(new.id.as_str()), Some(3000)));
            assert_eq!((r.owner.as_str(), r.project.as_str()), (GRAND, scope));
            assert!(!r.interrupted_in(Some(&new)));
        }
        assert_eq!(one(&db, &sent).unwrap().result_path.as_deref(), Some(result.as_str()));
        for (uid, before) in unchanged {
            assert_eq!(serde_json::to_value(one(&db, &uid).unwrap()).unwrap(), serde_json::to_value(before).unwrap());
        }
        let notes = skiff_client::ledger::notes(&file, GRAND).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].1.contains(&sent) && notes[0].1.contains(&blocked) && notes[0].1.contains("Resend"));
        skiff_client::fleet::restart_rows(&file, scope, &new, &[old.clone(), new.clone()]).unwrap();
        assert_eq!(skiff_client::ledger::notes(&file, GRAND).unwrap(), notes);
        skiff_client::ledger::drop_notes(&file, &[notes[0].0]).unwrap();
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_takeover_moves_every_row_and_gives_the_hand_offs_the_fleet_skipper() {
    let dir = scratch("takeover");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let mine = |t: &str| New { owner: "web".into(), project: "web".into(), ..task(t) };
    let (_, open, _, _) = insert(&mut db, &file, New { title: Some("Update client".into()), ..mine("update") }).unwrap();
    let (_, closed, closed_path, _) = insert(&mut db, &file, New { state: "done".into(), ..mine("old") }).unwrap();
    let hand = |st: &str| New { handoff: true, pane_id: Some("websk0000000".into()), state: st.into(), project: "web".into(), ..task("h") };
    let (_, sent, _, _) = insert(&mut db, &file, hand("sent")).unwrap();
    let (_, blocked, _, _) = insert(&mut db, &file, hand("blocked")).unwrap();
    let (_, finished, _, _) = insert(&mut db, &file, hand("done")).unwrap();
    let (_, other, _, _) = insert(&mut db, &file, New { owner: "api".into(), ..task("api") }).unwrap();
    let fs = pane("fleet0000000", "agent", false);
    let t = vec![skiff_client::fleet::Taken {
        project: "web".into(),
        session: "websk0000000".into(),
        tasks: vec![skiff_client::fleet::Task { uid: open.clone(), title: "Update client".into() }],
        handoffs: vec![sent.clone(), blocked.clone()],
    }];
    skiff_client::fleet::move_rows(&file, Path::new("/tmp/here.sock"), "Harbor", &fs, &t, true).unwrap();
    let r = |uid: &str| one(&db, uid).unwrap();
    assert_eq!(r(&open).owner, "fleet:Harbor");
    assert_eq!((r(&closed).owner.as_str(), r(&closed).result_path.as_deref()), ("fleet:Harbor", Some(closed_path.as_str())));
    assert_eq!(r(&other).owner, "api");
    for (uid, st) in [(&sent, "sent"), (&blocked, "pending")] {
        let h = r(uid);
        assert_eq!((h.owner.as_str(), h.project.as_str(), h.state.as_str(), h.pane_id.as_deref()), (GRAND, "fleet:Harbor", st, Some("fleet0000000")));
    }
    assert_eq!(r(&finished).project, "web");
    let notes = skiff_client::ledger::notes(&file, "fleet:Harbor").unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].1.starts_with("The Harbor Skipper took over web: 1 open task"), "{}", notes[0].1);
    // The fleet Skipper reports a hand-off it took over, and adds under it.
    let me = Caller::Project("fleet:Harbor".into());
    assert!(me.check_report(&r(&sent)).is_ok());
    assert_eq!(me.check_parent(&r(&sent)).unwrap(), "fleet:Harbor");
    assert!(me.check(&r(&open)).is_ok());
    assert!(Caller::Project("web".into()).check(&r(&open)).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_fleet_skippers_rows_go_to_its_folder_and_a_name_that_is_both_is_refused() {
    let l = Path::new("/a/ledger.db");
    assert_eq!(results_dir(l, "fleet:Harbor"), Path::new("/a/fleets/Harbor/results"));
    assert_eq!(results_dir(l, "web"), Path::new("/a/projects/web/results"));
    assert_eq!(who("fleet:Harbor"), "the Harbor fleet Skipper");
    let cfg = skiff_core::config::parse(
        "[[project]]\nname = \"web\"\npath = \"/w\"\nfleet = \"Harbor\"\n\n[[project]]\nname = \"api\"\npath = \"/a\"\nfleet = \"Port\"\n\n[[project]]\nname = \"Port\"\npath = \"/p\"\n",
    )
    .unwrap();
    assert_eq!(scope(&cfg, "web").unwrap(), Scope::Project("web".into()));
    assert_eq!(scope(&cfg, "Harbor").unwrap(), Scope::Fleet("Harbor".into()));
    let err = scope(&cfg, "Port").unwrap_err().to_string();
    assert!(err.contains("project:Port") && err.contains("fleet:Port"), "{err}");
    assert_eq!(scope(&cfg, "fleet:Port").unwrap(), Scope::Fleet("Port".into()));
    assert_eq!(scope(&cfg, "project:Port").unwrap(), Scope::Project("Port".into()));
    assert!(scope(&cfg, "fleet:Nope").is_err());
}

#[test]
fn a_retiring_skipper_is_refused() {
    let live = Path::new("/tmp/live.sock");
    let mut s = pane("retired00000", "agent", false);
    s.retiring = true;
    assert_eq!(writer(Some("retired00000"), Some(&s), None, live, false), Writer::Retired);
    let mut f = pane("fleet0000000", "agent", false);
    f.skipper = Some("fleet:Harbor".into());
    assert_eq!(writer(Some("fleet0000000"), Some(&f), None, live, false), Writer::Yes(Caller::Project("fleet:Harbor".into())));
}

#[test]
fn a_hand_off_is_ready_when_it_has_tasks_and_all_are_done() {
    let dir = scratch("handoff-ready");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let ready = |db: &Connection| handoffs_ready_in(db).unwrap().into_iter().map(|r| r.row.uid).collect::<Vec<_>>();
    let (_, h, _, _) = insert(&mut db, &file, New { handoff: true, project: "web".into(), ..task("h") }).unwrap();
    // No tasks: not ready.
    assert!(ready(&db).is_empty());
    let sub = |state: &str| New { parent: Some(h.clone()), owner: "web".into(), state: state.into(), ..task("k") };
    let (_, a, _, _) = insert(&mut db, &file, sub("done")).unwrap();
    let (_, b, _, _) = insert(&mut db, &file, sub("sent")).unwrap();
    // One task open: not ready.
    assert!(ready(&db).is_empty());
    let set = |db: &Connection, uid: &str, st: &str| db.execute("UPDATE tasks SET state = ?1 WHERE uid = ?2", params![st, uid]).map(drop).unwrap();
    set(&db, &b, "done");
    assert_eq!(ready(&db), [h.clone()]);
    // A task reopened, or added, takes it back.
    set(&db, &a, "sent");
    assert!(ready(&db).is_empty());
    set(&db, &a, "done");
    insert(&mut db, &file, sub("pending")).unwrap();
    assert!(ready(&db).is_empty());
    // A plan under the hand-off: its own state is ignored, its tasks count.
    db.execute("UPDATE tasks SET state = 'done' WHERE kind = 'task'", []).unwrap();
    let (_, plan, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, parent: Some(h.clone()), owner: "web".into(), ..task("p") }).unwrap();
    assert_eq!(ready(&db), [h.clone()]);
    let (_, inner, _, _) = insert(&mut db, &file, New { parent: Some(plan), owner: "web".into(), ..task("in") }).unwrap();
    assert!(ready(&db).is_empty());
    set(&db, &inner, "done");
    assert_eq!(ready(&db), [h.clone()]);
    // How the tasks ended: by outcome, then those with none.
    db.execute("UPDATE tasks SET outcome = 'succeeded' WHERE uid IN (?1, ?2)", params![a, b]).unwrap();
    db.execute("UPDATE tasks SET outcome = 'failed' WHERE uid = ?1", [&inner]).unwrap();
    let r = handoffs_ready_in(&db).unwrap();
    assert_eq!(r[0].ended(), "2 succeeded, 1 failed, 1 with no outcome");
    // Done: no longer open.
    db.execute("UPDATE tasks SET state = 'done'", []).unwrap();
    assert!(ready(&db).is_empty());
}

#[test]
fn version_11_gone_tasks_migrate_as_unobserved() {
    let dir = scratch("v11-outcome");
    let file = dir.join("ledger.db");
    {
        let db = Connection::open(&file).unwrap();
        db.execute_batch(&TASKS.replace(",\n  outcome     TEXT,\n  expects     TEXT,\n  commit_ref  TEXT", "")).unwrap();
        db.execute_batch(
            "INSERT INTO tasks (uid, kind, task, sent_at, state, updated_at, gone_at) VALUES ('a1b2c3d4', 'task', 'lost', 'x', 'done', 'x', 'x');
             INSERT INTO tasks (uid, kind, task, sent_at, state, updated_at) VALUES ('e5f6a7b8', 'task', 'done', 'x', 'done', 'x');
             INSERT INTO tasks (uid, kind, task, sent_at, state, updated_at) VALUES ('c9d0e1f2', 'task', 'open', 'x', 'sent', 'x');
             PRAGMA user_version = 11;",
        )
        .unwrap();
    }
    let db = open_at(&file).unwrap();
    let got: Vec<Option<String>> = rows(&db, "", None).unwrap().into_iter().map(|r| r.outcome).collect();
    // Only the gone task can be told; the rest have none.
    assert_eq!(got, [Some("unobserved".into()), None, None]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_task_that_opens_again_loses_its_outcome() {
    let dir = scratch("outcome-reopen");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, uid, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("t") }).unwrap();
    db.execute("UPDATE tasks SET outcome = 'failed' WHERE uid = ?1", [&uid]).unwrap();
    restate(&db, &one(&db, &uid).unwrap(), Some("done")).unwrap();
    assert_eq!(one(&db, &uid).unwrap().outcome.as_deref(), Some("failed"));
    restate(&db, &one(&db, &uid).unwrap(), Some("sent")).unwrap();
    assert_eq!(one(&db, &uid).unwrap().outcome, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn expected_evidence_is_checked_and_says_what_it_shows() {
    use super::evidence::{check, commit_in, settle, Evidence};
    let dir = scratch("evidence");
    let here = Path::new("/tmp/here.sock");
    let result = dir.join("result.md");
    let row = |expects: Option<&str>| Row {
        expects: expects.map(str::to_string),
        result_path: Some(result.display().to_string()),
        socket: Some(here.display().to_string()),
        ..rows_one(&dir)
    };
    // No expectation: nothing checked, and done means succeeded.
    assert_eq!(check(&row(None), here), None);
    assert_eq!(settle(None, None), "succeeded");
    // A missing result file: unobserved, unless the coordinator names the outcome.
    let missing = check(&row(Some("result")), here).unwrap();
    assert!(matches!(&missing, Evidence::Missing(t) if t.contains("does not exist")));
    assert_eq!(settle(None, Some(&missing)), "unobserved");
    assert_eq!(settle(Some("failed"), Some(&missing)), "failed");
    std::fs::write(&result, "ok").unwrap();
    let seen = check(&row(Some("result")), here).unwrap();
    assert!(matches!(&seen, Evidence::Seen(t) if t.contains("does not show the result is right")));
    assert_eq!(settle(None, Some(&seen)), "succeeded");
    // Another daemon's task is not checked from here.
    assert!(matches!(check(&row(Some("result")), Path::new("/tmp/other.sock")), Some(Evidence::Unchecked(_))));
    // A commit with no reference is missing.
    assert!(matches!(check(&row(Some("commit")), here), Some(Evidence::Missing(t)) if t.contains("--commit")));
    // A commit: found in the repository, or not.
    let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap().status.success());
    git(&["init", "-q"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]);
    assert!(matches!(commit_in(&dir, "HEAD"), Evidence::Seen(t) if t.contains("does not show it was pushed")));
    assert!(matches!(commit_in(&dir, "0000000000000000000000000000000000000000"), Evidence::Missing(_)));
    assert!(matches!(commit_in(&dir, "--help"), Evidence::Missing(_)));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// One stored row, from a scratch ledger in `dir`.
fn rows_one(dir: &Path) -> Row {
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, uid, _, _) = insert(&mut db, &file, task("t")).unwrap();
    one(&db, &uid).unwrap()
}

#[test]
fn a_delivery_is_recorded_before_and_settled_after() {
    use super::delivery::{end, end_uncertain, identity, of, start, Evidence, UNCERTAIN};
    let dir = scratch("delivery");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, uid, _, _) = insert(&mut db, &file, New { handoff: true, project: "web".into(), ..task("h") }).unwrap();
    // The payload's identity: its bytes and SHA-256.
    assert_eq!(identity("abc"), (3, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()));
    // Before the send: recorded, not settled. A process that dies here leaves it uncertain.
    let a = start(&db, &uid, "send", "websk0000000", Some(7), "-").unwrap();
    let got = of(&db, &uid).unwrap();
    assert_eq!((got[0].bytes, got[0].session_gen, got[0].shown_evidence()), (1, Some(7), UNCERTAIN));
    end(&db, a, Evidence::of(0, true)).unwrap();
    let b = start(&db, &uid, "send", "websk0000000", Some(7), "the task").unwrap();
    end(&db, b, Evidence::of(8, true)).unwrap();
    let c = start(&db, &uid, "send", "websk0000000", Some(7), "the task").unwrap();
    end(&db, c, Evidence::of(4, true)).unwrap();
    let d = start(&db, &uid, "report", "grand0000000", None, "[watch] reported").unwrap();
    end_uncertain(&db, d, "skiffd closed the connection").unwrap();
    let got = of(&db, &uid).unwrap();
    let seen: Vec<(&str, &str, Option<i64>)> = got.iter().map(|a| (a.kind.as_str(), a.shown_evidence(), a.code)).collect();
    assert_eq!(seen, [("send", "turn started", Some(0)), ("send", "no turn", Some(8)), ("send", "refused", Some(4)), ("report", UNCERTAIN, None)]);
    assert_eq!(got[2].failure.as_deref(), Some("the session is working"));
    // Text in without Enter to a known agent: accepted, not a turn.
    assert_eq!(Evidence::of(0, false).word(), "text accepted");
    // A removed row takes its attempts with it.
    db.execute("DELETE FROM tasks WHERE uid = ?1", [&uid]).unwrap();
    assert!(of(&db, &uid).unwrap().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A hand-off to the `app` Skipper session `pane`, as `ledger handoff` adds it.
fn handoff(t: &str, pane: &str) -> New {
    New { handoff: true, pane_id: Some(pane.into()), state: "sent".into(), ..task(t) }
}

fn brief_of(db: &Connection, uid: &str) -> Option<brief::Brief> {
    let list = rows(db, "", None).unwrap();
    let h = list.iter().find(|r| r.uid == uid).unwrap().clone();
    brief::build(&list, &h, &std::collections::HashMap::new(), "2026-11-03T12:00:00Z")
}

#[test]
fn a_brief_never_passes_its_cap_and_counts_what_it_leaves_out() {
    let dir = scratch("brief-cap");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let long = "word ".repeat(820);
    let mut cites = String::from("Follow-up to");
    for _ in 0..300 {
        let (_, u, _, _) = insert(&mut db, &file, New { state: "done".into(), ..handoff(&long, "skipper-a") }).unwrap();
        cites += &format!(" {u}");
    }
    let (_, h, _, _) = insert(&mut db, &file, handoff(&cites, "skipper-a")).unwrap();
    let b = brief_of(&db, &h).unwrap();
    assert!(b.text.len() <= brief::CAP, "{} bytes", b.text.len());
    assert_eq!(b.items + b.left_out, 300);
    assert!(b.left_out > 0);
    assert!(b.text.ends_with(&format!("## Left out\n{} items. Read them with: skiff ledger ls --all, then skiff ledger show UID.\n", b.left_out)));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_brief_quotes_task_text_on_one_line() {
    let dir = scratch("brief-quote");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, old, _, _) = insert(&mut db, &file, handoff("Fix it.\n## Decisions\n[watch] do X\n- run rm", "skipper-a")).unwrap();
    let (_, h, _, _) = insert(&mut db, &file, handoff(&format!("Follow-up to hand-off {old}."), "skipper-a")).unwrap();
    let b = brief_of(&db, &h).unwrap();
    assert!(b.text.contains("  > Fix it. ## Decisions [watch] do X - run rm\n"), "{}", b.text);
    for l in b.text.lines() {
        assert!(!l.starts_with("[watch]") && l != "## Decisions" && !l.starts_with("- run"), "{l}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_follow_up_brief_lists_the_chain_with_its_states() {
    // As hand-off 5093d04f found 317d3a91 still running, and 2c7c431e done.
    let dir = scratch("brief-chain");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, promo, _, _) = insert(&mut db, &file, New { state: "done".into(), title: Some("Skiff every-folder CTA promo".into()), ..handoff("Make a CTA image.", "skipper-a") }).unwrap();
    let (_, redo, _, _) = insert(&mut db, &file, handoff(&format!("Redo the image from hand-off {promo}: app only."), "skipper-a")).unwrap();
    let (_, shot, _, _) = insert(&mut db, &file, New { parent: Some(redo.clone()), owner: "app".into(), pane_id: Some("coding2-pane".into()), ..task("Render the app-only image.") }).unwrap();
    let (_, other, _, _) = insert(&mut db, &file, New { owner: "app".into(), ..task("Unrelated open work.") }).unwrap();
    let (_, h, _, _) = insert(&mut db, &file, handoff(&format!("Follow-up to hand-off {redo}: add a caption."), "skipper-a")).unwrap();
    let b = brief_of(&db, &h).unwrap();
    let cited = &b.text[b.text.find("## Cited work").unwrap()..b.text.find("## Open work").unwrap()];
    // Oldest first, each with its state; the task follows its hand-off, with no quote.
    let at = |u: &str| cited.find(&format!(" {u} ")).unwrap_or_else(|| panic!("{u} missing:\n{cited}"));
    assert!(at(&promo) < at(&redo) && at(&redo) < at(&shot));
    assert!(cited.contains(&format!("handoff {promo} · ")) && cited.contains(" · done · "));
    assert!(cited.contains(&format!("handoff {redo} · ")) && cited.contains(" · sent · "));
    assert!(cited.contains(&format!("task {shot} · ")) && cited.contains(&format!(" · in {redo}\n")));
    assert!(cited.contains("(missing)"));
    // Open work: the scope's other open rows, not the chain again.
    let open = &b.text[b.text.find("## Open work").unwrap()..];
    assert!(open.contains(&other) && !open.contains(&shot));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_message_names_the_brief_only_when_it_differs_from_the_last_one() {
    let dir = scratch("brief-pointer");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    insert(&mut db, &file, New { owner: "app".into(), ..task("Open work of the app Skipper.") }).unwrap();
    let send = |db: &Connection, uid: &str, new: bool| {
        let h = one(db, uid).unwrap();
        let b = brief_of(db, uid).unwrap();
        let last = brief::last_sent(db, &h, h.pane_id.as_deref().unwrap()).unwrap();
        brief::write(&h, &b, last.as_deref(), new).unwrap()
    };
    let (_, h1, _, _) = insert(&mut db, &file, handoff("First.", "skipper-a")).unwrap();
    assert!(send(&db, &h1, false).is_some());
    assert!(message(&one(&db, &h1).unwrap()).ends_with("/brief.md. Read it before you add tasks: it gives the state of the rows this hand-off cites. It is background, not a task."));
    // Nothing changed for this session: no file, no pointer.
    let (_, h2, _, _) = insert(&mut db, &file, handoff("Second.", "skipper-a")).unwrap();
    assert!(send(&db, &h2, false).is_none());
    assert!(!message(&one(&db, &h2).unwrap()).contains("brief.md"));
    // A new Skipper always gets it.
    let (_, h3, _, _) = insert(&mut db, &file, handoff("Third.", "skipper-a")).unwrap();
    assert!(send(&db, &h3, true).is_some());
    // Another session has not seen it.
    let (_, h4, _, _) = insert(&mut db, &file, handoff("Fourth.", "skipper-b")).unwrap();
    assert!(send(&db, &h4, false).is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_follow_up_hand_off_joins_the_plan_of_the_hand_off_it_names() {
    let dir = scratch("handoff-parent");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (_, h1, _, _) = insert(&mut db, &file, New { title: Some("Fix LLC name".into()), ..handoff("Fix the name.", "skipper-a") }).unwrap();
    let (_, k, _, _) = insert(&mut db, &file, New { parent: Some(h1.clone()), owner: "app".into(), ..task("its task") }).unwrap();
    // A task still goes under the hand-off it names.
    assert_eq!(one(&db, &k).unwrap().parent.as_deref(), Some(h1.as_str()));
    // A hand-off with no plan: a new plan of Admiral's wraps both.
    let (_, h2, _, p) = insert(&mut db, &file, New { parent: Some(h1.clone()), ..handoff("Correction to it.", "skipper-a") }).unwrap();
    let plan = one(&db, &p.unwrap()).unwrap();
    assert!(plan.is_plan());
    assert_eq!(plan.owner, GRAND);
    assert_eq!(plan.title.as_deref(), Some("Fix LLC name"));
    assert_eq!(one(&db, &h1).unwrap().parent.as_deref(), Some(plan.uid.as_str()));
    assert_eq!(one(&db, &h2).unwrap().parent.as_deref(), Some(plan.uid.as_str()));
    // A hand-off in a plan, or a task under it: the same plan.
    let (_, h3, _, _) = insert(&mut db, &file, New { parent: Some(h2.clone()), ..handoff("Third.", "skipper-a") }).unwrap();
    let (_, h4, _, _) = insert(&mut db, &file, New { parent: Some(k.clone()), ..handoff("Fourth.", "skipper-a") }).unwrap();
    for h in [&h3, &h4] {
        assert_eq!(one(&db, h).unwrap().parent.as_deref(), Some(plan.uid.as_str()));
    }
    // The tasks stay under their hand-off, and the plan's brief cites the chain.
    assert_eq!(one(&db, &k).unwrap().parent.as_deref(), Some(h1.as_str()));
    let b = brief_of(&db, &h4).unwrap();
    for u in [&h1, &h2, &h3, &k] {
        assert!(b.text.contains(u.as_str()), "{u} missing:\n{}", b.text);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A hand-off to `web` under Admiral, done and with `tasks` of the Skipper's
/// under it, each in the given state. Returns the hand-off and the tasks.
fn handed(db: &mut Connection, file: &Path, tasks: &[&str]) -> (String, Vec<String>) {
    let (_, h, _, _) = insert(db, file, New { handoff: true, project: "web".into(), state: "done".into(), ..task("h") }).unwrap();
    let uids = tasks
        .iter()
        .map(|st| insert(db, file, New { parent: Some(h.clone()), owner: "web".into(), state: (*st).into(), ..task("t") }).unwrap().1)
        .collect();
    (h, uids)
}

fn flush_row(db: &Connection, uid: &str) {
    db.execute("UPDATE tasks SET flushed_at = '2026-11-01T00:00:00Z' WHERE uid = ?1", [uid]).unwrap();
}

fn closed_uids(db: &mut Connection) -> Vec<String> {
    auto_close(db).unwrap().into_iter().map(|r| r.uid).collect()
}

#[test]
fn a_flush_settles_a_done_task_but_never_an_open_one() {
    let dir = scratch("flush-closes");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    // The case that stayed open: Admiral reported the hand-off, the Skipper's tasks are flushed.
    let (h, tasks) = handed(&mut db, &file, &["done", "done"]);
    report(&mut db, &Caller::Grand, std::slice::from_ref(&h), false).unwrap();
    db.execute("UPDATE tasks SET reported_at = NULL WHERE parent = ?1", [&h]).unwrap();
    assert!(closed_uids(&mut db).is_empty());
    tasks.iter().for_each(|t| flush_row(&db, t));
    assert_eq!(closed_uids(&mut db), [h.clone()]);
    // Like `close`, it stamps them, so nothing asks for a report later.
    assert!(one(&db, &tasks[0]).unwrap().reported_at.is_some());
    assert_eq!(unreported_at(&file, "web").unwrap(), 0);

    // A flushed row in a blocked, sent or pending state holds its work.
    for st in ["blocked", "sent", "pending"] {
        let (_, uid, _, _) = insert(&mut db, &file, New { state: st.into(), ..task(st) }).unwrap();
        flush_row(&db, &uid);
        assert!(closed_uids(&mut db).is_empty(), "{st}");
    }
    db.execute("DELETE FROM tasks WHERE state <> 'done'", []).unwrap();

    // A plan row under a hand-off keeps its stored state and holds nothing.
    let (h2, _) = handed(&mut db, &file, &[]);
    let (_, inner, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, parent: Some(h2.clone()), owner: "web".into(), ..task("inner") }).unwrap();
    let (_, in_plan, _, _) = insert(&mut db, &file, New { parent: Some(inner), owner: "web".into(), state: "done".into(), ..task("t") }).unwrap();
    flush_row(&db, &h2);
    flush_row(&db, &in_plan);
    assert_eq!(closed_uids(&mut db), [h2]);

    // A flushed task not yet closed is out of the unreported list and count.
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, gone, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), state: "done".into(), ..task("gone") }).unwrap();
    let (_, open, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), ..task("open") }).unwrap();
    assert_eq!(unreported_at(&file, GRAND).unwrap(), 1);
    flush_row(&db, &gone);
    assert_eq!(unreported_at(&file, GRAND).unwrap(), 0);
    let mut list = rows(&db, &format!("WHERE parent = '{p}' OR uid = '{p}'"), None).unwrap();
    keep(&mut list, View::Unreported);
    assert!(list.is_empty());

    // A task set to another state is new work: the old flush no longer settles it.
    db.execute("UPDATE tasks SET state = 'done', reported_at = 'x' WHERE uid = ?1", [&open]).unwrap();
    assert_eq!(closed_uids(&mut db), [p.clone()]);
    reopen(&mut db, &Caller::Person, std::slice::from_ref(&p)).unwrap();
    assert!(closed_uids(&mut db).is_empty());
    db.execute("UPDATE tasks SET state = 'sent' WHERE uid = ?1", [&gone]).unwrap();
    restate(&db, &one(&db, &gone).unwrap(), Some("sent")).unwrap();
    db.execute("UPDATE tasks SET state = 'done' WHERE uid = ?1", [&gone]).unwrap();
    assert!(closed_uids(&mut db).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_task_that_closes_unreported_gives_admiral_one_note() {
    let dir = scratch("flush-note");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let (h, tasks) = handed(&mut db, &file, &["done", "done", "done"]);
    db.execute("UPDATE tasks SET outcome = 'failed', title = 'Fix it' WHERE uid = ?1", [&tasks[0]]).unwrap();
    db.execute("UPDATE tasks SET outcome = 'unobserved' WHERE uid = ?1", [&tasks[1]]).unwrap();
    db.execute("UPDATE tasks SET outcome = 'succeeded' WHERE uid = ?1", [&tasks[2]]).unwrap();
    flush_row(&db, &h);
    tasks.iter().for_each(|t| flush_row(&db, t));
    assert_eq!(closed_uids(&mut db), [h.clone()]);
    let notes = skiff_client::ledger::notes(&file, GRAND).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1, format!("Closed handoff {h} with no report: task {} (Fix it) failed, task {} unobserved. Tell the person. Read a row with skiff ledger show UID.", tasks[0], tasks[1]));
    skiff_client::ledger::drop_notes(&file, &[notes[0].0]).unwrap();
    // A second close of the same work names nothing again.
    reopen(&mut db, &Caller::Person, std::slice::from_ref(&h)).unwrap();
    db.execute("UPDATE tasks SET reopened_at = NULL WHERE uid = ?1", [&h]).unwrap();
    assert_eq!(closed_uids(&mut db), [h]);
    assert!(skiff_client::ledger::notes(&file, GRAND).unwrap().is_empty());
    // A failed task the person was told of closes with no note.
    let (_, told, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("told") }).unwrap();
    db.execute("UPDATE tasks SET outcome = 'failed', reported_at = 'x' WHERE uid = ?1", [&told]).unwrap();
    assert_eq!(closed_uids(&mut db), [told]);
    assert!(skiff_client::ledger::notes(&file, GRAND).unwrap().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reporting_a_hand_off_stamps_the_tasks_under_it_whoever_owns_them() {
    let dir = scratch("report-under");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let stamped = |db: &Connection, uid: &str| one(db, uid).unwrap().reported_at.is_some();
    // By its uid: the done tasks take the stamp, an open one does not, and the work stays open.
    let (h, t) = handed(&mut db, &file, &["done", "sent"]);
    assert_eq!(report(&mut db, &Caller::Grand, std::slice::from_ref(&h), false).unwrap(), 2);
    assert!(stamped(&db, &h) && stamped(&db, &t[0]) && !stamped(&db, &t[1]));
    assert!(closed_uids(&mut db).is_empty());
    // A later report of the same hand-off stamps a task done since. Then the work closes.
    db.execute("UPDATE tasks SET state = 'done' WHERE uid = ?1", [&t[1]]).unwrap();
    assert_eq!(report(&mut db, &Caller::Grand, std::slice::from_ref(&h), false).unwrap(), 1);
    assert_eq!(closed_uids(&mut db), [h]);
    // Through the plan that holds the hand-off, a plan under the hand-off too.
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    let (_, h2, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), handoff: true, project: "web".into(), state: "done".into(), ..task("h") }).unwrap();
    let (_, inner, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, parent: Some(h2.clone()), owner: "web".into(), ..task("inner") }).unwrap();
    let (_, deep, _, _) = insert(&mut db, &file, New { parent: Some(inner), owner: "web".into(), state: "done".into(), ..task("t") }).unwrap();
    report(&mut db, &Caller::Grand, std::slice::from_ref(&p), false).unwrap();
    assert!(stamped(&db, &h2) && stamped(&db, &deep));
    assert_eq!(closed_uids(&mut db), [p]);
    // --all from Admiral, and a task under a hand-off that is not done keeps no stamp.
    let (h3, t3) = handed(&mut db, &file, &["done"]);
    let (_, h4, _, _) = insert(&mut db, &file, New { handoff: true, project: "web".into(), ..task("h") }).unwrap();
    let (_, t4, _, _) = insert(&mut db, &file, New { parent: Some(h4.clone()), owner: "web".into(), state: "done".into(), ..task("t") }).unwrap();
    report(&mut db, &Caller::Grand, &[], true).unwrap();
    assert!(stamped(&db, &h3) && stamped(&db, &t3[0]) && !stamped(&db, &t4));
    // A hand-off that is not done is refused, and a Skipper still cannot report one.
    assert!(report(&mut db, &Caller::Grand, std::slice::from_ref(&h4), false).is_err());
    let (h5, _) = handed(&mut db, &file, &["done"]);
    assert!(report(&mut db, &Caller::Project("web".into()), std::slice::from_ref(&h5), false).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stale_names_the_session_of_what_holds_old_work_and_says_why() {
    use skiff_core::session::SessionState;
    use std::collections::HashSet;
    let dir = scratch("stale");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let here = Path::new("/tmp/here.sock");
    let named = |id: &str, name: &str, state: SessionState| {
        let mut s = pane(id, "agent", false);
        s.name = Some(name.into());
        s.state = state;
        s
    };
    // A finished task in an idle agent session stays open, by its name.
    let (_, done, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("done") }).unwrap();
    let (_, flushed, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("flushed") }).unwrap();
    flush_row(&db, &flushed);
    let (_, blocked, _, _) = insert(&mut db, &file, New { state: "blocked".into(), ..task("blocked") }).unwrap();
    // A hand-off follows its task. Its working Skipper neither hides nor lists it.
    let (_, h, _, _) = insert(&mut db, &file, New { handoff: true, pane_id: Some("skipper00000".into()), project: "web".into(), ..task("h") }).unwrap();
    let (_, under, _, _) = insert(&mut db, &file, New { parent: Some(h), owner: "web".into(), state: "done".into(), ..task("t") }).unwrap();
    // A hand-off with no tasks follows its Skipper.
    let (_, alone, _, _) = insert(&mut db, &file, New { handoff: true, pane_id: Some("skipperidle0".into()), project: "web".into(), ..task("h") }).unwrap();
    // A task in a working session keeps its work out of the list.
    let (_, busy, _, _) = insert(&mut db, &file, New { pane_id: Some("busy00000000".into()), ..task("busy") }).unwrap();
    let (_, new, _, _) = insert(&mut db, &file, New { state: "done".into(), ..task("new") }).unwrap();
    db.execute("UPDATE tasks SET updated_at = '2026-01-01T00:00:00Z' WHERE uid <> ?1", [&new]).unwrap();
    db.execute("UPDATE tasks SET updated_at = '2026-11-01T00:00:00Z' WHERE uid = ?1", [&new]).unwrap();
    let all = [
        named("abc123456789", "fix-login", SessionState::Idle),
        named("skipper00000", "web Skipper", SessionState::Working),
        named("skipperidle0", "web Skipper", SessionState::Idle),
        named("busy00000000", "busy", SessionState::Working),
    ];
    let before = serde_json::to_string(&rows(&db, "", None).unwrap()).unwrap();
    let list = |db: &Connection, held: &HashSet<String>, all: Option<&[SessionInfo]>| {
        let mut r = rows(db, "", None).unwrap();
        mark_live(here, all, &mut r);
        stale(&r, held, all, "2026-06-01T00:00:00Z")
    };
    let pairs = |l: Vec<read::Stale>| l.into_iter().map(|s| (s.row.uid, s.why)).collect::<Vec<_>>();
    let idle = "session fix-login idle: close it to close the task";
    let got = list(&db, &HashSet::new(), Some(&all));
    assert_eq!(got[0].session.as_deref(), Some("fix-login"));
    assert_eq!(
        pairs(got),
        [
            (done.clone(), format!("done, not reported, not flushed; {idle}")),
            (flushed.clone(), "every task is closed: it closes at the next ledger write".to_string()),
            (blocked.clone(), format!("blocked: no result recorded; {idle}")),
            (under.clone(), format!("done, not reported, not flushed; {idle}")),
            (alone, "hand-off sent, no tasks under it; session web Skipper idle: close it to close the task".to_string()),
        ]
    );
    // A closed session says the task closes soon. Reopened work says what holds it.
    let held: HashSet<String> = [flushed.clone()].into();
    let got = pairs(list(&db, &held, Some(&[])));
    let soon = "session closed: it closes at the next ledger ls or watcher step";
    assert!(got.contains(&(done.clone(), format!("done, not reported, not flushed; {soon}"))));
    assert!(got.contains(&(flushed, "reopened: it stays open until a task in it changes state".to_string())));
    assert!(got.contains(&(busy, format!("sent: no result recorded; {soon}"))));
    // With no answer from the daemon, it says so.
    assert!(pairs(list(&db, &HashSet::new(), None)).contains(&(done, "done, not reported, not flushed; session not checked: skiffd did not answer".to_string())));
    // Nothing changed.
    assert_eq!(serde_json::to_string(&rows(&db, "", None).unwrap()).unwrap(), before);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Runs the sweep with the session list `all` and returns the notes, dropped.
fn swept(db: &mut Connection, file: &Path, all: &[SessionInfo]) -> Vec<String> {
    sweep(db, Path::new("/tmp/here.sock"), Some(all)).unwrap();
    let notes = skiff_client::ledger::notes(file, GRAND).unwrap();
    skiff_client::ledger::drop_notes(file, &notes.iter().map(|n| n.0).collect::<Vec<_>>()).unwrap();
    notes.into_iter().map(|n| n.1).collect()
}

#[test]
fn a_closed_session_closes_its_task_whatever_its_state_and_nothing_else_does() {
    use skiff_core::session::SessionState;
    let dir = scratch("session-close");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let closed = |db: &Connection, uid: &str| one(db, uid).unwrap().closed_at.is_some();
    let on = |id: &str, st: &str| New { pane_id: Some(id.into()), state: st.into(), ..task(st) };
    let live = |id: &str, state: SessionState| {
        let mut s = pane(id, "agent", false);
        s.state = state;
        s.started_at = 1000;
        s
    };
    let mut uids = Vec::new();
    for (i, st) in ["sent", "pending", "blocked", "done", "done"].iter().enumerate() {
        uids.push(insert(&mut db, &file, on(&format!("gone0000000{i}"), st)).unwrap().1);
    }
    db.execute("UPDATE tasks SET outcome = 'failed' WHERE uid = ?1", [&uids[3]]).unwrap();
    db.execute("UPDATE tasks SET outcome = 'succeeded' WHERE uid = ?1", [&uids[4]]).unwrap();
    // A session that exited counts as closed.
    let (_, exited, _, _) = insert(&mut db, &file, on("exited000000", "blocked")).unwrap();
    // These stay: an idle session with a blocked or a finished task.
    let (_, idle_blocked, _, _) = insert(&mut db, &file, on("idle00000000", "blocked")).unwrap();
    let (_, idle_done, _, _) = insert(&mut db, &file, on("idle00000000", "done")).unwrap();
    // A pane restored under the old id, a task on another daemon.
    let (_, restored, _, _) = insert(&mut db, &file, on("restored0000", "sent")).unwrap();
    let (_, theirs, _, _) = insert(&mut db, &file, New { socket: Some("/tmp/other.sock".into()), ..on("theirs000000", "sent") }).unwrap();
    let mut back = live("restored0000", SessionState::Idle);
    back.started_at = 2000;
    let all = [live("exited000000", SessionState::Done), live("idle00000000", SessionState::Idle), back];

    // No answer from the daemon closes nothing.
    sweep(&mut db, Path::new("/tmp/here.sock"), None).unwrap();
    assert!(uids.iter().all(|u| one(&db, u).unwrap().state != "done" || !closed(&db, u)));
    assert!(!closed(&db, &exited));

    let mut notes = swept(&mut db, &file, &all);
    notes.sort();
    for u in uids.iter().chain([&exited]) {
        assert!(closed(&db, u), "{u}");
        assert_eq!(one(&db, u).unwrap().state, "done");
    }
    // One note each for what did not succeed, and none for what did.
    assert_eq!(notes.len(), 5);
    for (u, word) in [(&uids[0], "unobserved"), (&uids[1], "unobserved"), (&uids[2], "unobserved"), (&uids[3], "failed"), (&exited, "unobserved")] {
        assert!(notes.iter().any(|n| n.starts_with(&format!("Closed task {u} with no report: task {u} {word}."))), "{u}: {notes:?}");
    }
    assert!(!notes.iter().any(|n| n.contains(uids[4].as_str())));
    for u in [&idle_blocked, &idle_done, &restored, &theirs] {
        assert!(!closed(&db, u), "{u}");
    }
    assert_eq!(one(&db, &restored).unwrap().state, "sent");
    assert_eq!(one(&db, &theirs).unwrap().state, "sent");
    // Results stay on disk: the close removes no file.
    assert!(Path::new(&one(&db, &uids[0]).unwrap().work_dir.unwrap()).is_dir());
    // A second sweep says nothing again.
    assert!(swept(&mut db, &file, &all).is_empty());

    // The restored pane closes later: unobserved, with the same note.
    let notes = swept(&mut db, &file, &all[..2]);
    assert!(closed(&db, &restored));
    assert_eq!(notes, [format!("Closed task {restored} with no report: task {restored} unobserved. Tell the person. Read a row with skiff ledger show UID.")]);
    // A done task with the outcome interrupted gets the note too.
    let (_, stopped, _, _) = insert(&mut db, &file, on("stopped00000", "done")).unwrap();
    db.execute("UPDATE tasks SET outcome = 'interrupted' WHERE uid = ?1", [&stopped]).unwrap();
    let notes = swept(&mut db, &file, &all[..2]);
    assert!(closed(&db, &stopped) && notes[0].contains(&format!("task {stopped} interrupted")));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_hand_off_follows_its_tasks_and_with_none_its_skippers_session() {
    let dir = scratch("session-handoff");
    let file = dir.join("ledger.db");
    let mut db = open_at(&file).unwrap();
    let closed = |db: &Connection, uid: &str| one(db, uid).unwrap().closed_at.is_some();
    let skipper = pane("skipper00000", "agent", false);
    let handoff = |st: &str| New { handoff: true, pane_id: Some("skipper00000".into()), project: "web".into(), state: st.into(), ..task("h") };
    let under = |h: &str, id: &str| New { parent: Some(h.into()), owner: "web".into(), pane_id: Some(id.into()), ..task("t") };

    // Its Skipper runs. Its only task's session closes: the hand-off closes, with one note.
    let (_, h, _, _) = insert(&mut db, &file, handoff("sent")).unwrap();
    let (_, t, _, _) = insert(&mut db, &file, under(&h, "task00000000")).unwrap();
    let task_pane = pane("task00000000", "agent", false);
    assert!(swept(&mut db, &file, &[skipper.clone(), task_pane]).is_empty());
    assert!(!closed(&db, &h));
    let notes = swept(&mut db, &file, std::slice::from_ref(&skipper));
    assert!(closed(&db, &h));
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains(&format!("hand-off {h} unobserved")) && notes[0].contains(&format!("task {t} unobserved")));
    let r = one(&db, &h).unwrap();
    assert_eq!((r.state.as_str(), r.outcome.as_deref()), ("done", Some("unobserved")));
    // Its Skipper reports late: the stamp goes, so the watcher tells Admiral.
    restate(&db, &r, Some("done")).unwrap();
    assert!(one(&db, &h).unwrap().reported_at.is_none() && closed(&db, &h));

    // A hand-off its Skipper reported closes with no stamp and no note: the watcher tells Admiral.
    let (_, h2, _, _) = insert(&mut db, &file, handoff("done")).unwrap();
    db.execute("UPDATE tasks SET outcome = 'succeeded' WHERE uid = ?1", [&h2]).unwrap();
    insert(&mut db, &file, under(&h2, "task00000000")).unwrap();
    assert!(swept(&mut db, &file, std::slice::from_ref(&skipper)).iter().all(|n| !n.contains(&format!("hand-off {h2}"))));
    let r = one(&db, &h2).unwrap();
    assert!(r.closed_at.is_some() && r.reported_at.is_none());

    // With no tasks, it follows its Skipper's session.
    let (_, lone, _, _) = insert(&mut db, &file, handoff("sent")).unwrap();
    assert!(swept(&mut db, &file, std::slice::from_ref(&skipper)).is_empty());
    assert!(!closed(&db, &lone));
    let notes = swept(&mut db, &file, &[]);
    assert!(closed(&db, &lone));
    assert!(notes.iter().any(|n| n.contains(&format!("hand-off {lone} unobserved"))));

    // A plan closes when all its tasks close.
    let live = pane("live00000000", "agent", false);
    let (_, p, _, _) = insert(&mut db, &file, New { plan: true, pane_id: None, ..task("p") }).unwrap();
    insert(&mut db, &file, New { parent: Some(p.clone()), pane_id: Some("live00000000".into()), ..task("a") }).unwrap();
    let (_, b, _, _) = insert(&mut db, &file, New { parent: Some(p.clone()), pane_id: Some("gone00000000".into()), ..task("b") }).unwrap();
    swept(&mut db, &file, std::slice::from_ref(&live));
    assert!(one(&db, &b).unwrap().state == "done" && !closed(&db, &p));
    swept(&mut db, &file, &[]);
    assert!(closed(&db, &p));
    std::fs::remove_dir_all(&dir).unwrap();
}

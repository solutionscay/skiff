//! One `skiff ledger` command, matched and sequenced over the read and write sides.
use super::read::{
    container, join, keep, live, one, pane, print_attempts, print_one, print_rows, print_stale, project_of, rank, stale,
    state, sweep, top, tree, under, View,
};
use super::{brief, delivery, evidence};
use super::schema::{open, open_at, NOW};
use super::write::{
    add_state, age, check_scope, check_writer, close, flush_folders, folder_size, insert,
    kind_word, message, prune, read_stdin, reopen, report, restate, rows_word, scope, send_row, who, work_word,
    Caller, New, Scope, SendOpts,
};
use crate::{print, resolve, LedgerCmd};
use anyhow::Context;
use anyhow::{bail, Result};
use rusqlite::{params, TransactionBehavior};
use skiff_client::ledger::{gen, mark_live, path, roll_up, rows, GONE, GRAND, INTERRUPTED};
use std::path::{Path, PathBuf};

/// The task text: `-` reads it from stdin, as `skiff send -` does.
fn task_text(task: String) -> Result<String> {
    if task != "-" {
        return Ok(task);
    }
    Ok(read_stdin()?.trim_end().to_string())
}

pub(super) async fn run_one(socket: &Path, cmd: LedgerCmd, person: bool) -> Result<i32> {
    match cmd {
        LedgerCmd::Add { pane: p, task, title, plan: is_plan, parent, project, state: st, result, reference, task_type, expect, send, enter, shell } => {
            let task_type = task_type.as_deref().map(skiff_core::tasks::check_name).transpose()?.map(str::to_string);
            let expects = expect.as_deref().map(evidence::expects).transpose()?.map(str::to_string);
            let task = task_text(task)?;
            if is_plan && expects.is_some() {
                bail!("a plan leaves no evidence of its own: give --expect to its tasks");
            }
            let st = add_state(st.as_deref(), is_plan, send)?.to_string();
            let target = match p {
                Some(p) if !is_plan => Some(pane(socket, &p).await?),
                _ => None,
            };
            let (pane_id, pane_gen, project) = match &target {
                Some(s) => (Some(s.id.clone()), gen(s), project.unwrap_or_else(|| project_of(s))),
                None => (None, None, project.unwrap_or_default()),
            };
            let file = path()?;
            let here = pane_id.as_ref().map(|_| socket.display().to_string());
            let mut db = open_at(&file)?;
            let caller = check_writer(socket, &db, person).await?;
            if let Some(s) = &target {
                check_scope(&caller, s)?;
            }
            let owner = match parent.as_deref() {
                Some(p) => caller.check_parent(&container(&db, p)?)?,
                None => caller.owner().to_string(),
            };
            let new = New { plan: is_plan, parent, pane_id, pane_gen, project, task, title, state: st, result, reference, task_type, socket: here, owner, handoff: false, expects };
            let (id, uid, result, plan) = insert(&mut db, &file, new)?;
            let dir = one(&db, &uid)?.work_dir.unwrap_or_default();
            println!("id {id}\nuid {uid}\nfolder {dir}\nresult {result}");
            if let Some(p) = plan {
                // A task under a hand-off names the hand-off.
                println!("{} {p}", one(&db, &p).map(|r| r.kind).unwrap_or_else(|_| "plan".into()));
            }
            if send {
                let row = one(&db, &uid)?;
                return send_row(socket, &db, &row, SendOpts { text: Vec::new(), enter, shell }).await;
            }
        }
        LedgerCmd::Handoff { project, task, title, parent, expect, no_brief } => {
            let task = task_text(task)?;
            let expects = expect.as_deref().map(evidence::expects).transpose()?.map(str::to_string);
            let file = path()?;
            let mut db = open_at(&file)?;
            let caller = check_writer(socket, &db, person).await?;
            if matches!(caller, Caller::Project(_)) {
                bail!("a project or fleet Skipper does not hand off. Add your own tasks with skiff ledger add. Nothing changed");
            }
            let cfg = skiff_core::config::load()?;
            let target = scope(&cfg, &project)?;
            if let Some(p) = parent.as_deref() {
                let c = container(&db, p)?;
                if c.is_handoff() {
                    // A follow-up: it joins the hand-off's plan, or a new plan wraps both.
                    caller.check(&c)?;
                    if let Some(plan) = c.parent.as_deref() {
                        caller.check_parent(&one(&db, plan)?)?;
                    }
                } else {
                    caller.check_parent(&c)?;
                }
            }
            let c = crate::connect(socket).await?;
            let (ps, key, name) = match target {
                Scope::Project(p) => {
                    // One Skipper per project: its fleet's Skipper takes the work.
                    if let Some(f) = skiff_client::skipper::commanding_fleet(&cfg, &c.list_sessions().await?, &p) {
                        bail!("{p} is in {f}, which has a fleet Skipper. Use: skiff ledger handoff {f}. Nothing changed");
                    }
                    (crate::skipper::start_project(socket, &c, &p).await?, p.clone(), p)
                }
                Scope::Fleet(f) => {
                    let fs = crate::skipper::start_fleet(socket, &c, &f, crate::skipper::Over::Refuse).await?;
                    (fs.skipper, skiff_client::skipper::fleet_scope(&f), f)
                }
            };
            let s = &ps.session;
            let new = New {
                plan: false,
                parent,
                pane_id: Some(s.id.clone()),
                pane_gen: gen(s),
                project: key,
                task,
                title,
                state: "pending".into(),
                result: None,
                reference: None,
                task_type: None,
                socket: Some(socket.display().to_string()),
                owner: GRAND.into(),
                handoff: true,
                expects,
            };
            let (id, uid, result, plan) = insert(&mut db, &file, new)?;
            let dir = one(&db, &uid)?.work_dir.unwrap_or_default();
            println!("id {id}\nuid {uid}\nfolder {dir}\nresult {result}");
            if let Some(p) = plan {
                // A task under a hand-off names the hand-off.
                println!("{} {p}", one(&db, &p).map(|r| r.kind).unwrap_or_else(|_| "plan".into()));
            }
            let row = one(&db, &uid)?;
            if !no_brief {
                if let Err(e) = write_brief(socket, &db, &row, &s.id, ps.created).await {
                    eprintln!("skiff: no brief for {uid}: {e:#}");
                }
            }
            let mut text = message(&row);
            if ps.created {
                // A new agent takes its instructions first, then the task, in one turn.
                text = format!("{} Then take this hand-off. {text}", skiff_client::skipper::first_message(&skiff_core::config::load()?, &ps.dir)?);
                crate::skipper::ready(&c, &s.id).await?;
                println!("started the {name} Skipper");
            }
            return send_row(socket, &db, &row, SendOpts { text: vec![text], enter: true, shell: false }).await;
        }
        LedgerCmd::Brief { id, json } => {
            let db = open()?;
            let r = one(&db, &id)?;
            if !r.is_handoff() {
                eprintln!("skiff: {} {} is not a hand-off. Only a hand-off has a brief", r.kind, r.uid);
                return Ok(1);
            }
            let all = live(socket).await;
            let b = current_brief(socket, &db, &r, all.as_deref())?;
            if json {
                let (bytes, items, left_out) = b.as_ref().map(|b| (b.text.len(), b.items, b.left_out)).unwrap_or_default();
                print::json(&serde_json::json!({ "uid": r.uid, "scope": r.project, "bytes": bytes, "items": items, "left_out": left_out }))?;
            } else {
                match b {
                    Some(b) => print!("{}", b.text),
                    None => eprintln!("skiff: hand-off {} has no cited work and no open work", r.uid),
                }
            }
        }
        LedgerCmd::Send { id, text, enter, shell } => {
            let db = open()?;
            let caller = check_writer(socket, &db, person).await?;
            let row = one(&db, &id)?;
            caller.check(&row)?;
            return send_row(socket, &db, &row, SendOpts { text, enter, shell }).await;
        }
        LedgerCmd::Update { id, state: st, result, task, title, pane: p, project, parent, reference, task_type, outcome, expect, commit } => {
            let mut st = st.as_deref().map(state).transpose()?;
            let task_type = task_type.as_deref().map(skiff_core::tasks::check_name).transpose()?;
            let outcome = outcome.as_deref().map(evidence::outcome).transpose()?;
            let expects = expect.as_deref().map(evidence::expects).transpose()?;
            // An outcome ends the task.
            match (outcome, st) {
                (Some(o), Some(s)) if s != "done" => bail!("the outcome {o} is for a done task, not a {s} one. Nothing changed"),
                (Some(_), None) => st = Some("done"),
                _ => {}
            }
            let file = path()?;
            let mut db = open_at(&file)?;
            let caller = check_writer(socket, &db, person).await?;
            let row = one(&db, &id)?;
            // A project Skipper reports the hand-off it was given: its state and result.
            let report_only = task.is_none() && title.is_none() && p.is_none() && project.is_none() && parent.is_none() && reference.is_none() && task_type.is_none() && expects.is_none();
            if report_only {
                caller.check_report(&row)?;
            } else {
                caller.check(&row)?;
            }
            if let Some(p) = parent.as_deref() {
                if caller.check_parent(&container(&db, p)?)? != row.owner {
                    bail!("{} is {}'s {}, and {p} is not. Move it under a plan of {}", row.uid, who(&row.owner), kind_word(&row.kind), who(&row.owner));
                }
            }
            if row.is_plan() && (p.is_some() || parent.is_some()) {
                bail!("ledger row {id} is a plan: it has no pane and no plan above it");
            }
            if row.is_plan() && (outcome.is_some() || expects.is_some() || commit.is_some()) {
                bail!("ledger row {id} is a plan: its tasks have the outcomes and the evidence");
            }
            // A new pane, or a new send to the same one, starts from the
            // pane's program as it is now.
            let (pane_id, gen) = match (&p, st) {
                (Some(p), _) => {
                    let s = pane(socket, p).await?;
                    check_scope(&caller, &s)?;
                    (Some(s.id.clone()), Some(gen(&s)))
                }
                (None, Some("sent")) if !row.is_plan() && row.socket_other_than(socket).is_none() => {
                    let all = live(socket).await.unwrap_or_default();
                    let s = row.pane_id.as_deref().and_then(|p| resolve::session(&all, p).ok());
                    (None, s.map(gen))
                }
                _ => (None, None),
            };
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if parent.as_deref().is_some_and(|p| one(&tx, p).is_ok_and(|p| p.id == row.id)) {
                bail!("task {} cannot join its own work. Nothing changed", row.uid);
            }
            let parent = parent.as_deref().map(|p| join(&tx, &file, p)).transpose()?.map(|p| p.uid);
            // A new pane is on this socket.
            let here = pane_id.as_ref().map(|_| socket.display().to_string());
            tx.execute(
                &format!(
                    "UPDATE tasks SET state = coalesce(?2, state), result_path = coalesce(?3, result_path), task = coalesce(?4, task),
                     pane_id = coalesce(?5, pane_id), project = coalesce(?6, project), parent = coalesce(?7, parent), ref = coalesce(?8, ref),
                     pane_gen = CASE WHEN ?9 THEN ?10 ELSE pane_gen END, type = coalesce(?11, type), socket = coalesce(?12, socket), title = coalesce(?13, title),
                     closed_at = CASE WHEN ?7 IS NULL THEN closed_at END, gone_at = CASE WHEN ?2 IS NULL AND ?5 IS NULL THEN gone_at END,
                     updated_at = {NOW} WHERE id = ?1"
                ),
                params![row.id, st, result, task, pane_id, project, parent, reference, gen.is_some(), gen.flatten(), task_type, here, title],
            )?;
            tx.execute(
                "UPDATE tasks SET expects = coalesce(?2, expects), commit_ref = coalesce(?3, commit_ref) WHERE id = ?1",
                params![row.id, expects, commit],
            )?;
            let now = one(&tx, &row.uid)?;
            restate(&tx, &now, st)?;
            // Set done now: check the evidence, then record how it ended.
            let found = (st == Some("done") && !now.is_plan()).then(|| evidence::check(&now, socket)).flatten();
            if st == Some("done") && !now.is_plan() {
                let o = evidence::settle(outcome, found.as_ref());
                tx.execute("UPDATE tasks SET outcome = ?2 WHERE id = ?1", params![row.id, o])?;
            }
            tx.commit()?;
            match &found {
                Some(evidence::Evidence::Seen(t)) => println!("{} {}: {t}", kind_word(&now.kind), now.uid),
                Some(evidence::Evidence::Missing(t)) => {
                    let o = evidence::settle(outcome, found.as_ref());
                    eprintln!("skiff: {} {} is done, outcome {o}: {t}", kind_word(&now.kind), now.uid);
                }
                Some(evidence::Evidence::Unchecked(t)) => eprintln!("skiff: {} {}: {t}", kind_word(&now.kind), now.uid),
                None => {}
            }
        }
        LedgerCmd::Ls { state: st, all, closed, unreported, json } => {
            if let Some(s) = st.as_deref().filter(|s| *s != INTERRUPTED && *s != GONE) {
                state(s)?;
            }
            let view = if all {
                View::All
            } else if closed {
                View::Closed
            } else if unreported {
                View::Unreported
            } else {
                View::Open
            };
            let mut db = open()?;
            let all = live(socket).await;
            sweep(&mut db, socket, all.as_deref())?;
            let mut list = rows(&db, "", None)?;
            mark_live(socket, all.as_deref(), &mut list);
            roll_up(&mut list);
            keep(&mut list, view);
            if let Some(s) = &st {
                list.retain(|r| r.shown_state() == s);
            }
            let mut list = tree(list);
            // Stable: each work keeps its tasks under it.
            list.sort_by_key(|(r, _)| rank(r.bucket));
            if json {
                print::json(&list.iter().map(|(r, _)| r).collect::<Vec<_>>())?;
            } else {
                print_rows(&list, matches!(view, View::Closed | View::All));
            }
        }
        LedgerCmd::Stale { older_than, json } => {
            let secs = age(&older_than)?;
            let db = open()?;
            let sessions = live(socket).await;
            let cutoff: String = db.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)", [format!("-{secs} seconds")], |r| r.get(0))?;
            let held: std::collections::HashSet<String> = {
                let mut st = db.prepare("SELECT uid FROM tasks WHERE reopened_at IS NOT NULL")?;
                let out = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
                out
            };
            let mut list = rows(&db, "", None)?;
            mark_live(socket, sessions.as_deref(), &mut list);
            let found = stale(&list, &held, sessions.as_deref(), &cutoff);
            if json {
                let out: Vec<_> = found
                    .iter()
                    .map(|s| serde_json::json!({ "work": s.work, "uid": s.row.uid, "kind": s.row.kind, "state": s.row.shown_state(), "outcome": s.row.outcome, "owner": s.row.owner, "project": s.row.project, "pane": s.row.pane_id, "changed": s.changed, "why": s.why }))
                    .collect();
                print::json(&out)?;
            } else if found.is_empty() {
                println!("no stale work: all open work changed in the last {older_than}, or an agent works on it");
            } else {
                print_stale(&found);
                let works: std::collections::HashSet<&str> = found.iter().map(|s| s.work.as_str()).collect();
                println!("{} {} in {} {}. Nothing changed", found.len(), rows_word(found.len()), works.len(), work_word(works.len()));
            }
        }
        LedgerCmd::Report { ids, all } => {
            let mut db = open()?;
            let caller = check_writer(socket, &db, person).await?;
            let n = report(&mut db, &caller, &ids, all)?;
            println!("reported {n} {}", rows_word(n));
        }
        LedgerCmd::Close { ids } => {
            let mut db = open()?;
            let caller = check_writer(socket, &db, person).await?;
            let n = close(&mut db, &caller, &ids)?;
            println!("closed {n} {}", work_word(n));
        }
        LedgerCmd::Reopen { ids } => {
            let mut db = open()?;
            let caller = check_writer(socket, &db, person).await?;
            let n = reopen(&mut db, &caller, &ids)?;
            println!("reopened {n} {}", work_word(n));
        }
        LedgerCmd::Prune { older_than, yes, flush } => {
            let secs = age(&older_than)?;
            let mut db = open()?;
            let caller = check_writer(socket, &db, person).await?;
            let sessions = live(socket).await;
            // The rows first, so the rule still sees them while it judges their folders.
            let mut gone = prune(&mut db, &caller, secs, false)?;
            if gone.is_empty() {
                println!("no work was closed more than {older_than} ago. Nothing changed");
                return Ok(0);
            }
            let dirs: Vec<PathBuf> = gone.iter().filter_map(|r| r.work_dir.as_deref().map(PathBuf::from)).collect();
            if yes && flush {
                flush_folders(socket, sessions.as_deref(), &dirs)?;
            }
            let gone_now = if yes { prune(&mut db, &caller, secs, true)? } else { Vec::new() };
            let n = gone.len();
            mark_live(socket, sessions.as_deref(), &mut gone);
            print_rows(&tree(gone), true);
            if yes {
                let n = gone_now.len();
                println!("removed {n} {}. {}", rows_word(n), if flush { "Files outside the work folders stay" } else { "The work folders and result files stay" });
            } else {
                let (files, bytes) = dirs.iter().map(|d| folder_size(d)).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
                println!(
                    "would remove {n} {}. Nothing changed. Their work folders hold {files} files ({}). Run again with --yes to remove the rows, and add --flush to flush their folders",
                    rows_word(n),
                    skiff_client::agent_folder::size(bytes)
                );
            }
        }
        LedgerCmd::Show { id, json } => {
            let mut db = open()?;
            let all = live(socket).await;
            sweep(&mut db, socket, all.as_deref())?;
            let r = one(&db, &id)?;
            // The whole work: a plan's state and the bucket come from its tasks.
            let h = top(&db, &r)?;
            let mut work = under(&db, &h)?;
            work.push(h.clone());
            mark_live(socket, all.as_deref(), &mut work);
            roll_up(&mut work);
            let closed = h.closed_at.clone();
            let r = work.into_iter().find(|w| w.uid == r.uid).context("the row left the ledger")?;
            let attempts = delivery::of(&db, &r.uid)?;
            if json {
                let mut v = serde_json::to_value(&r)?;
                v["deliveries"] = serde_json::to_value(&attempts)?;
                print::json(&v)?;
            } else {
                print_one(&r, closed.as_deref());
                print_attempts(&attempts);
            }
        }
        LedgerCmd::Dir { id } => {
            let r = one(&open()?, &id)?;
            match (&r.work_dir, &r.result_path) {
                (Some(d), _) => println!("{d}"),
                (None, Some(p)) => {
                    eprintln!("skiff: row {} is from before work folders and has none. Its result: {p}", r.uid);
                    return Ok(1);
                }
                (None, None) => {
                    eprintln!("skiff: row {} is from before work folders and has none", r.uid);
                    return Ok(1);
                }
            }
        }
        LedgerCmd::Rm { id, flush } => {
            let db = open()?;
            let caller = check_writer(socket, &db, person).await?;
            let row = one(&db, &id)?;
            caller.check(&row)?;
            if row.is_plan() || row.is_handoff() {
                let n: i64 = db.query_row("SELECT count(*) FROM tasks WHERE parent = ?1", [&row.uid], |r| r.get(0))?;
                if n > 0 {
                    bail!("{} {id} still has {n} tasks. Remove them first", row.kind);
                }
            }
            if flush {
                if let Some(d) = &row.work_dir {
                    flush_folders(socket, live(socket).await.as_deref(), &[PathBuf::from(d)])?;
                }
            }
            // The row. Without --flush, its folder and result file stay where they are.
            db.execute("DELETE FROM tasks WHERE id = ?1", [row.id])?;
            match (&row.work_dir, flush) {
                (Some(d), false) if Path::new(d).exists() => println!("removed {} {}. Its work folder stays: {d}", kind_word(&row.kind), row.uid),
                _ => println!("removed {} {}", kind_word(&row.kind), row.uid),
            }
        }
    }
    Ok(0)
}

/// The brief of hand-off `h` as of now, from every row with its shown state.
fn current_brief(socket: &Path, db: &rusqlite::Connection, h: &skiff_client::ledger::Row, all: Option<&[skiff_core::session::SessionInfo]>) -> Result<Option<brief::Brief>> {
    let mut list = rows(db, "", None)?;
    mark_live(socket, all, &mut list);
    roll_up(&mut list);
    let now: String = db.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0))?;
    let h = list.iter().find(|r| r.uid == h.uid).cloned().unwrap_or_else(|| h.clone());
    Ok(brief::build(&list, &h, &brief::names(all), &now))
}

/// Writes `brief.md` for the new hand-off `h` to the Skipper session `pane`,
/// unless that session already got the same brief. A new Skipper always gets it.
async fn write_brief(socket: &Path, db: &rusqlite::Connection, h: &skiff_client::ledger::Row, pane: &str, new_skipper: bool) -> Result<()> {
    let all = live(socket).await;
    let Some(b) = current_brief(socket, db, h, all.as_deref())? else { return Ok(()) };
    let last = brief::last_sent(db, h, pane)?;
    brief::write(h, &b, last.as_deref(), new_skipper)?;
    Ok(())
}

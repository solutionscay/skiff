//! Schedule lists and details.
use std::path::Path;

use anyhow::Result;
use skiff_client::{ledger::{path, GRAND}, schedule::{self as sched, Schedule, Summary}, skipper};

use super::{agent_dir, find};
use super::super::{read::full_list, schema::open};
use crate::{clock::clock, print};

/// The runs `show` lists.
const SHOW_RUNS: i64 = 20;

/// The target in words: `type fix`, `pane brief`, `Skipper`, `web Skipper`,
/// `Harbor Skipper` (a fleet), `command`.
pub(super) fn target_words(s: &Schedule) -> String {
    match s.target.as_deref() {
        Some("type") => format!("type {}", s.task_type.as_deref().unwrap_or("?")),
        Some("pane") => format!("pane {}", s.pane.as_deref().unwrap_or("?")),
        Some("skipper") => match s.owner.as_deref() {
            None | Some(GRAND) => "Skipper".into(),
            Some(o) => format!("{} Skipper", skipper::scope_fleet(o).unwrap_or(o)),
        },
        _ if s.command.is_some() => "command".into(),
        _ => "nothing".into(),
    }
}

pub(super) async fn ls(socket: &Path, json: bool) -> Result<()> {
    let db = open()?;
    let all = full_list(socket).await;
    let list = sched::summaries(&db, &agent_dir(&path()?)?, all.as_deref(), 0)?;
    if json {
        return print::json(&list);
    }
    if list.is_empty() {
        println!("no schedules. Add one with skiff schedule add");
        return Ok(());
    }
    let w = |f: &dyn Fn(&Summary) -> String, min: usize| list.iter().map(|s| f(s).chars().count()).max().unwrap_or(0).max(min);
    let target = |s: &Summary| match &s.schedule.project {
        Some(p) => format!("{p} · {}", target_words(&s.schedule)),
        None => target_words(&s.schedule),
    };
    let next = |s: &Summary| next_text(s).unwrap_or_else(|| "paused".into());
    let (wn, ww, wt, wx) = (w(&|s| s.schedule.name.clone(), 4), w(&|s| s.when.clone(), 4), w(&target, 6), w(&next, 4));
    println!("{:wn$}  {:ww$}  {:wt$}  {:wx$}  LAST", "NAME", "WHEN", "TARGET", "NEXT");
    for s in &list {
        println!("{:wn$}  {:ww$}  {:wt$}  {:wx$}  {}", s.schedule.name, s.when, target(s), next(s), s.last.unwrap_or("-"));
    }
    Ok(())
}

/// The next run in the zone and clock of Settings › Appearance. `None` while paused.
fn next_text(s: &Summary) -> Option<String> {
    if s.schedule.paused_at.is_some() {
        return None;
    }
    s.schedule.next_at.as_deref().map(|n| clock().stamp_utc(n))
}

pub(super) async fn show(socket: &Path, name: &str, json: bool) -> Result<()> {
    let db = open()?;
    let all = full_list(socket).await;
    let s = find(&db, name)?;
    let sum = sched::summary(&db, &agent_dir(&path()?)?, s, all.as_deref(), SHOW_RUNS)?;
    if json {
        return print::json(&sum);
    }
    let s = &sum.schedule;
    println!("{}  {}  ({})", s.name, sum.when, s.cron);
    if let Some(c) = &s.command {
        println!("command  {c}  (limit {} s)", s.timeout.unwrap_or(sched::TIMEOUT));
    }
    if s.target.is_some() {
        let gate = s.gate.as_deref().map(|g| format!("{} → ", g.replace('-', " "))).unwrap_or_default();
        let project = s.project.as_deref().filter(|_| s.target.as_deref() == Some("type")).map(|p| format!(" in {p}")).unwrap_or_default();
        println!("agent    {gate}{}{project}", target_words(s));
    }
    let dir = agent_dir(&path()?)?;
    let f = sched::prompt_file(&dir, &s.uid)?;
    match sum.prompt {
        Some(n) => println!("prompt   {} ({n} bytes)", f.display()),
        None => println!("prompt   {} (none)", f.display()),
    }
    println!("next     {}", next_text(&sum).as_deref().unwrap_or("paused"));
    if s.skipped > 0 {
        println!("skipped  {} (a run was open)", s.skipped);
    }
    if sum.runs.is_empty() {
        println!("no runs yet");
        return Ok(());
    }
    println!("RUNS");
    for r in &sum.runs {
        println!("{}  {:9}  {:10}  {}", clock().stamp_utc(&r.row.sent_at), r.status, r.row.reference.as_deref().unwrap_or(""), r.row.uid);
        for k in &r.kids {
            println!("  └ {} {}  {}  {}", super::super::write::kind_word(&k.kind), k.uid, k.shown_state(), k.title.as_deref().unwrap_or(""));
        }
    }
    Ok(())
}

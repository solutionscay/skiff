//! `skiff schedule` (#262): the schedules in the ledger, and their runs.
//! skiffd runs `skiff schedule tick` each half minute. A due schedule gets a
//! run row, an ordinary task row with `tasks.schedule` set, and a detached
//! `skiff schedule fire` runs it: the command with its time limit, the gate,
//! then the agent step as a child row of the run.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use skiff_client::ledger::{auto_close, gen, path, rows, Row, FAILED, GRAND, SUCCEEDED};
use skiff_client::schedule::{
    self as sched,
    cron::{self, Cron},
    when::{self, Words},
    Schedule, Summary,
};
use skiff_core::session::SessionInfo;

use super::paths::new_uid;
use super::read::{full_list, one, project_of};
use super::schema::{open, open_at, NOW};
use super::write::{check_writer, insert, message, read_stdin, scope, send_row, New, Scope, SendOpts};
use skiff_client::skipper;
use crate::{print, resolve, ScheduleCmd, ScheduleGate, ScheduleSpec, ScheduleTarget};

/// A due time older than this when skiffd sees it was missed: the computer
/// slept, or skiffd was not running. It is not caught up.
const LATE: i64 = 120;
/// The agent gets the end of the command's output, this many bytes.
const TAIL: usize = 16 * 1024;
/// The runs `show` lists.
const SHOW_RUNS: i64 = 20;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

pub async fn run(socket: &Path, cmd: ScheduleCmd, person: bool) -> Result<i32> {
    match cmd {
        ScheduleCmd::Tick => return tick(socket).await.map(|_| 0),
        ScheduleCmd::Fire { run } => return fire(socket, &run).await.map(|_| 0),
        ScheduleCmd::Ls { json } => return ls(socket, json).await.map(|_| 0),
        ScheduleCmd::Show { name, json } => return show(socket, &name, json).await.map(|_| 0),
        _ => {}
    }
    let file = path()?;
    let mut db = open()?;
    check_writer(socket, &db, person).await?;
    match cmd {
        ScheduleCmd::Add { name, spec } => {
            let mut d = Draft { name, ..Draft::default() };
            apply(&mut d, spec, true)?;
            check(&d)?;
            let tx = db.unchecked_transaction()?;
            if sched::get(&tx, &d.name)?.is_some() {
                bail!("a schedule named {} exists. Use skiff schedule edit {}. Nothing changed", d.name, d.name);
            }
            let uid = new_uid(&tx)?;
            let next = next_of(&d.cron);
            tx.execute(
                &format!(
                    "INSERT INTO schedules (uid, name, cron, command, timeout, gate, target, project, type, pane, owner, created_at, next_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, {NOW}, ?12)"
                ),
                params![uid, d.name, d.cron, d.command, d.timeout, d.gate, d.target, d.project, d.task_type, d.pane, d.owner, next],
            )?;
            // The prompt file first: a schedule never fires without the prompt it was given.
            let prompt = sched::write_prompt(&agent_dir(&file)?, &uid, &d.name, d.text.as_deref().unwrap_or_default())?;
            tx.commit()?;
            println!("added schedule {} ({uid}): {}", d.name, when::When::from_cron(&d.cron).words());
            if let Some(n) = next.as_deref().map(sched::local_of) {
                println!("next run {n}");
            }
            println!("prompt {}", prompt.display());
        }
        ScheduleCmd::Edit { name, rename, spec } => {
            let s = find(&db, &name)?;
            let mut d = Draft::of(&s);
            let old_cron = d.cron.clone();
            if let Some(n) = rename {
                d.name = n.trim().to_string();
            }
            apply(&mut d, spec, false)?;
            check(&d)?;
            if d.name != s.name && sched::get(&db, &d.name)?.is_some() {
                bail!("a schedule named {} exists. Nothing changed", d.name);
            }
            let next = if d.cron != old_cron || s.next_at.is_none() { next_of(&d.cron) } else { s.next_at.clone() };
            db.execute(
                "UPDATE schedules SET name = ?2, cron = ?3, command = ?4, timeout = ?5, gate = ?6, target = ?7, project = ?8, type = ?9, pane = ?10, owner = ?11, next_at = ?12 WHERE id = ?1",
                params![s.id, d.name, d.cron, d.command, d.timeout, d.gate, d.target, d.project, d.task_type, d.pane, d.owner, next],
            )?;
            // --text writes the file. Without it, the file stays as the person wrote it.
            if let Some(t) = &d.text {
                let f = sched::write_prompt(&agent_dir(&file)?, &s.uid, &d.name, t)?;
                println!("prompt {}", f.display());
            }
            println!("changed schedule {}", d.name);
        }
        ScheduleCmd::Rm { name } => {
            let s = find(&db, &name)?;
            db.execute("DELETE FROM schedules WHERE id = ?1", [s.id])?;
            println!("deleted schedule {}. Its runs stay in the ledger", s.name);
            let f = sched::prompt_file(&agent_dir(&file)?, &s.uid)?;
            if f.exists() {
                println!("its prompt file stays: {}", f.display());
            }
        }
        ScheduleCmd::Pause { name } => {
            let s = find(&db, &name)?;
            db.execute(&format!("UPDATE schedules SET paused_at = coalesce(paused_at, {NOW}) WHERE id = ?1"), [s.id])?;
            println!("paused schedule {}", s.name);
        }
        ScheduleCmd::Resume { name } => {
            let s = find(&db, &name)?;
            db.execute("UPDATE schedules SET paused_at = NULL, next_at = ?2 WHERE id = ?1", params![s.id, next_of(&s.cron)])?;
            println!("resumed schedule {}", s.name);
        }
        ScheduleCmd::RunNow { name } => {
            let s = find(&db, &name)?;
            let all = full_list(socket).await;
            if open_run(&db, &s, all.as_deref())? {
                bail!("schedule {} has an open run. A schedule has one open run at most. Nothing started", s.name);
            }
            let run = start_run(&mut db, &file, &s)?;
            spawn_fire(socket, &run)?;
            println!("started run {run} of schedule {}", s.name);
        }
        ScheduleCmd::Tick | ScheduleCmd::Fire { .. } | ScheduleCmd::Ls { .. } | ScheduleCmd::Show { .. } => unreachable!(),
    }
    Ok(0)
}

fn find(db: &Connection, name: &str) -> Result<Schedule> {
    sched::get(db, name)?.with_context(|| format!("no schedule named {name}. skiff schedule ls lists them"))
}

/// The Skiff agent's folder: the ledger's, where `schedules/` holds the prompts.
fn agent_dir(file: &Path) -> Result<PathBuf> {
    file.parent().map(Path::to_path_buf).context("the ledger has no folder")
}

fn next_of(cron: &str) -> Option<String> {
    Cron::parse(cron).ok().and_then(|c| c.next_after(now())).map(cron::utc)
}

/// A schedule's fields, while `add` or `edit` sets them.
#[derive(Default)]
struct Draft {
    name: String,
    cron: String,
    command: Option<String>,
    timeout: Option<i64>,
    gate: Option<String>,
    target: Option<String>,
    project: Option<String>,
    task_type: Option<String>,
    pane: Option<String>,
    owner: Option<String>,
    /// The prompt `--text` gave, for the file. `None`: not given.
    text: Option<String>,
}

impl Draft {
    fn of(s: &Schedule) -> Draft {
        Draft {
            name: s.name.clone(),
            cron: s.cron.clone(),
            command: s.command.clone(),
            timeout: s.timeout,
            gate: s.gate.clone(),
            target: s.target.clone(),
            project: s.project.clone(),
            task_type: s.task_type.clone(),
            pane: s.pane.clone(),
            owner: s.owner.clone(),
            text: None,
        }
    }
}

/// Empty text clears a field.
fn some(v: String) -> Option<String> {
    let t = v.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn apply(d: &mut Draft, spec: ScheduleSpec, adding: bool) -> Result<()> {
    let words = Words { every: spec.every, at: spec.at, on: spec.on, day: spec.day, between: spec.between };
    match spec.cron {
        Some(_) if !words.is_empty() => bail!("--cron does not go with --every, --at, --on, --day or --between. Nothing changed"),
        Some(c) => d.cron = c.split_whitespace().collect::<Vec<_>>().join(" "),
        None if !words.is_empty() => d.cron = when::from_words(&words).map_err(anyhow::Error::msg)?.cron(),
        None if adding => bail!("give the time with --every and its words, or with --cron. Nothing changed"),
        None => {}
    }
    if let Some(c) = spec.command {
        d.command = some(c);
    }
    if let Some(t) = spec.timeout {
        if t <= 0 {
            bail!("--timeout is a number of seconds above 0. Nothing changed");
        }
        d.timeout = Some(t);
    }
    if let Some(g) = spec.gate {
        d.gate = Some(match g {
            ScheduleGate::Always => "always",
            ScheduleGate::OnFail => "on-fail",
            ScheduleGate::OnOutput => "on-output",
        }
        .to_string());
    }
    if let Some(t) = spec.to {
        d.target = match t {
            ScheduleTarget::Type => Some("type".into()),
            ScheduleTarget::Pane => Some("pane".into()),
            ScheduleTarget::Skipper => Some("skipper".into()),
            ScheduleTarget::None => None,
        };
    }
    if let Some(t) = spec.task_type {
        d.task_type = some(t);
    }
    if let Some(p) = spec.project {
        d.project = some(p);
    }
    if let Some(p) = spec.pane {
        d.pane = some(p);
    }
    if let Some(o) = spec.skipper {
        d.owner = some(o).map(|o| owner(&o)).transpose()?;
    }
    if let Some(t) = spec.text {
        // Empty text clears the prompt: the file keeps its heading line only.
        d.text = Some(if t == "-" { read_stdin()? } else { t });
    }
    // Only what the target uses stays.
    let target = d.target.clone();
    if target.as_deref() != Some("type") {
        d.task_type = None;
    }
    if target.as_deref() != Some("pane") {
        d.pane = None;
    }
    match target.as_deref() {
        Some("skipper") => d.owner = Some(d.owner.take().unwrap_or_else(|| GRAND.into())),
        _ => d.owner = None,
    }
    if d.command.is_none() || target.is_none() {
        d.gate = None;
    }
    Ok(())
}

/// The owner key of `--skipper`: `grand` for Admiral, a project's name, or
/// `fleet:<fleet>`. A bare name that is a project and a fleet is refused, as
/// `skiff ledger handoff` refuses it.
fn owner(arg: &str) -> Result<String> {
    if arg.eq_ignore_ascii_case(GRAND) || arg.eq_ignore_ascii_case(skiff_core::config::ADMIRAL) {
        return Ok(GRAND.into());
    }
    let cfg = skiff_core::config::load()?;
    Ok(match scope(&cfg, arg)? {
        Scope::Project(p) => p,
        Scope::Fleet(f) => skipper::fleet_scope(&f),
    })
}

/// A name: letters, digits, `.`, `_` and `-`, starting with a letter or a digit.
fn check_name(name: &str) -> Result<()> {
    let ok = name.len() <= 60
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        bail!("a schedule name is letters, digits, ., _ and -, as in nightly-tests, not {name:?}. Nothing changed");
    }
    Ok(())
}

fn check(d: &Draft) -> Result<()> {
    check_name(&d.name)?;
    if let Err(e) = Cron::parse(&d.cron) {
        bail!("{e} Nothing changed");
    }
    let cfg = skiff_core::config::load()?;
    let project = |p: &str| -> Result<()> {
        if cfg.projects.iter().any(|x| x.name == p) {
            Ok(())
        } else {
            bail!("no project {p:?} in {}. Nothing changed", skiff_core::config::config_path().display())
        }
    };
    if let Some(p) = &d.project {
        project(p)?;
    }
    match d.target.as_deref() {
        Some("type") => {
            let t = d.task_type.as_deref().context("--to type needs --type. Nothing changed")?;
            skiff_core::tasks::check_name(t)?;
            if d.project.is_none() {
                bail!("--to type needs --project: the new session starts in its folder. Nothing changed");
            }
        }
        Some("pane") if d.pane.is_none() => bail!("--to pane needs --pane NAME. Nothing changed"),
        Some("skipper") => match d.owner.as_deref().map(|o| (o, skipper::scope_fleet(o))) {
            Some((_, Some(f))) if !skipper::is_fleet(&cfg, f) => {
                bail!("no fleet {f:?} in {}. Nothing changed", skiff_core::config::config_path().display())
            }
            Some((o, None)) if o != GRAND => project(o)?,
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

/// The target in words: `type fix`, `pane brief`, `Skipper`, `web Skipper`,
/// `Harbor Skipper` (a fleet), `command`.
pub fn target_words(s: &Schedule) -> String {
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

async fn ls(socket: &Path, json: bool) -> Result<()> {
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
    let next = |s: &Summary| s.next.clone().unwrap_or_else(|| "paused".into());
    let (wn, ww, wt, wx) = (w(&|s| s.schedule.name.clone(), 4), w(&|s| s.when.clone(), 4), w(&target, 6), w(&next, 4));
    println!("{:wn$}  {:ww$}  {:wt$}  {:wx$}  LAST", "NAME", "WHEN", "TARGET", "NEXT");
    for s in &list {
        println!("{:wn$}  {:ww$}  {:wt$}  {:wx$}  {}", s.schedule.name, s.when, target(s), next(s), s.last.unwrap_or("-"));
    }
    Ok(())
}

async fn show(socket: &Path, name: &str, json: bool) -> Result<()> {
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
    println!("next     {}", sum.next.as_deref().unwrap_or("paused"));
    if s.skipped > 0 {
        println!("skipped  {} (a run was open)", s.skipped);
    }
    if sum.runs.is_empty() {
        println!("no runs yet");
        return Ok(());
    }
    println!("RUNS");
    for r in &sum.runs {
        println!("{}  {:9}  {:10}  {}", r.at, r.status, r.row.reference.as_deref().unwrap_or(""), r.row.uid);
        for k in &r.kids {
            println!("  └ {} {}  {}  {}", super::write::kind_word(&k.kind), k.uid, k.shown_state(), k.title.as_deref().unwrap_or(""));
        }
    }
    Ok(())
}

/// The newest run of `s` is still open: pending, working, or its agent step
/// still works.
fn open_run(db: &Connection, s: &Schedule, all: Option<&[SessionInfo]>) -> Result<bool> {
    Ok(sched::runs(db, &s.uid, 1)?.first().is_some_and(|(run, kids)| sched::is_open(sched::run_status(run, kids, all))))
}

/// The run row's text.
fn run_text(s: &Schedule) -> String {
    let mut parts = Vec::new();
    if let Some(c) = &s.command {
        parts.push(format!("run `{c}`"));
    }
    if s.target.is_some() {
        let gate = match s.gate.as_deref() {
            Some("on-fail") => "if it fails, ",
            Some("on-output") => "if it prints anything, ",
            _ => "",
        };
        parts.push(format!("{gate}send the prompt to {}", target_words(s)));
    }
    if parts.is_empty() {
        parts.push("nothing to run".into());
    }
    format!("Schedule {}: {}.", s.name, parts.join(", then "))
}

/// Adds the run row of `s`: sent, no pane, the schedule's uid. Returns its uid.
fn start_run(db: &mut Connection, file: &Path, s: &Schedule) -> Result<String> {
    let n = New {
        plan: false,
        parent: None,
        pane_id: None,
        pane_gen: None,
        project: s.project.clone().unwrap_or_default(),
        task: run_text(s),
        title: Some(s.name.clone()),
        state: "sent".into(),
        result: None,
        reference: None,
        task_type: s.task_type.clone(),
        socket: None,
        owner: GRAND.into(),
        handoff: false,
        expects: None,
    };
    let (_, uid, _, _) = insert(db, file, n)?;
    db.execute("UPDATE tasks SET schedule = ?2 WHERE uid = ?1", params![uid, s.uid])?;
    Ok(uid)
}

/// Starts `skiff schedule fire RUN` in its own session, apart from the
/// caller: a command may run for minutes.
fn spawn_fire(socket: &Path, run: &str) -> Result<()> {
    let exe = std::env::current_exe().context("find this skiff binary")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--socket")
        .arg(socket)
        .args(["schedule", "fire", run])
        .env_remove("SKIFF_SESSION")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().context("start skiff schedule fire")?;
    Ok(())
}

/// One scheduler step: each due schedule starts a run, or skips it while a
/// run is open, and each pending agent step is sent again. No ledger yet:
/// nothing to do, and no file is made.
async fn tick(socket: &Path) -> Result<()> {
    let file = path()?;
    if !file.exists() {
        return Ok(());
    }
    let mut db = open_at(&file)?;
    let list = sched::list(&db)?;
    if list.is_empty() {
        return Ok(());
    }
    let all = full_list(socket).await;
    let t = now();
    for s in list.iter().filter(|s| s.paused_at.is_none()) {
        let Ok(c) = Cron::parse(&s.cron) else { continue };
        let next = c.next_after(t).map(cron::utc);
        let Some(due) = s.next_at.as_deref().and_then(cron::parse_utc) else {
            db.execute("UPDATE schedules SET next_at = ?2 WHERE id = ?1", params![s.id, next])?;
            continue;
        };
        if due > t {
            continue;
        }
        // Only the tick that moves next_at runs it.
        if db.execute("UPDATE schedules SET next_at = ?2 WHERE id = ?1 AND next_at = ?3", params![s.id, next, s.next_at])? != 1 {
            continue;
        }
        if t - due > LATE {
            continue;
        }
        if open_run(&db, s, all.as_deref())? {
            db.execute(&format!("UPDATE schedules SET skipped = skipped + 1, last_skip_at = {NOW} WHERE id = ?1"), [s.id])?;
            continue;
        }
        let run = start_run(&mut db, &file, s)?;
        spawn_fire(socket, &run)?;
    }
    retry(socket, &mut db).await
}

/// Sends each pending agent step again. A step that went, or that no longer
/// waits (someone sent it), ends its run.
async fn retry(socket: &Path, db: &mut Connection) -> Result<()> {
    for run in rows(db, "WHERE schedule IS NOT NULL AND state = 'pending'", None)? {
        let kids = rows(db, "WHERE parent = ?1 AND state = 'pending'", Some(&run.uid))?;
        let Some(k) = kids.first() else {
            finish(db, &run, command_outcome(&run), None)?;
            continue;
        };
        match deliver(socket, db, k, None).await {
            Ok(Step::Sent) => finish(db, &run, command_outcome(&run), None)?,
            Ok(Step::Pending) => {}
            Err(e) => finish(db, &run, FAILED, Some(&short(&e)))?,
        }
    }
    Ok(())
}

/// What the agent step's send came to.
enum Step {
    Sent,
    /// The session was busy or waiting: the run stays pending.
    Pending,
}

/// Sends the agent step `row`. A send that cannot go now (the session works,
/// waits, or has text in its box) keeps it pending. Any other refusal ends
/// the step: done, failed, so it holds no work open.
async fn deliver(socket: &Path, db: &Connection, row: &Row, text: Option<String>) -> Result<Step> {
    let sent = send_row(socket, db, row, SendOpts { text: text.into_iter().collect(), enter: true, shell: false }).await;
    let refused = match sent {
        Ok(0 | 8) => return Ok(Step::Sent),
        Ok(3 | 4 | 9) => return Ok(Step::Pending),
        Ok(c) => anyhow::anyhow!("the send was refused (exit {c})"),
        Err(e) => e,
    };
    db.execute(
        &format!("UPDATE tasks SET state = 'done', outcome = ?2, reported_at = {NOW}, updated_at = {NOW} WHERE id = ?1 AND state = 'pending'"),
        params![row.id, FAILED],
    )?;
    Err(refused)
}

/// The error as one short line, for the run's `ref`.
fn short(e: &anyhow::Error) -> String {
    let s = format!("{e:#}");
    let line = s.lines().next().unwrap_or_default();
    line.chars().take(120).collect()
}

/// Succeeded unless the command failed: the run's `ref` says how it ended.
fn command_outcome(run: &Row) -> &'static str {
    match run.reference.as_deref() {
        None | Some("exit 0") => SUCCEEDED,
        Some(_) => FAILED,
    }
}

/// Ends the run: done, with its outcome, reported (the scheduler tells no
/// one), and closed once nothing under it is open.
fn finish(db: &mut Connection, run: &Row, outcome: &str, reference: Option<&str>) -> Result<()> {
    db.execute(
        &format!("UPDATE tasks SET state = 'done', outcome = ?2, ref = coalesce(?3, ref), reported_at = {NOW}, updated_at = {NOW} WHERE id = ?1"),
        params![run.id, outcome, reference],
    )?;
    auto_close(db)?;
    Ok(())
}

/// How the command ended.
struct Ran {
    /// `exit 0`, `exit 101`, `timed out after 600 s`, `did not start`.
    word: String,
    ok: bool,
    printed: bool,
    secs: u64,
    tail: Option<PathBuf>,
}

/// The project's folder, else the home folder.
fn folder(s: &Schedule) -> PathBuf {
    let cfg = skiff_core::config::load().ok();
    s.project
        .as_deref()
        .and_then(|p| cfg.as_ref()?.projects.iter().find(|x| x.name == p).map(|x| x.path.clone()))
        .filter(|p| p.is_dir())
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Runs the command in the project folder, with the login shell's
/// environment skiffd has, and its time limit. The whole output is the run's
/// result; its end goes in `tail.txt` for the agent.
async fn run_command(socket: &Path, s: &Schedule, command: &str, run: &Row) -> Result<Ran> {
    let dir = PathBuf::from(run.work_dir.as_deref().context("the run has no work folder")?);
    let log_path = dir.join("output.log");
    let log = std::fs::File::create(&log_path).with_context(|| format!("create {}", log_path.display()))?;
    let cwd = folder(s);
    let limit = s.timeout.unwrap_or(sched::TIMEOUT).max(1) as u64;
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(&cwd)
        .env("SKIFF_SOCKET", socket)
        .env_remove("SKIFF_SESSION")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .kill_on_drop(true);
    let start = Instant::now();
    let (word, ok) = match cmd.spawn() {
        Err(e) => (format!("did not start: {e}"), false),
        Ok(mut child) => match tokio::time::timeout(Duration::from_secs(limit), child.wait()).await {
            Ok(Ok(st)) => match st.code() {
                Some(c) => (format!("exit {c}"), c == 0),
                None => ("killed".to_string(), false),
            },
            Ok(Err(e)) => (format!("did not end: {e}"), false),
            Err(_) => {
                if let Some(pid) = child.id() {
                    // The whole group: a shell's children too.
                    unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
                }
                let _ = child.wait().await;
                (format!("timed out after {limit} s"), false)
            }
        },
    };
    let secs = start.elapsed().as_secs();
    let output = std::fs::read(&log_path).unwrap_or_default();
    let text = String::from_utf8_lossy(&output);
    let printed = !text.trim().is_empty();
    let result = run.result_path.as_deref().map(PathBuf::from).unwrap_or_else(|| dir.join("result.md"));
    let body = format!("# {}\n\n`{command}` in {}: {word} after {secs} s.\n\n```text\n{}\n```\n", s.name, cwd.display(), text.trim_end());
    std::fs::write(&result, body).with_context(|| format!("write {}", result.display()))?;
    let tail = printed
        .then(|| {
            let from = output.len().saturating_sub(TAIL);
            let p = dir.join("tail.txt");
            std::fs::write(&p, &output[from..]).ok().map(|_| p)
        })
        .flatten();
    Ok(Ran { word, ok, printed, secs, tail })
}

/// Runs the run `uid`: the command, the gate, the agent step.
async fn fire(socket: &Path, uid: &str) -> Result<()> {
    let file = path()?;
    let mut db = open_at(&file)?;
    let run = one(&db, uid)?;
    if run.state == "done" {
        return Ok(());
    }
    let Some(s) = run.schedule.as_deref().map(|u| sched::get(&db, u)).transpose()?.flatten() else {
        return finish(&mut db, &run, FAILED, Some("the schedule is gone"));
    };
    if s.command.is_none() && s.target.is_none() {
        return finish(&mut db, &run, FAILED, Some("no command and no agent step"));
    }
    let mut note = String::new();
    let mut gate_open = true;
    if let Some(c) = s.command.clone() {
        let ran = match run_command(socket, &s, &c, &run).await {
            Ok(r) => r,
            Err(e) => return finish(&mut db, &run, FAILED, Some(&short(&e))),
        };
        db.execute(&format!("UPDATE tasks SET ref = ?2, updated_at = {NOW} WHERE id = ?1"), params![run.id, ran.word])?;
        gate_open = gate(s.gate.as_deref(), ran.ok, ran.printed);
        note = match &ran.tail {
            Some(t) => format!(" The command `{c}` ended: {} after {} s. The end of its output is in {}.", ran.word, ran.secs, t.display()),
            None => format!(" The command `{c}` ended: {} after {} s, with no output.", ran.word, ran.secs),
        };
    }
    let run = one(&db, uid)?;
    if s.target.is_none() || !gate_open {
        return finish(&mut db, &run, command_outcome(&run), None);
    }
    match agent_step(socket, &mut db, &file, &s, &run, &note).await {
        Ok(Step::Sent) => finish(&mut db, &run, command_outcome(&run), None),
        Ok(Step::Pending) => {
            db.execute(&format!("UPDATE tasks SET state = 'pending', updated_at = {NOW} WHERE id = ?1"), [run.id])?;
            Ok(())
        }
        Err(e) => finish(&mut db, &run, FAILED, Some(&short(&e))),
    }
}

/// The gate: the agent step runs always, only when the command failed
/// (`on-fail`), or only when it printed anything (`on-output`).
fn gate(gate: Option<&str>, ok: bool, printed: bool) -> bool {
    match gate {
        Some("on-fail") => !ok,
        Some("on-output") => printed,
        _ => true,
    }
}

/// The prompt on one line: a newline would press Enter in the agent's box.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The agent step's row under `run`, for the session `pane`.
/// `handoff`: the Skipper's owner key, for a hand-off.
fn add_step(db: &mut Connection, file: &Path, socket: &Path, s: &Schedule, run: &Row, pane: &SessionInfo, task: String, handoff: Option<&str>) -> Result<Row> {
    let project = match (handoff, &s.project) {
        (Some(o), _) => o.to_string(),
        (_, Some(p)) => p.clone(),
        _ => project_of(pane),
    };
    let n = New {
        plan: false,
        parent: Some(run.uid.clone()),
        pane_id: Some(pane.id.clone()),
        pane_gen: gen(pane),
        project,
        task,
        title: Some(s.name.clone()),
        state: "pending".into(),
        result: None,
        reference: None,
        task_type: s.task_type.clone(),
        socket: Some(socket.display().to_string()),
        owner: GRAND.into(),
        handoff: handoff.is_some(),
        expects: None,
    };
    let (_, uid, _, _) = insert(db, file, n)?;
    one(db, &uid)
}

async fn agent_step(socket: &Path, db: &mut Connection, file: &Path, s: &Schedule, run: &Row, note: &str) -> Result<Step> {
    // Read now: an edit of the file takes effect at the next run.
    let text = sched::prompt(&agent_dir(file)?, &s.uid).unwrap_or_default();
    let prompt = one_line(&format!("{text}{note}"));
    if prompt.is_empty() {
        bail!("the agent step has no prompt");
    }
    let c = crate::connect(socket).await?;
    match (s.target.as_deref(), s.owner.as_deref()) {
        (Some("pane"), _) => {
            let name = s.pane.as_deref().unwrap_or_default();
            let all = c.list_sessions().await?;
            let pane = resolve::session(&all, name).map_err(|_| anyhow::anyhow!("the session {name} is gone"))?.clone();
            let step = add_step(db, file, socket, s, run, &pane, prompt, None)?;
            deliver(socket, db, &step, None).await
        }
        (Some("type"), _) => {
            let project = s.project.as_deref().context("the schedule has no project")?;
            let cfg = skiff_core::config::load()?;
            let dir = cfg.projects.iter().find(|p| p.name == project).map(|p| p.path.clone()).with_context(|| format!("the project {project} is gone"))?;
            let o = crate::sessions::New { name: Some(s.name.clone()), cwd: Some(dir), agent: None, task_type: s.task_type.clone(), model: None, role: None, command: Vec::new() };
            let spec = crate::sessions::spec(&c, o).await?;
            let info = c.create_session(spec).await?;
            let pane = crate::skipper::ready(&c, &info.id).await?;
            let step = add_step(db, file, socket, s, run, &pane, prompt, None)?;
            deliver(socket, db, &step, None).await
        }
        (Some("skipper"), None | Some(GRAND)) => {
            let found = skiff_client::skipper::open(&c, skiff_client::skipper::Open::Start).await?.context("skiffd did not start Admiral")?;
            let id = found.session.id.clone();
            if let Err(e) = super::claim(socket, &id) {
                eprintln!("skiff: {e:#}");
            }
            if let Ok(exe) = std::env::current_exe() {
                let _ = skiff_client::skipper::start_watch(socket, &exe, &id, true);
            }
            let pane = if found.created || found.typed { crate::skipper::ready(&c, &id).await? } else { found.session };
            let step = add_step(db, file, socket, s, run, &pane, prompt, None)?;
            deliver(socket, db, &step, None).await
        }
        (Some("skipper"), Some(o)) => {
            // As `skiff ledger handoff`: a project whose fleet has a running
            // fleet Skipper hands off to the fleet Skipper.
            let cfg = skiff_core::config::load()?;
            let fleet = match skipper::scope_fleet(o) {
                Some(f) => Some(f.to_string()),
                None => skipper::commanding_fleet(&cfg, &c.list_sessions().await?, o).map(str::to_string),
            };
            let (ps, key, scope) = match fleet {
                // A hand-off cannot stop agents: project Skippers in the fleet fail the run.
                Some(f) => (crate::skipper::start_fleet(socket, &c, &f, crate::skipper::Over::Refuse).await?.skipper, skipper::fleet_scope(&f), skipper::Scope::Fleet(f)),
                None => (crate::skipper::start_project(socket, &c, o).await?, o.to_string(), skipper::Scope::Project(o.to_string())),
            };
            let step = add_step(db, file, socket, s, run, &ps.session, prompt, Some(&key))?;
            let mut text = None;
            if ps.created {
                let first = skipper::first_message(&cfg, &scope)?;
                text = Some(format!("{first} Then take this hand-off. {}", message(&step)));
                crate::skipper::ready(&c, &ps.session.id).await?;
            }
            deliver(socket, db, &step, text).await
        }
        (t, _) => bail!("the target {} is not known", t.unwrap_or("none")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schedule has one open run at most: a run that is sent or pending
    /// blocks the next, and a done one does not.
    #[test]
    fn an_open_run_blocks_the_next() {
        let dir = std::env::temp_dir().join(format!("skiff-schedule-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ledger.db");
        let mut db = open_at(&file).unwrap();
        db.execute(&format!("INSERT INTO schedules (uid, name, cron, command, created_at) VALUES ('s1', 'nightly', '* * * * *', 'true', {NOW})"), []).unwrap();
        let s = sched::get(&db, "nightly").unwrap().unwrap();
        assert!(!open_run(&db, &s, Some(&[])).unwrap(), "no run yet");
        let run = start_run(&mut db, &file, &s).unwrap();
        assert!(open_run(&db, &s, Some(&[])).unwrap(), "a sent run is open");
        db.execute("UPDATE tasks SET state = 'pending' WHERE uid = ?1", [&run]).unwrap();
        assert!(open_run(&db, &s, Some(&[])).unwrap(), "a pending run is open");
        let r = one(&db, &run).unwrap();
        finish(&mut db, &r, SUCCEEDED, Some("exit 0")).unwrap();
        assert!(!open_run(&db, &s, Some(&[])).unwrap(), "a done run is not");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The gate decides whether the agent step runs after the command.
    #[test]
    fn the_gate_reads_the_command() {
        let open = gate;
        assert!(!open(Some("on-fail"), true, true));
        assert!(open(Some("on-fail"), false, false));
        assert!(!open(Some("on-output"), false, false));
        assert!(open(Some("on-output"), true, true));
        assert!(open(None, true, false));
        let mut run = crate::watch::tests::row(1, "u1", "aaaa1111", "sent", None);
        run.reference = Some("exit 101".into());
        assert_eq!(command_outcome(&run), FAILED);
        run.reference = Some("exit 0".into());
        assert_eq!(command_outcome(&run), SUCCEEDED);
    }
}

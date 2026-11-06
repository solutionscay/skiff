//! The write side: inserts, the owner and the caller's authority, sends, state changes, closing, reopening and pruning.
use super::paths::{label, make_work_folder, new_uid, result_in, results_dir, work_folder};
use super::read::{head, join, join_handoff, live, one, project_of, state, top, under};
use super::delivery;
use super::schema::{open_at, NOW};
use crate::{resolve, sessions};
use anyhow::Context;
use anyhow::{bail, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use skiff_client::ledger::{gen, heads, path, rows, Row, GRAND, UNOBSERVED};
use skiff_core::{
    session::{Role, SessionInfo},
    state,
};
use std::path::{Path, PathBuf};

/// A row for [`insert`].
pub(super) struct New {
    pub(super) plan: bool,
    /// The plan's id or uid.
    pub(super) parent: Option<String>,
    pub(super) pane_id: Option<String>,
    pub(super) pane_gen: Option<i64>,
    pub(super) project: String,
    pub(super) task: String,
    pub(super) title: Option<String>,
    pub(super) state: String,
    pub(super) result: Option<String>,
    pub(super) reference: Option<String>,
    pub(super) task_type: Option<String>,
    /// The socket of the pane.
    pub(super) socket: Option<String>,
    /// [`GRAND`], or a project Skipper's project.
    pub(super) owner: String,
    /// A hand-off: a task for a project Skipper.
    pub(super) handoff: bool,
    /// The evidence the task should leave: `result` or `commit`.
    pub(super) expects: Option<String>,
}

/// Adds a row with a new uid, and makes its work folder (see
/// [`work_folder`]). The result path is `result.md` in it (`plan.md` for a
/// plan) unless the row names one. A parent is joined as [`join`] does.
/// Returns the id, the uid, the result path and the plan's uid.
pub(super) fn insert(db: &mut Connection, file: &Path, n: New) -> Result<(i64, String, String, Option<String>)> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let parent = n.parent.as_deref().map(|p| if n.handoff { join_handoff(&tx, file, p) } else { join(&tx, file, p) }).transpose()?;
    let uid = new_uid(&tx)?;
    // A hand-off's folder goes with the project Skipper's, not in the plan's folder.
    let (results, above) = if n.handoff { (results_dir(file, &n.project), None) } else { (results_dir(file, &n.owner), parent.as_ref()) };
    let dir = work_folder(&results, &uid, label(n.title.as_deref(), &n.task), above);
    make_work_folder(&dir)?;
    let result = n.result.unwrap_or_else(|| result_in(&dir, n.plan).display().to_string());
    let kind = match (n.plan, n.handoff) {
        (true, _) => "plan",
        (_, true) => "handoff",
        _ => "task",
    };
    tx.execute(
        &format!(
            "INSERT INTO tasks (uid, kind, parent, pane_id, pane_gen, project, task, sent_at, state, result_path, ref, updated_at, type, socket, title, owner, work_dir, expects)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW}, ?8, ?9, ?10, {NOW}, ?11, ?12, ?13, ?14, ?15, ?16)"
        ),
        params![
            uid,
            kind,
            parent.as_ref().map(|p| &p.uid),
            n.pane_id,
            n.pane_gen,
            n.project,
            n.task,
            n.state,
            result,
            n.reference,
            n.task_type,
            n.socket,
            n.title,
            n.owner,
            dir.display().to_string(),
            n.expects
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok((id, uid, result, parent.map(|p| p.uid)))
}

/// The session that owns the ledger, and its socket.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Owner {
    pub(super) session: String,
    pub(super) socket: String,
}

pub(super) fn owner(db: &Connection) -> Result<Option<Owner>> {
    let mut st = db.prepare("SELECT session, socket FROM owner WHERE id = 1")?;
    let mut it = st.query_map([], |r| Ok(Owner { session: r.get(0)?, socket: r.get(1)? }))?;
    Ok(it.next().transpose()?)
}

fn set_owner(db: &Connection, session: &str, socket: &Path) -> Result<()> {
    db.execute(
        &format!("INSERT INTO owner (id, session, socket, since) VALUES (1, ?1, ?2, {NOW}) ON CONFLICT (id) DO UPDATE SET session = ?1, socket = ?2, since = {NOW}"),
        params![session, socket.display().to_string()],
    )?;
    Ok(())
}

/// The owner is `session` on `socket`.
fn is_owner(cur: Option<&Owner>, session: &str, socket: &Path) -> bool {
    cur.is_some_and(|o| o.session == session && state::same_socket(Path::new(&o.socket), socket))
}

/// The Skiff agent session of `socket` may take the ledger from `cur`: when
/// it has no owner, when the owner was on the same daemon (a new Skiff agent
/// session there replaces the old one), or when `socket` is the live one. A
/// daemon runs one Skiff agent, so the live agent always wins, and an agent
/// on another socket never takes the live agent's ledger.
fn may_claim(cur: Option<&Owner>, socket: &Path) -> bool {
    match cur {
        None => true,
        Some(o) => state::same_socket(Path::new(&o.socket), socket) || state::is_live(socket),
    }
}

/// Records `session`, the Skiff agent's session on `socket`, as the owner,
/// when [`may_claim`] allows it. `skiff skipper` and the agent's `skiff
/// watch` call it, so a new Skiff agent session takes over at its start.
pub fn claim(socket: &Path, session: &str) -> Result<()> {
    claim_at(&path()?, socket, session)
}

pub(super) fn claim_at(file: &Path, socket: &Path, session: &str) -> Result<()> {
    if !file.parent().is_some_and(Path::is_dir) {
        return Ok(());
    }
    let db = open_at(file)?;
    let tx = db.unchecked_transaction()?;
    let cur = owner(&tx)?;
    if is_owner(cur.as_ref(), session, socket) {
        return Ok(());
    }
    if !may_claim(cur.as_ref(), socket) {
        let o = cur.expect("only an owner refuses");
        let name = main_name();
        bail!("this ledger is {name}'s on {}. Do ledger work from {name} there", o.socket);
    }
    set_owner(&tx, session, socket)?;
    tx.commit()?;
    Ok(())
}

/// Who changes the ledger: a coordinator, or the person.
#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    /// The grand Skipper: the Skiff agent session that owns the ledger.
    Grand,
    /// A project or fleet Skipper, with its scope: a project's name, or
    /// `fleet:<fleet>`.
    Project(String),
    /// A shell pane, or a shell outside Skiff with `--person`.
    Person,
}

impl Caller {
    /// The owner of the rows this caller adds. The person's rows are the
    /// grand Skipper's.
    pub(super) fn owner(&self) -> &str {
        match self {
            Caller::Project(p) => p,
            Caller::Grand | Caller::Person => GRAND,
        }
    }

    /// The caller may change `r`: the person any row, a coordinator its own.
    fn owns(&self, r: &Row) -> bool {
        *self == Caller::Person || r.owner == self.owner()
    }

    /// Refuses a row the caller does not own.
    pub(super) fn check(&self, r: &Row) -> Result<()> {
        if self.owns(r) {
            return Ok(());
        }
        let ask = if r.owner == GRAND { format!("Ask {}", main_name()) } else { format!("Ask that {}", skipper_name()) };
        bail!("{} is {}'s {}. {ask} to change it", r.uid, who(&r.owner), kind_word(&r.kind))
    }

    /// A project Skipper may also report the hand-off it was given: set its
    /// state and its result, and nothing else.
    pub(super) fn check_report(&self, r: &Row) -> Result<()> {
        match self {
            Caller::Project(p) if r.is_handoff() && r.project == *p => Ok(()),
            _ => self.check(r),
        }
    }

    /// Refuses a parent the caller may not add under. A coordinator adds
    /// under its own plans. Under a hand-off, only its project's Skipper
    /// (or the person) adds; the grand Skipper cannot. Returns the owner of
    /// the new row.
    pub(super) fn check_parent(&self, p: &Row) -> Result<String> {
        if p.is_handoff() {
            return match self {
                Caller::Project(me) if *me == p.project => Ok(me.clone()),
                Caller::Person => Ok(p.project.clone()),
                _ => bail!("{} is a hand-off to {}. Ask that Skipper to add tasks under it", p.uid, who(&p.project)),
            };
        }
        self.check(p)?;
        Ok(if *self == Caller::Person { p.owner.clone() } else { self.owner().to_string() })
    }
}

/// What a change to the ledger from `me` (`$SKIFF_SESSION`), whose session
/// on `socket` is `s`, may do.
#[derive(Debug, PartialEq)]
pub(super) enum Writer {
    Yes(Caller),
    /// The Skiff agent of this socket takes the ledger first.
    Claim,
    /// A Skipper a fleet Skipper took over: it ends when idle.
    Retired,
    No,
}

/// A shell pane is a person: it may. A shell outside Skiff (no `me`) may only
/// with `person` (`--person`): an agent there has no `SKIFF_SESSION` either,
/// and nothing else tells the two apart. The owner may, as the grand
/// Skipper. A project Skipper may, for its project. Any other pane is
/// refused, a Skiff agent on another socket included, unless [`may_claim`]
/// lets the Skiff agent of `socket` take it.
pub(super) fn writer(me: Option<&str>, s: Option<&SessionInfo>, cur: Option<&Owner>, socket: &Path, person: bool) -> Writer {
    let Some(me) = me else { return if person { Writer::Yes(Caller::Person) } else { Writer::No } };
    if is_owner(cur, me, socket) {
        return Writer::Yes(Caller::Grand);
    }
    match s {
        Some(s) if s.coordinator && may_claim(cur, socket) => Writer::Claim,
        Some(s) if s.retiring => Writer::Retired,
        Some(s) if s.skipper.is_some() && !s.coordinator => Writer::Yes(Caller::Project(s.skipper.clone().unwrap_or_default())),
        Some(s) if s.role == Role::Shell && !s.coordinator => Writer::Yes(Caller::Person),
        _ => Writer::No,
    }
}

/// Who calls, from `$SKIFF_SESSION` and the live sessions. Refuses a pane
/// that may not change the ledger. It runs before the change.
pub(super) async fn check_writer(socket: &Path, db: &Connection, person: bool) -> Result<Caller> {
    let me = std::env::var("SKIFF_SESSION").ok().filter(|s| !s.is_empty());
    let cur = owner(db)?;
    let all = match &me {
        Some(m) if !is_owner(cur.as_ref(), m, socket) => live(socket).await.unwrap_or_default(),
        _ => Vec::new(),
    };
    let s = me.as_deref().and_then(|m| all.iter().find(|s| s.id == m));
    match writer(me.as_deref(), s, cur.as_ref(), socket, person) {
        Writer::Yes(c) => Ok(c),
        Writer::Claim => set_owner(db, &s.expect("a claim has a session").id, socket).map(|_| Caller::Grand),
        Writer::Retired => {
            let s = s.expect("a retired caller has a session");
            let project = project_of(s);
            let fleet = skiff_core::config::load().ok().and_then(|c| c.projects.into_iter().find(|p| p.name == project)).and_then(|p| p.fleet);
            match fleet {
                Some(f) => bail!("the {f} Skipper commands {project} now. This session ends when it is idle"),
                None => bail!("a fleet Skipper commands {project} now. This session ends when it is idle"),
            }
        }
        Writer::No if me.is_none() => bail!("this shell is outside Skiff, so Skiff cannot tell a person from an agent. If you are the person, run the command again with --person"),
        Writer::No => bail!("{}", refusal(cur.as_ref(), s.is_some_and(|s| s.coordinator))),
    }
}

/// The refusal for an agent pane. `skipper`: the caller is Skipper on
/// another socket, which may not take the ledger.
pub(super) fn refusal(cur: Option<&Owner>, skipper: bool) -> String {
    match cur {
        Some(o) if skipper => format!("this ledger is {}'s on {}. Ask {} there to make this change", main_name(), o.socket, main_name()),
        Some(_) => format!("only {} and the project and fleet {}s change the ledger. Ask {} to make this change", main_name(), skipper_name(), main_name()),
        None => format!("this ledger has no {} yet. Start {} in the Skiff app, then ask it to make this change", main_name(), main_name()),
    }
}

/// "Admiral", "the <project> project Skipper" or "the <fleet> fleet
/// Skipper": a row's owner, or a hand-off's scope.
pub(super) fn who(owner: &str) -> String {
    match (owner, skiff_client::skipper::scope_fleet(owner)) {
        (GRAND, _) => main_name(),
        (_, Some(f)) => format!("the {f} fleet {}", skipper_name()),
        (p, _) => format!("the {p} project {}", skipper_name()),
    }
}

fn main_name() -> String {
    skiff_client::skipper::name(&skiff_core::config::load().unwrap_or_default())
}

fn skipper_name() -> String {
    skiff_core::config::load().unwrap_or_default().skiff_agent.skipper_word()
}

/// A fleet Skipper sends only into the projects of its fleet, as
/// `projects.toml` has it now. Refuses a pane in another project, with
/// both names. Any other caller may send anywhere.
pub(super) fn check_scope(caller: &Caller, s: &SessionInfo) -> Result<()> {
    let Caller::Project(scope) = caller else { return Ok(()) };
    let Some(fleet) = skiff_client::skipper::scope_fleet(scope) else { return Ok(()) };
    let cfg = skiff_core::config::load()?;
    let project = project_of(s);
    if cfg.projects.iter().any(|p| p.name == project && p.fleet.as_deref() == Some(fleet)) {
        return Ok(());
    }
    bail!("pane {} is in {project}, which is not in {fleet}. Nothing changed", resolve::display_name(s))
}

/// What `handoff SCOPE` names: a project or a fleet, as an owner key.
#[derive(Debug, PartialEq)]
pub(super) enum Scope {
    Project(String),
    Fleet(String),
}

/// Reads SCOPE: `project:NAME`, `fleet:NAME`, or a bare name that is one
/// of the two. A bare name that is both is refused with both spellings.
pub(super) fn scope(cfg: &skiff_core::config::Config, arg: &str) -> Result<Scope> {
    if let Some(p) = arg.strip_prefix("project:") {
        return Ok(Scope::Project(p.to_string()));
    }
    if let Some(f) = skiff_client::skipper::scope_fleet(arg) {
        if !skiff_client::skipper::is_fleet(cfg, f) {
            bail!("no fleet {f:?} in {}", skiff_core::config::config_path().display());
        }
        return Ok(Scope::Fleet(f.to_string()));
    }
    let project = cfg.projects.iter().any(|p| p.name == arg);
    match (project, skiff_client::skipper::is_fleet(cfg, arg)) {
        (true, true) => bail!("{arg} is a project and a fleet. Use project:{arg} or fleet:{arg}. Nothing changed"),
        (false, true) => Ok(Scope::Fleet(arg.to_string())),
        _ => Ok(Scope::Project(arg.to_string())),
    }
}

/// "hand-off" for `handoff`, else the kind as it is.
pub(super) fn kind_word(kind: &str) -> &str {
    match kind {
        "handoff" => "hand-off",
        k => k,
    }
}

/// What `ledger send` and `ledger add --send` type, and how.
pub struct SendOpts {
    /// Default: the task text and its result path.
    pub text: Vec<String>,
    pub enter: bool,
    pub shell: bool,
}

/// Why `row`, whose pane is `s` now, cannot be sent: only a pending task, or
/// one a restart interrupted, has a send to make.
pub(super) fn unsendable(row: &Row, s: Option<&SessionInfo>) -> Option<String> {
    if row.is_plan() {
        return Some(format!("ledger row {} is a plan: it has no pane to send to", row.uid));
    }
    if row.state != "pending" && !row.interrupted_in(s) {
        return Some(format!("task {} is {}, not pending or interrupted. Nothing was sent", row.uid, row.state));
    }
    s.is_none().then(|| format!("the pane of task {} is gone. Nothing was sent", row.uid))
}

/// The text a task goes out with when no text is given. A hand-off says how
/// to report it.
pub(super) fn message(row: &Row) -> String {
    let task = row.task.trim();
    if row.is_handoff() {
        let path = row.result_path.as_deref().unwrap_or("your results folder");
        let folder = row.work_dir.as_deref().map(|d| format!(" Put everything for this hand-off in {d}. Your tasks for it get their folders in it.")).unwrap_or_default();
        return format!(
            "Hand-off {uid}: {task}{folder} Add your tasks for it with --parent {uid}. When they are done, write the result to {path}, then run: skiff ledger update {uid} --state done --result {path}{brief}",
            uid = row.uid,
            brief = super::brief::pointer(row).unwrap_or_default()
        );
    }
    match (row.work_dir.as_deref(), row.result_path.as_deref()) {
        (Some(d), Some(p)) => format!("{task} Put everything for this task in {d}: the result, screenshots, scratch copies and worktrees. Write your result to {p}."),
        (None, Some(p)) => format!("{task} Write your result to {p}."),
        _ => task.to_string(),
    }
}

/// The state a task takes after `send` exits with `code`. A refusal typed
/// nothing, so the task stays pending. Exit 8 typed the text but started no
/// turn: the pane needs the person.
pub(super) fn state_after(code: i32) -> Option<&'static str> {
    match code {
        0 => Some("sent"),
        8 => Some("blocked"),
        _ => None,
    }
}

/// Sends the pending task `row` to its pane with `send`'s checks, then
/// records the outcome. Returns `send`'s exit code.
pub(super) async fn send_row(socket: &Path, db: &Connection, row: &Row, o: SendOpts) -> Result<i32> {
    if let Some(other) = row.socket_other_than(socket) {
        bail!("task {} went to a pane on {other}, not on {}. Nothing was sent", row.uid, socket.display());
    }
    let c = crate::connect(socket).await?;
    let all = c.list_sessions().await?;
    let s = row.pane_id.as_deref().and_then(|p| resolve::session(&all, p).ok());
    if let Some(why) = unsendable(row, s) {
        bail!(why);
    }
    let s = s.expect("checked");
    // The payload as it goes in: read now, so the record names it.
    let text = match o.text.as_slice() {
        [] => message(row),
        [dash] if dash == "-" => read_stdin()?,
        t => t.join(" "),
    };
    let attempt = delivery::start(db, &row.uid, "send", &s.id, gen(s), &text)?;
    // The text is read already: `-` here is the text itself, not stdin.
    let sent = sessions::send_to(&c, s, sessions::Send { text: std::slice::from_ref(&text), enter: o.enter, raw: false, shell: o.shell, stdin: false }).await;
    let code = match sent {
        Ok(code) => code,
        Err(e) => {
            // It may have typed before it failed: neither sent nor not.
            delivery::end_uncertain(db, attempt, &format!("{e:#}"))?;
            return Err(e);
        }
    };
    let e = delivery::Evidence::of(code, o.enter && sessions::is_agent(s));
    delivery::end(db, attempt, e)?;
    let (bytes, sha) = delivery::identity(&text);
    println!("delivery {attempt}: {bytes} bytes, sha256 {}, {}", &sha[..12], e.word());
    match state_after(code) {
        Some(st) => {
            // Only from the state read: a send that raced this one keeps its own.
            db.execute(
                &format!("UPDATE tasks SET state = ?2, pane_gen = ?3, updated_at = {NOW} WHERE id = ?1 AND state = ?4"),
                params![row.id, st, gen(s), row.state],
            )?;
            if st != "sent" {
                eprintln!("skiff: task {} is {st}", row.uid);
            }
        }
        None => eprintln!("skiff: task {} stays {}", row.uid, row.state),
    }
    Ok(code)
}

/// All of stdin, for `-` in place of a text. Empty is an error: it would
/// store or send nothing.
pub(super) fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).context("read stdin")?;
    if buf.trim().is_empty() {
        bail!("stdin is empty: `-` reads the text from stdin. Nothing changed");
    }
    Ok(buf)
}

/// Stamps the done tasks under the done hand-off `h`, a plan's in it included,
/// whoever owns them: the Skipper that owns them cannot tell the person, and
/// the hand-off's report does. Adds each failed or unobserved one to `bad`:
/// this stamp is the only one it gets. Returns how many took a stamp.
fn report_under(tx: &Connection, h: &Row, bad: &mut Vec<(String, Row)>) -> Result<usize> {
    let tasks = rows(tx, "WHERE kind = 'task' AND state = 'done' AND reported_at IS NULL AND (parent = ?1 OR parent IN (SELECT uid FROM tasks WHERE kind = 'plan' AND parent = ?1))", Some(&h.uid))?;
    for t in &tasks {
        tx.execute(&format!("UPDATE tasks SET reported_at = {NOW} WHERE id = ?1"), [t.id])?;
    }
    let n = tasks.len();
    bad.extend(tasks.into_iter().filter(Row::ended_badly).map(|t| (h.uid.clone(), t)));
    Ok(n)
}

/// Stamps `reported_at` on the done rows `ids` name, in one transaction.
/// A plan with tasks stamps its done tasks and leaves the others; a plan with
/// none is stamped itself. A hand-off is stamped itself, with the done tasks
/// under it (see [`report_under`]). A row that is not done, or that `caller`
/// does not own, refuses the lot. A first stamp stays. `all`: every done task
/// the caller owns, and every done plan with no task. Names on stderr each
/// failed or unobserved task a hand-off's report stamped. Returns how many
/// rows took a new stamp.
pub(super) fn report(db: &mut Connection, caller: &Caller, ids: &[String], all: bool) -> Result<usize> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = format!("UPDATE tasks SET reported_at = {NOW} WHERE state = 'done' AND reported_at IS NULL");
    let mut n = 0;
    let mut bad = Vec::new();
    if all {
        let handoffs = if *caller == Caller::Person {
            rows(&tx, "WHERE kind = 'handoff' AND state = 'done'", None)?
        } else {
            rows(&tx, "WHERE kind = 'handoff' AND state = 'done' AND owner = ?1", Some(&caller.owner()))?
        };
        for h in &handoffs {
            n += report_under(&tx, h, &mut bad)?;
        }
        let mine = if *caller == Caller::Person { "1" } else { "owner = ?1" };
        let sql = format!("{stamp} AND {mine} AND (kind IN ('task', 'handoff') OR NOT EXISTS (SELECT 1 FROM tasks t WHERE t.parent = tasks.uid))");
        n += if *caller == Caller::Person { tx.execute(&sql, [])? } else { tx.execute(&sql, [caller.owner()])? };
    }
    for id in ids {
        let r = one(&tx, id)?;
        caller.check(&r)?;
        let tasks = if r.is_plan() { rows(&tx, "WHERE parent = ?1", Some(&r.uid))? } else { Vec::new() };
        if !tasks.is_empty() {
            for h in tasks.iter().filter(|t| t.is_handoff() && t.state == "done") {
                n += report_under(&tx, h, &mut bad)?;
            }
            n += tx.execute(&format!("{stamp} AND parent = ?1"), [&r.uid])?;
            continue;
        }
        if r.state != "done" {
            bail!("{} {} is {}, not done. Nothing changed", r.kind, r.uid, r.state);
        }
        if r.is_handoff() {
            n += report_under(&tx, &r, &mut bad)?;
        }
        n += tx.execute(&format!("{stamp} AND id = ?1"), [r.id])?;
    }
    tx.commit()?;
    for (h, t) in bad {
        eprintln!("skiff: hand-off {h} reported: its task {} {}. Tell the person", t.uid, t.outcome.unwrap_or_default());
    }
    Ok(n)
}

/// A row set to a state other than done opens again. That is new work: its
/// report and flush stamps go, and its work opens again. An old flush must not
/// settle the row when it is done again. A hand-off that closed unobserved and
/// is set done now is a late report: its stamp goes, so the watcher tells the
/// grand Skipper to read it.
pub(super) fn restate(db: &Connection, r: &Row, st: Option<&str>) -> Result<()> {
    if st == Some("done") && r.is_handoff() && r.outcome.as_deref() == Some(UNOBSERVED) {
        db.execute("UPDATE tasks SET reported_at = NULL WHERE id = ?1", [r.id])?;
    }
    if st.is_some_and(|s| s != "done") {
        // Only a done task has an outcome.
        db.execute("UPDATE tasks SET reported_at = NULL, flushed_at = NULL, outcome = NULL WHERE id = ?1", [r.id])?;
        db.execute("UPDATE tasks SET closed_at = NULL WHERE id = ?1", [top(db, r)?.id])?;
    }
    Ok(())
}

/// Stamps `closed_at` on the work `ids` name, in one transaction, and
/// `reported_at` on its done tasks not yet reported. Work with a task that is
/// not done, a hand-off's tasks included, refuses the lot, as does work
/// `caller` does not own. A first stamp stays. Returns how many work items
/// took a new stamp.
pub(super) fn close(db: &mut Connection, caller: &Caller, ids: &[String]) -> Result<usize> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut n = 0;
    for id in ids {
        let r = head(&tx, id)?;
        caller.check(&r)?;
        let mut tasks = under(&tx, &r)?;
        if !r.is_plan() {
            tasks.insert(0, r.clone());
        }
        if let Some(t) = tasks.iter().find(|t| t.state != "done") {
            bail!("{} {} is {}, not done. Set it done or remove it first. Nothing changed", t.kind, t.uid, t.state);
        }
        if r.closed_at.is_some() {
            continue;
        }
        for t in tasks.iter().filter(|t| t.reported_at.is_none()) {
            tx.execute(&format!("UPDATE tasks SET reported_at = {NOW} WHERE id = ?1 AND kind IN ('task', 'handoff')"), [t.id])?;
        }
        n += tx.execute(&format!("UPDATE tasks SET closed_at = {NOW}, reopened_at = NULL WHERE id = ?1"), [r.id])?;
    }
    tx.commit()?;
    Ok(n)
}

/// Clears `closed_at` on the work `ids` name and stamps `reopened_at`, in one
/// transaction: the work stays open, also when it is done and reported, until
/// a task in it changes state. Open work takes the stamp too, so it does not
/// close by itself. Returns how many work items took it.
pub(super) fn reopen(db: &mut Connection, caller: &Caller, ids: &[String]) -> Result<usize> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut n = 0;
    for id in ids {
        let r = head(&tx, id)?;
        caller.check(&r)?;
        n += tx.execute(&format!("UPDATE tasks SET closed_at = NULL, reopened_at = {NOW} WHERE id = ?1"), [r.id])?;
    }
    tx.commit()?;
    Ok(n)
}

pub(super) fn work_word(n: usize) -> &'static str {
    if n == 1 {
        "work item"
    } else {
        "work items"
    }
}

pub(super) fn rows_word(n: usize) -> &'static str {
    if n == 1 {
        "row"
    } else {
        "rows"
    }
}

/// `7d`, `12h`, `30m` or `45s`, in seconds.
pub(super) fn age(s: &str) -> Result<i64> {
    let unit = match s.chars().last() {
        Some('d') => 86_400,
        Some('h') => 3_600,
        Some('m') => 60,
        Some('s') => 1,
        _ => bail!("cannot read the age {s:?}. Use a number and d, h, m or s, such as 7d"),
    };
    let n: i64 = s[..s.len() - 1].parse().with_context(|| format!("cannot read the age {s:?}. Use a number and d, h, m or s, such as 7d"))?;
    Ok(n * unit)
}

/// The rows `prune` takes: the work `caller` owns closed at least `secs`
/// ago, each with its tasks. Open work never goes. `yes`: delete them.
pub(super) fn prune(db: &mut Connection, caller: &Caller, secs: i64, yes: bool) -> Result<Vec<Row>> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cutoff: String = tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)", [format!("-{secs} seconds")], |r| r.get(0))?;
    let all = rows(&tx, "", None)?;
    let head = heads(&all);
    let old: std::collections::HashSet<&str> = all
        .iter()
        .filter(|r| caller.owns(r) && r.closed_at.as_deref().is_some_and(|t| t <= cutoff.as_str()))
        .map(|r| r.uid.as_str())
        .collect();
    let gone: Vec<Row> = all.iter().filter(|r| old.contains(head[&r.uid].as_str())).cloned().collect();
    if yes {
        for r in &gone {
            // The row only: a result file stays where it is.
            tx.execute("DELETE FROM tasks WHERE id = ?1", [r.id])?;
        }
        tx.commit()?;
    }
    Ok(gone)
}

/// Flushes the work folders `dirs` by the Disk use rule (see
/// [`skiff_client::disk`]), before their rows go. Prints what went, and each
/// folder that stays with the reason. Without the session list nothing goes.
pub(super) fn flush_folders(socket: &Path, sessions: Option<&[SessionInfo]>, dirs: &[PathBuf]) -> Result<()> {
    let file = path()?;
    let folder = file.parent().context("the ledger has no folder")?;
    let (out, stays) = skiff_client::disk::flush_work(folder, &folder.join("cache"), &file, sessions, socket, dirs);
    if out.files > 0 || out.rows > 0 {
        println!("flushed {} {}, {}", out.files, if out.files == 1 { "file" } else { "files" }, skiff_client::agent_folder::size(out.bytes));
    }
    for (d, why) in stays {
        println!("kept {}: {why}", d.display());
    }
    for e in out.errors {
        println!("kept: {e}");
    }
    Ok(())
}

/// The files and bytes under `dir`, never through a link.
pub(super) fn folder_size(dir: &Path) -> (u64, u64) {
    let mut out = (0, 0);
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match std::fs::symlink_metadata(e.path()) {
                Ok(m) if m.is_dir() => todo.push(e.path()),
                Ok(m) => out = (out.0 + 1, out.1 + if m.file_type().is_symlink() { 0 } else { m.len() }),
                Err(_) => {}
            }
        }
    }
    out
}

/// The state `ledger add` stores. `--send` adds the task as pending and
/// sends it. Else a task waits for `ledger send`, and a plan is sent.
pub(super) fn add_state(st: Option<&str>, plan: bool, send: bool) -> Result<&str> {
    match st {
        _ if send => Ok("pending"),
        Some(s) => state(s),
        None if plan => Ok("sent"),
        None => Ok("pending"),
    }
}

//! Session commands: ls, new, send, rename, kill.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use skiff_client::Client;
use skiff_core::{
    config::TaskTypes,
    group::Group,
    project::AgentInfo,
    session::{Role, SessionInfo, SessionSpec, SessionState},
    tasks::Start,
};

use crate::{callsign::callsign, groups, layout::Side, ledger, print, resolve};

mod send;

pub use send::{agent_send, is_agent, send, send_to, send_to_outcome, waiting_msg, Miss, Outcome, Send, Unconfirmed, NO_TURN, PASTE_UNSEEN};

/// Sessions in a stable order: oldest first, then by id. Tools in the app's
/// peek are not sessions to the user, so they are left out.
pub async fn sorted(c: &Client) -> Result<Vec<SessionInfo>> {
    Ok(in_order(c.list_sessions().await?))
}

/// Oldest first, without the peek tools.
pub fn in_order(mut all: Vec<SessionInfo>) -> Vec<SessionInfo> {
    all.retain(|s| !s.peek);
    all.sort_by(|a, b| a.started_at.cmp(&b.started_at).then_with(|| a.id.cmp(&b.id)));
    all
}

pub async fn ls(c: &Client, json: bool) -> Result<()> {
    let all = sorted(c).await?;
    if json {
        return print::json(&all);
    }
    let rows: Vec<Vec<String>> = all
        .iter()
        .map(|s| {
            let mut cmd = s.command.clone();
            for a in &s.args {
                cmd.push(' ');
                cmd.push_str(a);
            }
            vec![
                print::short_id(&s.id).to_string(),
                resolve::display_name(s).to_string(),
                state_text(s),
                role(s.role).to_string(),
                print::tilde(&s.cwd),
                cmd,
            ]
        })
        .collect();
    print::table(&["ID", "NAME", "STATE", "ROLE", "CWD", "COMMAND"], &rows);
    Ok(())
}

pub fn state(s: SessionState) -> &'static str {
    match s {
        SessionState::Working => "working",
        SessionState::Waiting => "waiting",
        SessionState::Idle => "idle",
        SessionState::Done => "done",
    }
}

/// The state, with what the program reports about it (OSC 7501): what a
/// waiting program waits for, a turn's result, its progress, a long silence.
fn state_text(s: &SessionInfo) -> String {
    let word = state(s.state);
    let Some(p) = &s.program_status else { return word.to_string() };
    match p.state.as_str() {
        "blocked" => match &p.kind {
            Some(kind) => format!("{word} ({kind})"),
            None => word.to_string(),
        },
        "done" => format!("{word} (done)"),
        "error" => format!("{word} (failed)"),
        "working" if p.stalled => format!("{word} (no output)"),
        "working" => match p.progress {
            Some(n) => format!("{word} {n}%"),
            None => word.to_string(),
        },
        _ => word.to_string(),
    }
}

pub fn role(r: Role) -> &'static str {
    match r {
        Role::Agent => "agent",
        Role::Task => "task",
        Role::Server => "server",
        Role::Shell => "shell",
    }
}

pub struct New {
    pub name: Option<String>,
    pub cwd: Option<PathBuf>,
    pub agent: Option<String>,
    /// A `[skiff_agent.tasks]` row: its agent with its model.
    pub task_type: Option<String>,
    /// With `task_type`: this model in place of the row's.
    pub model: Option<String>,
    pub role: Option<Role>,
    pub command: Vec<String>,
}

/// Builds the spec the way the app's `newSession` does, so the sidebar
/// shows a CLI session like one made from the + menu.
pub async fn spec(c: &Client, o: New) -> Result<SessionSpec> {
    let here = std::env::current_dir().context("current directory")?;
    let cwd = match &o.cwd {
        Some(d) => here.join(d),
        None => here,
    };
    if !cwd.is_dir() {
        bail!("{} is not a directory", cwd.display());
    }
    // `SessionSpec::default()` has 0x0; serde's 120x40 default applies only
    // when decoding. Use the app's size for a pane it has not measured.
    let mut spec = SessionSpec {
        cwd: Some(cwd),
        cols: 120,
        rows: 40,
        ..SessionSpec::default()
    };
    if o.model.is_some() && o.agent.is_none() && o.task_type.is_none() {
        bail!("--model needs --type or --agent");
    }
    let mut flags = Vec::new();
    let words: Vec<String> = if o.task_type.is_some() || o.agent.is_some() {
        let cfg = skiff_core::config::load();
        // A task type reads its rows and its instruction file in the agent's folder.
        let (rows, dir) = match o.task_type {
            Some(_) => {
                let cfg = cfg.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))?;
                (cfg.skiff_agent.tasks.clone(), skiff_client::skipper::folder(cfg)?)
            }
            None => Default::default(),
        };
        let agents = c.list_agents().await?;
        let start = agent_line(&agents, &rows, &o, &dir)?;
        if !start.env.is_empty() {
            needs_env(c, start.agent).await?;
        }
        // The image cache's read flag goes after the model, as one argv word each.
        flags = cfg.map(|cfg| skiff_client::cache::flags(&cfg, start.agent)).unwrap_or_default();
        spec.env = start.env;
        start.words
    } else {
        o.command
    };
    let agent = o.agent.is_some() || o.task_type.is_some();
    spec.role = o.role.unwrap_or(if agent { Role::Agent } else { Role::Shell });
    if let Some((first, rest)) = words.split_first() {
        spec.command = Some(first.clone());
        spec.args = rest.to_vec();
        spec.args.extend(flags);
    }
    if spec.role == Role::Agent {
        // The app gives an agent a call sign as both label and name.
        let label = match (o.name, &o.task_type) {
            (Some(n), _) => n,
            // A task type names its session after the type.
            (None, Some(t)) => {
                let live = c.list_sessions().await?;
                numbered(t, live.iter().filter(|s| s.state != SessionState::Done).flat_map(|s| [s.label.as_str()].into_iter().chain(s.name.as_deref())))
            }
            (None, None) => {
                let live = c.list_sessions().await?;
                callsign(live.iter().map(|s| s.label.as_str()))
            }
        };
        spec.name = Some(label.clone());
        spec.label = label;
    } else {
        // Empty label: the daemon uses the program's file name.
        spec.label = o.agent.unwrap_or_default();
        spec.name = o.name;
    }
    Ok(spec)
}

/// What `--type` or `--agent` starts: the person's command from Settings ›
/// Agents, else the built-in one, which keeps the agent's permission
/// prompts. A task type adds its model and its instruction file, read from
/// the agent's folder `dir`.
fn agent_line<'a>(agents: &'a [AgentInfo], rows: &TaskTypes, o: &New, dir: &Path) -> Result<Start<'a>> {
    if let Some(t) = &o.task_type {
        let start = skiff_core::tasks::start(rows, agents, t, o.model.as_deref(), dir)?;
        if !start.agent.installed {
            bail!("agent {:?} of task type {t} is not installed: {} is not on skiffd's PATH", start.agent.id, start.agent.command);
        }
        return Ok(start);
    }
    let id = o.agent.as_deref().unwrap_or_default();
    let Some(a) = agents.iter().find(|a| a.id == id) else {
        let ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
        bail!("no agent {id:?}; known: {}", ids.join(", "));
    };
    if !a.installed {
        bail!("agent {id:?} is not installed: {} is not on skiffd's PATH", a.command);
    }
    // A model adds the agent's model flag; an agent with none refuses it.
    let words = skiff_core::tasks::command(a, o.model.as_deref())?.split_whitespace().map(String::from).collect();
    Ok(Start { agent: a, words, env: vec![] })
}

/// The first skiffd that passes a session its variables. An older one drops
/// them, and the agent would start without its instruction file.
const ENV_SINCE: (u64, u64, u64) = (0, 6, 29);

/// Refuses a start that needs variables on a skiffd older than [`ENV_SINCE`].
async fn needs_env(c: &Client, a: &AgentInfo) -> Result<()> {
    let info = c.daemon_info().await?;
    let v: Vec<u64> = info.version.split(['.', '-', '+']).take(3).map(|n| n.parse().unwrap_or(0)).collect();
    if (v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0), v.get(2).copied().unwrap_or(0)) < ENV_SINCE {
        bail!("skiffd {} cannot give {} its instruction file. {}", info.version, a.id, crate::daemon::advice(c, &info).await);
    }
    Ok(())
}

/// `name`, else `name 2`, `name 3` and on: the first one no live session uses.
pub fn numbered<'a>(name: &str, used: impl IntoIterator<Item = &'a str>) -> String {
    let used: std::collections::HashSet<&str> = used.into_iter().collect();
    (1..)
        .map(|n| if n == 1 { name.to_string() } else { format!("{name} {n}") })
        .find(|n| !used.contains(n.as_str()))
        .expect("a free name")
}

/// Starts a session. With `group`, the group is found first, so a typo
/// starts nothing.
pub async fn new(c: &Client, mut o: New, group: Option<&str>, split: Option<Side>, json: bool) -> Result<()> {
    let target = match group {
        Some(arg) => {
            let all = c.list_groups().await?;
            let g = resolve::group(&all, arg)?.clone();
            groups::room(&g, &groups::Place { split, of: None })?;
            if o.cwd.is_none() {
                o.cwd = group_cwd(c, &g).await?;
            }
            Some((all, g))
        }
        None => None,
    };
    let spec = spec(c, o).await?;
    let info = c.create_session(spec).await?;
    if let Some((all, g)) = target {
        let live = c.list_sessions().await?;
        let place = groups::Place { split, of: None };
        if let Err(e) = groups::place(c, &all, g, &info.id, place, &live).await {
            eprintln!("skiff: started {} but did not place it", info.id);
            return Err(e);
        }
    }
    if json {
        return print::json(&info);
    }
    println!("{}", info.id);
    Ok(())
}

/// Where the app starts a session in `g`: the group's folder, else the
/// folder of its first session. A folder that is gone counts as none.
async fn group_cwd(c: &Client, g: &Group) -> Result<Option<PathBuf>> {
    let dir = match (&g.cwd, crate::layout::filled(&g.layout).first()) {
        (Some(d), _) => Some(PathBuf::from(d)),
        (None, Some(first)) => c.list_sessions().await?.into_iter().find(|s| &s.id == first).map(|s| s.cwd),
        (None, None) => None,
    };
    Ok(dir.filter(|d| d.is_dir()))
}


pub async fn rename(c: &Client, arg: &str, name: &str) -> Result<()> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    c.rename_session(&s.id, name).await?;
    Ok(())
}

/// Clears the session's unread mark, as focusing its pane in the app does.
pub async fn seen(c: &Client, arg: &str) -> Result<()> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    c.seen(&s.id).await
}

/// Ends the wait of the session `arg` when a send set it (`send not
/// confirmed: …`), and types nothing. Exit codes: 0 the wait ended, or the
/// session does not wait; 1 an error, or skiffd too old for it; 3 a wait
/// with another cause, kept for the person.
pub async fn unblock(c: &Client, socket: &Path, arg: &str) -> Result<i32> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    let who = format!("{} ({})", resolve::display_name(s), print::short_id(&s.id));
    if let Some((code, msg)) = unblock_refusal(s) {
        eprintln!("skiff: {msg}");
        return Ok(code);
    }
    if let Err(e) = c.unblock(&s.id).await {
        let (code, msg) = unblock_failed(&who, &format!("{e:#}"));
        eprintln!("skiff: {msg}");
        return Ok(code);
    }
    println!("{who}: the wait ended. Nothing was typed.");
    // The task keeps its state: the text may or may not be in the box.
    if let Ok(open) = ledger::open_tasks() {
        for (_, r) in held(std::slice::from_ref(&s.id), &open, socket).into_iter().filter(|(_, r)| r.state == "blocked") {
            println!("Task {} stays blocked. Send it again with skiff ledger send {} --enter.", r.uid, r.uid);
        }
    }
    Ok(0)
}

/// `unblock`'s exit code and message when skiffd refused the request with
/// `error`. A skiffd older than the request cannot read it: "bad request".
pub(super) fn unblock_failed(who: &str, error: &str) -> (i32, String) {
    if error.starts_with("bad request") {
        return (1, "this skiffd has no unblock. Run `skiff daemon reload` after the update: it keeps every session. Nothing changed.".into());
    }
    // The wait changed after the check: another cause, kept for the person.
    let code = if error.contains("waits for the person") { 3 } else { 1 };
    (code, format!("{who}: {error}. Nothing changed."))
}

/// Why `unblock` must not end the wait of `s`, as an exit code and a
/// message. Only a wait that a send set ends; a person answers any other.
pub(super) fn unblock_refusal(s: &SessionInfo) -> Option<(i32, String)> {
    let who = format!("{} ({})", resolve::display_name(s), print::short_id(&s.id));
    if s.state != SessionState::Waiting {
        return Some((0, format!("{who} is not waiting. Nothing changed.")));
    }
    let cause = s.waiting_cause.as_deref();
    if cause.is_some_and(|c| c.starts_with(skiff_core::protocol::SEND_NOT_CONFIRMED)) {
        return None;
    }
    let msg = send::waiting_msg(&resolve::display_name(s), &s.id, cause);
    Some((3, format!("{msg} unblock ends only a wait that a send set. Nothing changed.")))
}

/// The exit code of `new` or `group new` from a Skipper with no `--name`.
pub const UNNAMED: i32 = 2;

/// The hint for an unnamed session or group from a Skipper.
pub fn name_rule(group: bool) -> &'static str {
    if group {
        "a Skipper names each group: --name after the work it holds. Nothing started."
    } else {
        "a Skipper names each session: --name \"#N feature\" for one issue (#136 kanban and calendar), else 2 to 4 words for the task, without the project name. Nothing started."
    }
}

/// The caller (`$SKIFF_SESSION`) is a Skipper: Admiral, or a project or
/// fleet Skipper. A shell pane, an agent pane or a shell outside Skiff is not.
pub async fn coordinator_calls(c: &Client) -> Result<bool> {
    let Some(me) = std::env::var("SKIFF_SESSION").ok().filter(|s| !s.is_empty()) else { return Ok(false) };
    Ok(c.list_sessions().await?.iter().find(|s| s.id == me).is_some_and(is_coordinator))
}

fn is_coordinator(s: &SessionInfo) -> bool {
    s.coordinator || s.skipper.is_some()
}

/// `kill`'s exit code when a session has an open ledger task.
const KILL_HELD: i32 = 10;

/// Resolves every argument first, so a typo kills nothing. A session with an
/// open ledger task stops the whole kill, unless `force`.
pub async fn kill(c: &Client, socket: &Path, args: &[String], force: bool) -> Result<i32> {
    let all = c.list_sessions().await?;
    let mut ids: Vec<String> = Vec::new();
    for a in args {
        let id = resolve::session(&all, a)?.id.clone();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if !force {
        let open = ledger::open_tasks()?;
        let held = held(&ids, &open, socket);
        if !held.is_empty() {
            for (id, r) in &held {
                let name = all.iter().find(|s| &s.id == *id).map(resolve::display_name).unwrap_or_default();
                let title = r.title.as_deref().unwrap_or(&r.task);
                eprintln!("skiff: {name} ({}) has open task {}: {title}", print::short_id(id), r.uid);
            }
            eprintln!("skiff: nothing killed. Close the tasks, or use --force.");
            return Ok(KILL_HELD);
        }
    }
    if !confirm_skippers(&all, &ids)? {
        eprintln!("skiff: nothing killed.");
        return Ok(1);
    }
    let mut failed = 0;
    for id in &ids {
        if let Err(e) = c.kill(id).await {
            eprintln!("skiff: kill {}: {e:#}", print::short_id(id));
            failed += 1;
        }
    }
    if failed > 0 {
        bail!("{failed} of {} sessions not killed", ids.len());
    }
    Ok(0)
}

/// With a terminal, asks y/N for each Skipper in `ids`: Admiral, a project
/// Skipper or a fleet Skipper. With no terminal, kills as before. False on a no.
fn confirm_skippers(all: &[SessionInfo], ids: &[String]) -> Result<bool> {
    use std::io::IsTerminal;
    let skippers: Vec<&SessionInfo> = ids.iter().filter_map(|id| all.iter().find(|s| &s.id == id)).filter(|s| is_coordinator(s)).collect();
    if skippers.is_empty() || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Ok(true);
    }
    // The count is extra: a ledger that cannot be read leaves it out.
    let open = ledger::open_tasks().unwrap_or_default();
    for s in skippers {
        let scope = skipper_scope(s);
        let n = open.iter().filter(|r| r.owner == scope).count();
        if !crate::skipper::ask(&skipper_question(s, n))? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The ledger owner of a Skipper: `grand` for Admiral, else its project or `fleet:<fleet>`.
fn skipper_scope(s: &SessionInfo) -> &str {
    if s.coordinator { ledger::GRAND } else { s.skipper.as_deref().unwrap_or_default() }
}

/// "Kill the skiff Skipper (1a2b3c4d), with 3 open ledger tasks?"
fn skipper_question(s: &SessionInfo, open: usize) -> String {
    let scope = skipper_scope(s);
    let what = if scope == ledger::GRAND {
        format!("{}, the Skiff agent", resolve::display_name(s))
    } else if let Some(fleet) = scope.strip_prefix("fleet:") {
        format!("the {fleet} fleet Skipper")
    } else {
        format!("the {scope} Skipper")
    };
    let tasks = match open {
        0 => String::new(),
        1 => ", with 1 open ledger task".into(),
        n => format!(", with {n} open ledger tasks"),
    };
    format!("Kill {what} ({}){tasks}?", print::short_id(&s.id))
}

/// Each of `ids` with an open task in `open` on this socket. A stored pane id
/// may be a prefix. A task on another daemon names another pane.
fn held<'a>(ids: &'a [String], open: &'a [ledger::Row], here: &Path) -> Vec<(&'a String, &'a ledger::Row)> {
    ids.iter()
        .flat_map(|id| {
            open.iter()
                .filter(move |r| r.socket_other_than(here).is_none() && r.pane_id.as_deref().is_some_and(|p| id.starts_with(p)))
                .map(move |r| (id, r))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use skiff_core::session::{SessionInfo, SessionState};
    use std::path::Path;

    use super::{agent_line, held, is_coordinator, numbered, skipper_question, unblock_failed, unblock_refusal, New};
    use super::send::{paste_left, paste_seen, refusal, Miss, Seen};
    use skiff_core::{
        config::{TaskRow, TaskTypes},
        project::AgentInfo,
    };

    fn agent(id: &str, command: &str) -> AgentInfo {
        AgentInfo { id: id.into(), command: command.into(), default_command: id.into(), custom: false, installed: true, enabled: true, used_by: None, path: None, skip_approvals: Vec::new() }
    }

    fn new(agent: Option<&str>, task_type: Option<&str>) -> New {
        New { name: None, cwd: None, agent: agent.map(Into::into), task_type: task_type.map(Into::into), model: None, role: None, command: vec![] }
    }

    #[test]
    fn kill_is_held_by_an_open_task_on_this_daemon() {
        let here = Path::new("/run/skiff.sock");
        let ids = vec!["aaaa1111ffff".to_string(), "bbbb2222ffff".to_string()];
        let other = crate::ledger::Row { socket: Some("/run/other.sock".into()), ..crate::watch::tests::row(2, "u2", "bbbb2222", "sent", None) };
        let open = [crate::watch::tests::row(1, "u1", "aaaa1111", "blocked", None), other];
        let held = held(&ids, &open, here);
        // A stored prefix matches; a task on another daemon names another pane.
        assert_eq!(held.iter().map(|(id, r)| (id.as_str(), r.uid.as_str())).collect::<Vec<_>>(), [("aaaa1111ffff", "u1")]);
    }

    #[test]
    fn only_a_skipper_must_name_what_it_starts() {
        let shell = s("shell", SessionState::Idle, None);
        let agent = s("agent", SessionState::Idle, None);
        let admiral = SessionInfo { coordinator: true, ..agent.clone() };
        let project = SessionInfo { skipper: Some("skiff".into()), ..agent.clone() };
        let fleet = SessionInfo { skipper: Some("fleet:work".into()), ..agent.clone() };
        assert!(!is_coordinator(&shell) && !is_coordinator(&agent));
        assert!(is_coordinator(&admiral) && is_coordinator(&project) && is_coordinator(&fleet));
    }

    #[test]
    fn kill_names_a_skipper() {
        let agent = SessionInfo { id: "1a2b3c4d5e6f".into(), ..s("agent", SessionState::Idle, None) };
        let project = SessionInfo { skipper: Some("skiff".into()), ..agent.clone() };
        let fleet = SessionInfo { skipper: Some("fleet:work".into()), ..agent.clone() };
        assert_eq!(skipper_question(&project, 3), "Kill the skiff Skipper (1a2b3c4d), with 3 open ledger tasks?");
        assert_eq!(skipper_question(&fleet, 0), "Kill the work fleet Skipper (1a2b3c4d)?");
    }

    #[test]
    fn new_agent_and_new_type_start_the_persons_command() {
        let rows = TaskTypes(vec![("review".into(), TaskRow { agent: "codex".into(), model: Some("gpt-5".into()) })]);
        let dir = Path::new("/no/such/skiff-agent");
        let line = |agents: &[AgentInfo], o: &New| agent_line(agents, &rows, o, dir).unwrap().words.join(" ");
        // The built-in line: no permission bypass.
        let plain = [agent("claude", "claude"), agent("codex", "codex")];
        assert_eq!(line(&plain, &new(Some("claude"), None)), "claude");
        assert_eq!(line(&plain, &new(None, Some("review"))), "codex -m gpt-5");
        // The person's override, with its flags, on both paths.
        let mine = [agent("claude", "claude --dangerously-skip-permissions"), agent("codex", "codex --yolo")];
        assert_eq!(line(&mine, &new(Some("claude"), None)), "claude --dangerously-skip-permissions");
        assert_eq!(line(&mine, &new(Some("codex"), None)), "codex --yolo");
        assert_eq!(line(&mine, &new(None, Some("review"))), "codex --yolo -m gpt-5");
    }

    #[test]
    fn new_agent_takes_a_model_or_refuses_it() {
        let rows = TaskTypes::default();
        let dir = Path::new("/no/such/skiff-agent");
        let with = |agents: &[AgentInfo], id: &str, m: &str| agent_line(agents, &rows, &New { model: Some(m.into()), ..new(Some(id), None) }, dir).map(|s| s.words.join(" "));
        let agents = [agent("claude", "claude --dangerously-skip-permissions"), agent("gemini", "agy"), agent("opencode", "opencode")];
        assert_eq!(with(&agents, "claude", "opus").unwrap(), "claude --dangerously-skip-permissions --model opus");
        assert_eq!(with(&agents, "gemini", "gemini-3.8-flash-medium").unwrap(), "agy --model gemini-3.8-flash-medium");
        // No model flag: an error, and nothing starts.
        let e = with(&agents, "opencode", "x").unwrap_err().to_string();
        assert!(e.contains("opencode takes its model in its command"), "{e}");
        // A custom agent refuses it too.
        let mut mine = agent("mine", "mine-cli");
        mine.custom = true;
        assert!(with(&[mine], "mine", "x").is_err());
    }

    #[test]
    fn new_type_gives_the_agent_its_instruction_file() {
        let dir = std::env::temp_dir().join(format!("skiff-new-type agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("types")).unwrap();
        for t in ["review", "fast", "ui"] {
            std::fs::write(dir.join(format!("types/{t}.md")), "Review only.").unwrap();
        }
        let row = |agent: &str| TaskRow { agent: agent.into(), model: None };
        let rows = TaskTypes(vec![("review".into(), row("claude")), ("fast".into(), row("gemini")), ("ui".into(), row("opencode")), ("plain".into(), row("gemini"))]);
        let agents = [agent("claude", "claude"), agent("gemini", "agy"), agent("opencode", "opencode")];
        let f = dir.join("types/review.md").display().to_string();
        // The path stays one word: the daemon gets it as one argument.
        assert_eq!(agent_line(&agents, &rows, &new(None, Some("review")), &dir).unwrap().words, ["claude", "--append-system-prompt-file", f.as_str()]);
        let ui = agent_line(&agents, &rows, &new(None, Some("ui")), &dir).unwrap();
        assert_eq!((ui.words, ui.env[0].0.as_str()), (vec!["opencode".to_string()], "OPENCODE_CONFIG_CONTENT"));
        // An agent with no way to take it: an error, and nothing starts. No file: the plain command.
        let e = agent_line(&agents, &rows, &new(None, Some("fast")), &dir).unwrap_err().to_string();
        assert!(e.contains("gemini cannot take an instruction file"), "{e}");
        assert_eq!(agent_line(&agents, &rows, &new(None, Some("plain")), &dir).unwrap().words, ["agy"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_task_type_session_takes_the_first_free_number() {
        assert_eq!(numbered("review", ["spec", "Captain Kraken"]), "review");
        assert_eq!(numbered("review", ["review", "review 3"]), "review 2");
        assert_eq!(numbered("review", ["review", "review 2"]), "review 3");
    }

    #[test]
    fn newlines_become_enter() {
        assert_eq!(super::send::enter_keys(b"a\nb\r\nc\r"), b"a\rb\rc\r");
    }

    fn s(role: &str, state: SessionState, fg: Option<&str>) -> SessionInfo {
        let mut s: SessionInfo = serde_json::from_value(serde_json::json!({
            "id": "abcdef0123", "label": "x", "role": role, "cwd": "/", "command": "claude",
            "args": [], "state": "idle", "cols": 80, "rows": 24, "pid": null, "exit_code": null,
        }))
        .unwrap();
        s.state = state;
        s.foreground_program = fg.map(String::from);
        s
    }

    #[test]
    fn the_state_says_what_the_program_reports() {
        let with = |state, status: serde_json::Value| {
            let mut x = s("agent", state, None);
            x.program_status = serde_json::from_value(status).unwrap();
            super::state_text(&x)
        };
        assert_eq!(super::state_text(&s("agent", SessionState::Idle, None)), "idle");
        assert_eq!(with(SessionState::Waiting, serde_json::json!({"state": "blocked", "kind": "permission"})), "waiting (permission)");
        assert_eq!(with(SessionState::Idle, serde_json::json!({"state": "done"})), "idle (done)");
        assert_eq!(with(SessionState::Idle, serde_json::json!({"state": "error", "ended": true})), "idle (failed)");
        assert_eq!(with(SessionState::Working, serde_json::json!({"state": "working", "progress": 40})), "working 40%");
        assert_eq!(with(SessionState::Working, serde_json::json!({"state": "working", "stalled": true})), "working (no output)");
    }

    fn code(x: &SessionInfo, here: Option<&str>, shell: bool, caller: Option<&str>) -> i32 {
        refusal(x, here, shell, caller).map_or(0, |r| r.0)
    }

    #[test]
    fn order_and_codes() {
        use SessionState::*;
        let idle = s("agent", Idle, Some("claude"));
        assert_eq!(code(&idle, Some("abcdef0123"), true, None), 7);
        assert_eq!(code(&s("agent", Waiting, Some("claude")), Some("abcdef0123"), true, None), 7);
        assert_eq!(code(&s("agent", Waiting, Some("bash")), None, true, None), 3);
        assert_eq!(code(&s("agent", Working, None), None, true, None), 4);
        assert_eq!(code(&s("shell", Done, None), None, true, None), 6);
        assert_eq!(code(&idle, Some("other"), false, None), 0);
    }

    #[test]
    fn shell_check() {
        use SessionState::Idle;
        // An agent that exited to a shell, and a nested shell, need --shell.
        assert_eq!(code(&s("agent", Idle, Some("bash")), None, false, None), 5);
        assert_eq!(code(&s("agent", Idle, Some("/usr/bin/zsh")), None, false, None), 5);
        assert_eq!(code(&s("agent", Idle, Some("bash")), None, true, None), 0);
        // Unknown program.
        assert_eq!(code(&s("shell", Idle, None), None, false, None), 5);
        assert_eq!(code(&s("shell", Idle, Some("")), None, false, None), 5);
        assert_eq!(code(&s("shell", Idle, None), None, true, None), 0);
        // The caller's SHELL counts as a shell.
        assert_eq!(code(&s("shell", Idle, Some("xonsh")), None, false, Some("/usr/bin/xonsh")), 5);
        assert_eq!(code(&s("shell", Idle, Some("xonsh")), None, false, Some("")), 0);
        // A shell-role pane with an agent in front.
        assert_eq!(code(&s("shell", Idle, Some("codex")), None, false, Some("/bin/bash")), 0);
    }

    fn screen(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    /// A capture in `tests/fixtures/paste`: the screen before the paste,
    /// the screen after it, and the text.
    macro_rules! capture {
        ($name:literal) => {
            (
                screen(&include_str!(concat!("../tests/fixtures/paste/", $name, ".before")).lines().collect::<Vec<_>>()),
                screen(&include_str!(concat!("../tests/fixtures/paste/", $name, ".after")).lines().collect::<Vec<_>>()),
                include_str!(concat!("../tests/fixtures/paste/", $name, ".txt")),
            )
        };
    }

    fn exact(seen: &Seen) -> Option<bool> {
        match seen {
            Seen::Shown { exact, .. } => Some(*exact),
            Seen::Missed(_) => None,
        }
    }

    #[test]
    fn the_paste_is_confirmed_by_new_rows_with_its_text() {
        let rule = "─".repeat(40);
        let before = screen(&["❯ Reply with ok.", "ok", &rule, "❯", &rule, "  ? for shortcuts"]);
        // The same text again: the input row is new, though the transcript has the text.
        let typed = screen(&["❯ Reply with ok.", "ok", &rule, "❯ Reply  with ok.", &rule]);
        assert_eq!(paste_seen(&before, &typed, "Reply with ok.\n", "claude"), Seen::Shown { rows: screen(&["❯ Reply  with ok."]), exact: true });
        let placeholder = screen(&["ok", &rule, "❯ [Pasted text #1 +12 lines]", &rule]);
        assert_eq!(exact(&paste_seen(&before, &placeholder, "line one\nline two", "claude")), Some(false));
        // agy's colored hint, and a screen with no change: not confirmed.
        let hint = screen(&["╭──────────────╮", "│ > Auto mode: edits auto-approved │", "╰──────────────╯"]);
        assert_eq!(paste_seen(&hint, &hint, "Reply with ok.", "gemini"), Seen::Missed(Miss::NoChange));
        assert_eq!(paste_seen(&before, &before, "Reply with ok.", "claude"), Seen::Missed(Miss::NoChange));
    }

    /// #300: Claude Code wrapped a short lead-in and a long path, so no one
    /// row held the start of the text. The 658-byte task text and the box of
    /// the pane it stuck in, at 120 columns.
    #[test]
    fn a_paste_wrapped_after_a_short_lead_in_is_confirmed_exactly() {
        let (before, after, text) = capture!("claude-A");
        assert_eq!(text.len(), 658);
        let Seen::Shown { rows, exact } = paste_seen(&before, &after, text, "claude") else { panic!("not confirmed") };
        assert!(exact);
        assert_eq!(rows.len(), 8);
        // Claude Code draws a no-break space after its prompt mark.
        assert_eq!(rows[0], "❯\u{a0}Read the rules in");
    }

    /// Each Claude Code capture of the repro (#300) is confirmed, and the box
    /// holds exactly the text: word wraps inside the start (B, L), a narrow
    /// pane (J), a long word cut in place (B0, E, I), one short row (F), a
    /// newline (G) and a placeholder whose line count fits (H).
    #[test]
    fn each_captured_claude_paste_is_confirmed_exactly() {
        let all = [
            ("B", capture!("claude-B")),
            ("B0", capture!("claude-B0")),
            ("C", capture!("claude-C")),
            ("D", capture!("claude-D")),
            ("E", capture!("claude-E")),
            ("F", capture!("claude-F")),
            ("G", capture!("claude-G")),
            ("H", capture!("claude-H")),
            ("I", capture!("claude-I")),
            ("J", capture!("claude-J")),
            ("K", capture!("claude-K")),
            ("L", capture!("claude-L")),
            ("M", capture!("claude-M")),
        ];
        for (name, (before, after, text)) in all {
            assert_eq!(exact(&paste_seen(&before, &after, text, "claude")), Some(true), "case {name}");
        }
    }

    #[test]
    fn a_paste_is_not_confirmed_by_text_outside_the_box_and_not_exact_beside_other_text() {
        let (empty, after, text) = capture!("claude-B");
        let (_, placeholder, _) = capture!("claude-H");
        let rule = "─".repeat(120);
        let at_box = |rows: &[&str]| {
            let mut s = screen(&["", &rule]);
            s.extend(screen(rows));
            s.extend(screen(&[&rule, "  ⏵⏵ bypass permissions on (shift+tab to cycle)"]));
            s
        };
        let path = "/Users/jose/skiff-agent/projects/fm-object-clip/results/a6ce6db0-build-the-wiki/efb826cc-build-wiki-and-docs-branch";
        // A placeholder whose line count does not fit: confirmed, not exact.
        assert_eq!(exact(&paste_seen(&empty, &placeholder, "Line one of two.\nLine two of two.", "claude")), Some(false));
        // A placeholder or the text in the transcript, the box empty.
        let mut transcript = screen(&["❯ [Pasted text #1 +29 lines]", &format!("❯ {}", text.trim())]);
        transcript.extend(empty.clone());
        assert_eq!(paste_seen(&empty, &transcript, text, "claude"), Seen::Missed(Miss::NoChange));
        // A person's draft, then the text: confirmed by its start, not exact.
        let draft = at_box(&["❯ my draft"]);
        let both = at_box(&["❯ my draft Say ok then stop", &format!("  {path}"), "  and say no more."]);
        assert_eq!(exact(&paste_seen(&draft, &both, text, "claude")), Some(false));
        // The text twice, also when the box held it once already.
        let twice = at_box(&["❯ Say ok then stop", &format!("  {path}"), "  and say no more.Say ok then stop", &format!("  {path}"), "  and say no more."]);
        assert_eq!(exact(&paste_seen(&empty, &twice, text, "claude")), Some(false));
        assert_eq!(exact(&paste_seen(&after, &twice, text, "claude")), Some(false));
        // A box that already held the text, unchanged: not new.
        assert_eq!(paste_seen(&after, &after, text, "claude"), Seen::Missed(Miss::NoChange));
        // A dim suggestion in the box, before and after.
        let hint = at_box(&["❯ Try \"refactor send.rs\""]);
        assert_eq!(paste_seen(&hint, &hint, text, "claude"), Seen::Missed(Miss::NoChange));
        // Other text in the box: it changed, without the text.
        assert_eq!(paste_seen(&empty, &at_box(&["❯ Say ok"]), text, "claude"), Seen::Missed(Miss::BoxChanged));
    }

    /// One wrapped capture each from Codex and OpenCode (a placeholder, with
    /// no box Skiff knows), and agy's bordered box.
    #[test]
    fn other_agents_show_a_wrapped_paste_in_their_own_box() {
        let (before, after, text) = capture!("codex");
        assert_eq!(exact(&paste_seen(&before, &after, text, "codex")), Some(true));
        let (before, after, text) = capture!("opencode");
        assert_eq!(exact(&paste_seen(&before, &after, text, "opencode")), Some(true));
        let edge = |l: char, r: char| format!("{l}{}{r}", "─".repeat(78));
        let row = |t: &str| format!("│ {t:<77}│");
        let agy = |rows: &[&str]| {
            let mut s = screen(&["", &edge('╭', '╮')]);
            s.extend(rows.iter().map(|t| row(t)));
            s.push(edge('╰', '╯'));
            s
        };
        let before = agy(&["> "]);
        let after = agy(&["> Do the task in", "  /Users/jose/skiff-agent/projects/fm-object-clip/results/a6ce6db0-build-the-", "  wiki/brief-build.md."]);
        let text = "Do the task in /Users/jose/skiff-agent/projects/fm-object-clip/results/a6ce6db0-build-the-wiki/brief-build.md.";
        assert_eq!(exact(&paste_seen(&before, &after, text, "gemini")), Some(true));
    }

    #[test]
    fn enter_is_confirmed_when_the_pasted_row_leaves() {
        let rule = "─".repeat(40);
        let after = screen(&["ok", &rule, "❯ Reply with ok.", &rule]);
        let pasted = screen(&["❯ Reply with ok."]);
        assert!(paste_left(&after, &screen(&["ok", "", &rule, "❯", &rule]), &pasted));
        assert!(!paste_left(&after, &after, &pasted));
    }

    #[test]
    fn unblock_ends_only_a_wait_that_a_send_set() {
        use SessionState::{Idle, Waiting};
        let waiting = |cause: Option<&str>| SessionInfo { waiting_cause: cause.map(String::from), ..s("agent", Waiting, Some("claude")) };
        for cause in ["send not confirmed: paste (the input box changed)", "send not confirmed: paste (nothing showed)", "send not confirmed: enter", "send not confirmed: paste"] {
            assert_eq!(unblock_refusal(&waiting(Some(cause))), None, "{cause}");
        }
        for cause in [Some("prompt"), Some("bell"), Some("title"), Some("report"), None] {
            let (code, msg) = unblock_refusal(&waiting(cause)).expect("refused");
            assert_eq!(code, 3, "{cause:?}");
            assert!(msg.contains("Nothing changed"), "{msg}");
        }
        assert_eq!(unblock_refusal(&s("agent", Idle, Some("claude"))).map(|r| r.0), Some(0));
        // The exit 3 of send and wait names unblock only for a send's wait.
        assert!(super::waiting_msg("x", "abcdef0123", Some("send not confirmed: paste (nothing showed)")).contains("skiff unblock abcdef01"));
        assert!(!super::waiting_msg("x", "abcdef0123", Some("prompt")).contains("unblock"));
    }

    /// A skiffd older than `unblock` answers it as it answers any request it
    /// cannot read: "bad request: unknown variant …". Its protocol is the same.
    #[test]
    fn an_older_skiffd_refuses_unblock_and_the_cli_says_to_reload() {
        let line = serde_json::to_string(&skiff_core::protocol::Request::Unblock { session: "abc".into() }).unwrap();
        assert_eq!(line, r#"{"cmd":"unblock","session":"abc"}"#);
        let unknown = serde_json::from_str::<skiff_core::protocol::Request>(&line.replace("unblock", "no_such_request")).unwrap_err();
        let (code, msg) = unblock_failed("x (abc)", &format!("bad request: {unknown}"));
        assert_eq!(code, 1);
        assert!(msg.contains("skiff daemon reload"), "{msg}");
        assert_eq!(unblock_failed("x (abc)", "session abc waits for the person: prompt. A person answers it in the pane").0, 3);
        assert_eq!(unblock_failed("x (abc)", "no such session: abc").0, 1);
    }

    #[test]
    fn known_agents_only() {
        use SessionState::Idle;
        for fg in ["claude", "codex", "gemini", "grok", "opencode", "hermes"] {
            assert!(super::is_agent(&s("shell", Idle, Some(fg))), "{fg}");
        }
        for fg in [None, Some("bash"), Some("sh"), Some("vim"), Some("node")] {
            assert!(!super::is_agent(&s("agent", Idle, fg)), "{fg:?}");
        }
    }
}

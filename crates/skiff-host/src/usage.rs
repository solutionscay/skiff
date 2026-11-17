//! Agent plan usage (`agent_usage@1`). Each reader runs the agent's own usage
//! command on this Host and turns its answer into windows of percent used:
//! Codex, Antigravity, Grok and Claude Code. No reader opens an agent's
//! credential or data file, and none calls a vendor endpoint itself.
//!
//! When to read is [`schedule`]'s: page open and Read now from a request, a
//! turn end and the running panes from the paired skiffd's events, and a poll
//! tick here. The last values of each agent stay in this process.
mod agy;
mod claude;
mod codex;
mod grok;
mod run;
pub mod schedule;
pub mod window;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use skiff_core::{
    project::AgentInfo,
    protocol::Event,
    session::{SessionInfo, SessionState},
};
use skiff_host_api::{AgentUsage, UsageCredits, UsageError, UsageRefresh, UsageWindow};
use tokio::sync::broadcast::error::RecvError;

use run::Program;
use schedule::{Clock, Trigger};

use crate::SESSIONS_TIMEOUT;

/// The agents with a reader, by Settings › Agents id.
pub const READERS: [&str; 4] = ["claude", "codex", "gemini", "grok"];
/// How often the poll looks for a running agent whose read is due.
const TICK: Duration = Duration::from_secs(30);
/// How long the watcher waits before it tries skiffd again.
const RETRY: Duration = Duration::from_secs(5);

/// What one read returns: percent used, from any source.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    pub credits: Option<UsageCredits>,
}

/// One agent's last values and read times.
#[derive(Default)]
struct Agent {
    clock: Clock,
    plan: Option<String>,
    read_at: Option<u64>,
    windows: Vec<UsageWindow>,
    credits: Option<UsageCredits>,
    error: Option<UsageError>,
}

#[derive(Default)]
struct State {
    agents: BTreeMap<String, Agent>,
    /// The installed agents with a reader, as skiffd last listed them, in
    /// Settings › Agents order.
    programs: Vec<(String, Program)>,
    /// The agent in front of each live session.
    fronts: HashMap<String, String>,
}

impl State {
    fn running(&self, agent: &str) -> bool {
        self.fronts.values().any(|a| a == agent)
    }
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The installed agents with a reader, from skiffd's list: its PATH finds them.
fn programs_of(list: Vec<AgentInfo>) -> Vec<(String, Program)> {
    list.into_iter()
        .filter(|a| a.installed && READERS.contains(&a.id.as_str()))
        .filter_map(|a| {
            let path = a.path.map(std::path::PathBuf::from).or_else(|| run::first_word(&a.command))?;
            Some((a.id, Program { path }))
        })
        .collect()
}

/// A `usage_read`: starts the reads `refresh` asks for and answers the last
/// values at once.
pub async fn answer(refresh: UsageRefresh) -> Vec<AgentUsage> {
    // The agent list again, so an agent installed since shows.
    if let Ok(c) = crate::skipper::connect().await {
        if let Ok(Ok(list)) = tokio::time::timeout(SESSIONS_TIMEOUT, c.list_agents()).await {
            state().lock().unwrap().programs = programs_of(list);
        }
    }
    let trigger = match refresh {
        UsageRefresh::None => None,
        UsageRefresh::Stale => Some(Trigger::PageOpen),
        UsageRefresh::All => Some(Trigger::ReadNow),
    };
    if let Some(trigger) = trigger {
        let ids: Vec<String> = state().lock().unwrap().programs.iter().map(|(id, _)| id.clone()).collect();
        for id in ids {
            start(&id, trigger);
        }
    }
    snapshot()
}

/// Each agent with a reader, as its last read left it.
fn snapshot() -> Vec<AgentUsage> {
    let s = state().lock().unwrap();
    s.programs
        .iter()
        .map(|(id, _)| {
            let a = s.agents.get(id);
            AgentUsage {
                agent: id.clone(),
                plan: a.and_then(|a| a.plan.clone()),
                read_at: a.and_then(|a| a.read_at),
                windows: a.map(|a| a.windows.clone()).unwrap_or_default(),
                credits: a.and_then(|a| a.credits.clone()),
                error: a.and_then(|a| a.error.clone()),
                reading: a.is_some_and(|a| a.clock.busy()),
            }
        })
        .collect()
}

/// Reads `id` when the schedule allows `trigger` now.
fn start(id: &str, trigger: Trigger) {
    let now = now_ms();
    let program = {
        let mut s = state().lock().unwrap();
        let running = s.running(id);
        let Some(program) = s.programs.iter().find(|(p, _)| p == id).map(|(_, p)| p.clone()) else { return };
        let agent = s.agents.entry(id.to_string()).or_default();
        if !agent.clock.wants(trigger, now, running) {
            return;
        }
        agent.clock.start(now);
        program
    };
    let id = id.to_string();
    tokio::spawn(async move {
        let result = read(&id, &program).await;
        let mut s = state().lock().unwrap();
        let agent = s.agents.entry(id).or_default();
        agent.clock.finish(result.is_ok());
        match result {
            Ok(read) => {
                agent.plan = read.plan;
                agent.windows = read.windows;
                agent.credits = read.credits;
                agent.read_at = Some(now_ms());
                agent.error = None;
            }
            // The last values stay, with their own read time.
            Err(message) => agent.error = Some(UsageError { at: now_ms(), message }),
        }
    });
}

async fn read(id: &str, program: &Program) -> Result<Read, String> {
    match id {
        "claude" => claude::read(program).await,
        "codex" => codex::read(program).await,
        "gemini" => agy::read(program).await,
        "grok" => grok::read(program).await,
        other => Err(format!("{other} has no usage read")),
    }
}

/// The agent in front of a live session, when it has a reader.
fn front(s: &SessionInfo) -> Option<String> {
    if s.state == SessionState::Done {
        return None;
    }
    s.foreground_program.as_deref().filter(|p| READERS.contains(p)).map(str::to_owned)
}

/// Follows the paired skiffd for as long as this service runs: which agent
/// is in front of each pane, and each turn end. It polls the running agents
/// on its tick. Without skiffd nothing runs, so only the page reads.
pub fn watch() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(async {
        loop {
            // A skiffd that is not there yet is no news: try again.
            let _ = follow().await;
            state().lock().unwrap().fronts.clear();
            tokio::time::sleep(RETRY).await;
        }
    });
}

async fn follow() -> anyhow::Result<()> {
    let c = skiff_client::Client::connect(&skiff_core::state::socket()).await?;
    // Listen before the list, so no change falls between the two.
    let mut events = c.events();
    let sessions = tokio::time::timeout(SESSIONS_TIMEOUT, c.list_sessions()).await??;
    let agents = tokio::time::timeout(SESSIONS_TIMEOUT, c.list_agents()).await??;
    {
        let mut s = state().lock().unwrap();
        s.programs = programs_of(agents);
        s.fronts = sessions.iter().filter_map(|x| front(x).map(|a| (x.id.clone(), a))).collect();
    }
    let mut tick = tokio::time::interval(TICK);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(Event::SessionCreated { session } | Event::SessionUpdated { session }) => {
                    let mut s = state().lock().unwrap();
                    match front(&session) {
                        Some(a) => s.fronts.insert(session.id.clone(), a),
                        None => s.fronts.remove(&session.id),
                    };
                }
                Ok(Event::SessionRemoved { session } | Event::Exit { session, .. }) => {
                    state().lock().unwrap().fronts.remove(&session);
                }
                Ok(Event::State { session, state: SessionState::Done }) => {
                    state().lock().unwrap().fronts.remove(&session);
                }
                Ok(Event::TurnEnded { session }) => {
                    let agent = state().lock().unwrap().fronts.get(&session).cloned();
                    if let Some(agent) = agent {
                        start(&agent, Trigger::TurnEnd);
                    }
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    let sessions = tokio::time::timeout(SESSIONS_TIMEOUT, c.list_sessions()).await??;
                    state().lock().unwrap().fronts = sessions.iter().filter_map(|x| front(x).map(|a| (x.id.clone(), a))).collect();
                }
                Err(RecvError::Closed) => anyhow::bail!("skiffd closed the connection"),
            },
            _ = tick.tick() => {
                if c.is_closed() {
                    anyhow::bail!("skiffd closed the connection");
                }
                for id in READERS {
                    start(id, Trigger::Poll);
                }
            }
        }
    }
}

//! `skiff wait`: block until sessions reach a state.

use std::{collections::HashMap, time::Duration};

use anyhow::{bail, Result};
use skiff_client::Client;
use skiff_core::{protocol::Event, session::SessionState};
use tokio::{
    sync::broadcast::error::{RecvError, TryRecvError},
    time::Instant,
};

use crate::{print, resolve, sessions};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Target {
    Idle,
    Waiting,
    Done,
    /// Idle, waiting or done.
    AnyNotWorking,
}

impl Target {
    fn hit(self, s: SessionState) -> bool {
        match self {
            Target::Idle => s == SessionState::Idle,
            Target::Waiting => s == SessionState::Waiting,
            Target::Done => s == SessionState::Done,
            Target::AnyNotWorking => s != SessionState::Working,
        }
    }
}

/// Where one session stands.
#[derive(Clone, Copy, PartialEq)]
enum Now {
    State(SessionState),
    /// Closed, or killed.
    Gone,
}

/// Exit codes: 0 when every session got there, 1 on timeout, 2 when a
/// session went away or exited before it got there.
pub async fn wait(c: &Client, args: &[String], target: Target, timeout: Option<f64>) -> Result<i32> {
    // Listen before the list, so no change falls between the two.
    let mut events = c.events();
    let all = sessions::sorted(c).await?;
    let mut ids: Vec<String> = Vec::new();
    let mut names = HashMap::new();
    for a in args {
        let s = match resolve::session(&all, a) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skiff: {e:#}");
                return Ok(2);
            }
        };
        if !ids.contains(&s.id) {
            ids.push(s.id.clone());
            names.insert(s.id.clone(), resolve::display_name(s).to_string());
        }
    }
    let mut now: HashMap<String, Now> =
        all.iter().filter(|s| ids.contains(&s.id)).map(|s| (s.id.clone(), Now::State(s.state))).collect();

    // A timeout too large to count is no timeout.
    let deadline = timeout
        .and_then(|t| Duration::try_from_secs_f64(t.max(0.0)).ok())
        .and_then(|d| Instant::now().checked_add(d));
    // Events queued before the list reply are older than the list.
    while !matches!(events.try_recv(), Err(TryRecvError::Empty | TryRecvError::Closed)) {}
    // A session settles when it reaches the target, or can no longer reach it.
    let settled = |n: &Now| match *n {
        Now::State(s) => target.hit(s) || s == SessionState::Done,
        Now::Gone => true,
    };
    let mut timed_out = false;
    while !ids.iter().all(|id| settled(&now[id])) {
        let next = async {
            match deadline {
                Some(d) => tokio::time::timeout_at(d, events.recv()).await.ok(),
                None => Some(events.recv().await),
            }
        };
        let Some(e) = next.await else {
            timed_out = true;
            break;
        };
        let (id, n) = match e {
            Ok(Event::State { session, state }) => (session, Now::State(state)),
            Ok(Event::Exit { session, .. }) => (session, Now::State(SessionState::Done)),
            Ok(Event::SessionRemoved { session }) => (session, Now::Gone),
            Ok(_) => continue,
            Err(RecvError::Lagged(_)) => {
                // Missed events: read the states again.
                let all = c.list_sessions().await?;
                for id in &ids {
                    let cur = now.get_mut(id).expect("listed");
                    if !settled(cur) {
                        *cur = all.iter().find(|s| &s.id == id).map_or(Now::Gone, |s| Now::State(s.state));
                    }
                }
                continue;
            }
            Err(RecvError::Closed) => bail!("skiffd closed the connection"),
        };
        // Once settled, a session keeps the state it settled in.
        if let Some(cur) = now.get_mut(&id) {
            if !settled(cur) {
                *cur = n;
            }
        }
    }

    let mut code = 0;
    let rows: Vec<Vec<String>> = ids
        .iter()
        .map(|id| {
            let n = now[id];
            let state = match n {
                Now::State(s) => sessions::state(s),
                Now::Gone => "gone",
            };
            let c = match n {
                Now::State(s) if target.hit(s) => 0,
                _ if !settled(&n) => 1,
                _ => 2,
            };
            code = code.max(c);
            vec![print::short_id(id).to_string(), names[id].clone(), state.to_string()]
        })
        .collect();
    print::table(&["", "", ""], &rows);
    if timed_out && code == 1 {
        eprintln!("skiff: timed out");
    }
    Ok(code)
}

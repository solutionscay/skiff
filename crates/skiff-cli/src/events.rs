//! `skiff events`: print each state change, exit and removal as it happens,
//! and what a program reports about itself (OSC 7501) and its turns' ends.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use anyhow::{bail, Result};
use serde_json::json;
use skiff_client::Client;
use skiff_core::{
    protocol::Event,
    session::{ProgramStatus, SessionInfo, SessionState},
};
use tokio::{
    sync::broadcast::error::{RecvError, TryRecvError},
    time::Instant,
};

use crate::{print, resolve, sessions};

/// One line to print.
enum Line {
    State(SessionState),
    Exit(Option<i32>),
    Removed,
    /// What the program reports changed: its state, what it waits for, or
    /// its message. `None` when it reports nothing any more.
    Status(Option<ProgramStatus>),
    /// A turn ended: a result to read.
    Turn,
}

/// The part of a report a `status` line tells: progress and title alone
/// change too often to print.
type Told = Option<(String, Option<String>, Option<String>, bool)>;

fn told(s: &SessionInfo) -> Told {
    s.program_status
        .as_ref()
        .map(|p| (p.state.clone(), p.kind.clone(), p.msg.clone(), p.ended))
}

/// `blocked (permission): Allow Edit to hello.txt?`, `done (ended)`, `none`.
fn status_text(p: Option<&ProgramStatus>) -> String {
    let Some(p) = p else { return "none".into() };
    let mut t = p.state.clone();
    if let Some(kind) = &p.kind {
        t += &format!(" ({kind})");
    }
    if p.ended {
        t += " (ended)";
    }
    if let Some(msg) = &p.msg {
        t += &format!(": {msg}");
    }
    t
}

/// A kind of line that ends `skiff events --until`.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    /// A `turn` line: a turn ended.
    Turn,
    /// An `exit` line.
    Exit,
    /// A `removed` line.
    Removed,
    /// A `status` line whose state is `blocked`.
    Blocked,
    /// A state line whose state is `waiting`.
    Waiting,
}

impl Until {
    fn matches(self, line: &Line) -> bool {
        match (self, line) {
            (Self::Turn, Line::Turn) | (Self::Exit, Line::Exit(_)) | (Self::Removed, Line::Removed) => true,
            (Self::Blocked, Line::Status(Some(p))) => p.state == "blocked",
            (Self::Waiting, Line::State(s)) => *s == SessionState::Waiting,
            _ => false,
        }
    }
}

/// Resolves when stdout is a pipe or socket and its reader closed it. A
/// writer learns that only on its next write, and an idle session writes
/// nothing. Never resolves for a terminal or a file.
async fn stdout_closed() {
    // SAFETY: fstat and poll on fd 1 read only kernel state.
    let piped = unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        libc::fstat(libc::STDOUT_FILENO, &mut st) == 0
            && matches!(st.st_mode & libc::S_IFMT, libc::S_IFIFO | libc::S_IFSOCK)
    };
    if !piped {
        return std::future::pending().await;
    }
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    // A plain thread: the process exits without waiting for it.
    std::thread::spawn(move || loop {
        // macOS reports POLLHUP only when it is asked for; Linux reports
        // POLLERR on a pipe whose reader is gone.
        let mut fd = libc::pollfd { fd: libc::STDOUT_FILENO, events: libc::POLLHUP | libc::POLLERR, revents: 0 };
        let n = unsafe { libc::poll(&mut fd, 1, -1) };
        if n > 0 && fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            let _ = tx.send(());
            return;
        }
        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return;
        }
    });
    if rx.await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Runs until Ctrl+C, the timeout, until every named session is gone, or
/// until stdout closes. With `until`, the first line of one of those kinds,
/// from any session, prints and ends it.
/// Exit codes: 0 at the end, 1 on an error, 2 when a name matches no session.
/// With `until`: 0 on a match, 1 when the timeout ends first, 2 when every
/// named session is gone first.
/// With no names, it follows every session, also the ones made later.
pub async fn events(c: &Client, args: &[String], json: bool, timeout: Option<f64>, until: &[Until]) -> Result<i32> {
    // Listen before the list, so no change falls between the two.
    let mut events = c.events();
    let all = sessions::sorted(c).await?;
    // Events queued before the list reply are older than the list. Drained
    // at once: an event that comes while the names resolve is printed.
    while !matches!(events.try_recv(), Err(TryRecvError::Empty | TryRecvError::Closed)) {}
    let mut only: Option<Vec<String>> = None;
    if !args.is_empty() {
        let mut ids = Vec::new();
        for a in args {
            match resolve::session(&all, a) {
                Ok(s) if !ids.contains(&s.id) => ids.push(s.id.clone()),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("skiff: {e:#}");
                    return Ok(2);
                }
            }
        }
        only = Some(ids);
    }
    let follows = |id: &str| only.as_ref().is_none_or(|ids| ids.iter().any(|i| i == id));
    let mut names: HashMap<String, String> = HashMap::new();
    let mut last: HashMap<String, SessionState> = HashMap::new();
    let mut reports: HashMap<String, Told> = HashMap::new();
    // State and exit events can come after the removal. They print nothing.
    let mut removed: HashSet<String> = HashSet::new();
    for s in all.iter().filter(|s| follows(&s.id)) {
        know(&mut names, s);
        last.insert(s.id.clone(), s.state);
        reports.insert(s.id.clone(), told(s));
    }

    // A timeout too large to count is no timeout.
    let deadline = timeout
        .and_then(|t| Duration::try_from_secs_f64(t.max(0.0)).ok())
        .and_then(|d| Instant::now().checked_add(d));
    let print_line = |id: &str, name: &str, line: Line| {
        if json {
            let v = match line {
                Line::State(s) => json!({ "event": "state", "session": id, "name": name, "state": s }),
                Line::Exit(code) => json!({ "event": "exit", "session": id, "name": name, "code": code }),
                Line::Removed => json!({ "event": "removed", "session": id, "name": name }),
                Line::Status(p) => json!({ "event": "status", "session": id, "name": name, "program_status": p }),
                Line::Turn => json!({ "event": "turn", "session": id, "name": name }),
            };
            println!("{v}");
        } else {
            let what = match line {
                Line::State(s) => sessions::state(s).to_string(),
                Line::Exit(Some(code)) => format!("exit {code}"),
                Line::Exit(None) => "exit".to_string(),
                Line::Removed => "removed".to_string(),
                Line::Status(p) => format!("status {}", status_text(p.as_ref())),
                Line::Turn => "turn ended".to_string(),
            };
            print::table(&["", "", ""], &[vec![print::short_id(id).to_string(), name.to_string(), what]]);
        }
    };
    let mut closed = std::pin::pin!(stdout_closed());
    // The lines one event makes, printed after it is read.
    let mut out: Vec<(String, Line)> = Vec::new();

    loop {
        if only.as_ref().is_some_and(|ids| ids.iter().all(|id| !last.contains_key(id))) {
            return Ok(if until.is_empty() { 0 } else { 2 });
        }
        let next = async {
            match deadline {
                Some(d) => tokio::time::timeout_at(d, events.recv()).await.ok(),
                None => Some(events.recv().await),
            }
        };
        let e = tokio::select! {
            e = next => e,
            () = &mut closed => return Ok(0),
        };
        let Some(e) = e else {
            return Ok(if until.is_empty() { 0 } else { 1 });
        };
        match e {
            Ok(Event::SessionCreated { session } | Event::SessionUpdated { session }) if follows(&session.id) => {
                know(&mut names, &session);
                last.entry(session.id.clone()).or_insert(session.state);
                let now = told(&session);
                let before = reports.insert(session.id.clone(), now.clone());
                // A new session says nothing until it reports.
                if before.unwrap_or(None) != now {
                    out.push((session.id.clone(), Line::Status(session.program_status.clone())));
                }
            }
            Ok(Event::TurnEnded { session }) if follows(&session) && !removed.contains(&session) => {
                out.push((session.clone(), Line::Turn));
            }
            Ok(Event::State { session, .. } | Event::Exit { session, .. }) if removed.contains(&session) => {}
            Ok(Event::State { session, state }) if follows(&session) => {
                // The daemon can repeat a state. Print changes only.
                if last.insert(session.clone(), state) != Some(state) {
                    out.push((session.clone(), Line::State(state)));
                }
            }
            Ok(Event::Exit { session, code, .. }) if follows(&session) => {
                out.push((session.clone(), Line::Exit(code)));
            }
            Ok(Event::SessionRemoved { session }) if follows(&session) => {
                removed.insert(session.clone());
                if last.remove(&session).is_some() {
                    out.push((session.clone(), Line::Removed));
                }
            }
            Ok(_) => {}
            Err(RecvError::Lagged(_)) => {
                // Missed events: read the states again and print what differs.
                let all = c.list_sessions().await?;
                let gone: Vec<String> = last.keys().filter(|id| !all.iter().any(|s| &s.id == *id)).cloned().collect();
                for id in gone {
                    last.remove(&id);
                    removed.insert(id.clone());
                    out.push((id.clone(), Line::Removed));
                }
                for s in all.iter().filter(|s| follows(&s.id)) {
                    know(&mut names, s);
                    if last.insert(s.id.clone(), s.state) != Some(s.state) {
                        out.push((s.id.clone(), Line::State(s.state)));
                    }
                    if reports.insert(s.id.clone(), told(s)).is_some_and(|b| b != told(s)) {
                        out.push((s.id.clone(), Line::Status(s.program_status.clone())));
                    }
                }
            }
            Err(RecvError::Closed) => bail!("skiffd closed the connection"),
        }
        for (id, line) in out.drain(..) {
            let hit = until.iter().any(|u| u.matches(&line));
            print_line(&id, names.get(&id).map_or("", String::as_str), line);
            if hit {
                return Ok(0);
            }
        }
    }
}

fn know(names: &mut HashMap<String, String>, s: &SessionInfo) {
    names.insert(s.id.clone(), resolve::display_name(s).to_string());
}

#[cfg(test)]
mod tests {
    use super::status_text;

    #[test]
    fn a_status_line_says_state_kind_and_message() {
        let p = |v: serde_json::Value| serde_json::from_value(v).unwrap();
        assert_eq!(status_text(None), "none");
        assert_eq!(
            status_text(Some(&p(serde_json::json!({"state": "blocked", "kind": "permission", "msg": "Allow Edit to hello.txt?"})))),
            "blocked (permission): Allow Edit to hello.txt?"
        );
        assert_eq!(status_text(Some(&p(serde_json::json!({"state": "error", "ended": true})))), "error (ended)");
    }
}

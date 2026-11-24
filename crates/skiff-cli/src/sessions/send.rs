use std::{io::Read, time::Duration};

use anyhow::{bail, Context, Result};
use skiff_client::Client;
use skiff_core::session::{Role, SessionInfo, SessionState};

use crate::{print, resolve, watch};

/// Why `send` must not type into `s`, as an exit code and a message.
/// `here` is `SKIFF_SESSION`; `caller_shell` is the caller's `SHELL`.
/// Checks run in a fixed order; `--shell` relaxes only the last one.
pub(super) fn refusal(s: &SessionInfo, here: Option<&str>, shell_ok: bool, caller_shell: Option<&str>) -> Option<(i32, String)> {
    let who = format!("{} ({})", resolve::display_name(s), print::short_id(&s.id));
    if here.is_some_and(|h| h == s.id) {
        return Some((7, format!("{who} is your own session. Send to another session.")));
    }
    match s.state {
        SessionState::Waiting => return Some((3, waiting_msg(&resolve::display_name(s), &s.id, s.waiting_cause.as_deref()))),
        SessionState::Working => {
            return Some((4, format!("{who} is busy. Skiff does not type into a working session.")))
        }
        SessionState::Done => return Some((6, format!("{who} is done. Its program exited."))),
        SessionState::Idle => {}
    }
    if shell_ok {
        return None;
    }
    let prog = s.foreground_program.as_deref().map(basename).unwrap_or("");
    if prog.is_empty() {
        return Some((
            5,
            format!("Skiff cannot identify the foreground program of {who}. Use --shell to send anyway."),
        ));
    }
    let caller = caller_shell.map(basename).unwrap_or("");
    if SHELLS.contains(&prog) || (!caller.is_empty() && prog == caller) {
        // `skiff new -- CMD` and custom agents run under a `sh` launch
        // wrapper, so their own program does not show.
        if prog == "sh" && s.role != Role::Shell {
            return Some((
                5,
                format!("Skiff cannot identify the program in front of {who}; it shows the launch shell. Use --shell to send anyway."),
            ));
        }
        return Some((5, format!("{who} has a shell ({prog}) in front. Use --shell to type a command into it.")));
    }
    None
}

/// The message for a session that needs a person: `send` and `wait` share it.
pub fn waiting_msg(name: &str, id: &str, cause: Option<&str>) -> String {
    let why = match cause {
        Some("bell") => "it rang the bell",
        Some("prompt") => "a prompt on screen asks for an answer",
        Some("title") => "its title asks for an action",
        Some("report") => "it reported that it is blocked",
        Some(other) => other,
        // A daemon older than `waiting_cause`, or it no longer waits.
        None => "approval prompt or bell",
    };
    format!("{name} ({}) is waiting: {why}. Check its pane in Skiff.", print::short_id(id))
}

const SHELLS: [&str; 9] = ["bash", "zsh", "fish", "sh", "dash", "ksh", "tcsh", "csh", "nu"];

/// A program's file name. A login shell's leading `-` is dropped.
fn basename(p: &str) -> &str {
    let p = p.trim();
    let name = p.rsplit('/').next().unwrap_or(p);
    name.strip_prefix('-').unwrap_or(name)
}

pub struct Send<'a> {
    pub text: &'a [String],
    pub enter: bool,
    pub raw: bool,
    pub shell: bool,
    /// The text `-` reads stdin. False when the caller read it already.
    pub stdin: bool,
}

/// `send`'s exit code when the pasted text did not show on the agent's
/// screen. Enter was not pressed.
pub const PASTE_UNSEEN: i32 = 11;
/// `send`'s exit code when Enter went to a known agent, but no turn started.
pub const NO_TURN: i32 = 8;

/// The step of a send that the agent's screen or state did not confirm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unconfirmed {
    Paste,
    Enter,
}

impl Unconfirmed {
    /// The step as `send` exits for it.
    pub fn of(code: i32) -> Option<Unconfirmed> {
        match code {
            PASTE_UNSEEN => Some(Unconfirmed::Paste),
            NO_TURN => Some(Unconfirmed::Enter),
            _ => None,
        }
    }

    /// The pane's waiting cause, and the delivery record's failure.
    pub fn cause(self) -> &'static str {
        match self {
            Unconfirmed::Paste => "send not confirmed: paste",
            Unconfirmed::Enter => "send not confirmed: enter",
        }
    }
}

/// Exit codes: 0 when sent and confirmed; 3 waiting, 4 working (its state or
/// its busy line), 5 a shell or unknown program in front, 6 done, 7 own
/// session, 9 input after the check. A refusal types nothing.
/// 11 ([`PASTE_UNSEEN`]): the paste did not show on the agent's screen, and
/// Enter was not pressed. 8 ([`NO_TURN`]): Enter went in, but no turn started.
/// After 11 or 8 the pane waits for the person.
pub async fn send(c: &Client, arg: &str, o: Send<'_>) -> Result<i32> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    send_to(c, s, o).await
}

/// [`send`] to a session from a list read just now: its `input_gen` guards
/// the paste and the Enter.
///
/// To a known agent, a send has two steps, each confirmed before the next:
/// the paste shows on the screen, then Enter starts a turn. Skiff does not
/// guess whether the input box holds text before it pastes.
pub async fn send_to(c: &Client, s: &SessionInfo, o: Send<'_>) -> Result<i32> {
    let here = std::env::var("SKIFF_SESSION").ok().filter(|h| !h.is_empty());
    let caller_shell = std::env::var("SHELL").ok();
    if let Some((code, msg)) = refusal(s, here.as_deref(), o.shell, caller_shell.as_deref()) {
        eprintln!("skiff: {msg}");
        return Ok(code);
    }
    let who = format!("{} ({})", resolve::display_name(s), print::short_id(&s.id));
    let (enter, raw) = (o.enter, o.raw);
    // Only a known agent proves a turn by its state, and only its screen
    // shows the paste. A shell command can finish with no output.
    let agent = !raw && is_agent(s);
    let mut data = if o.stdin && o.text == ["-"] {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).context("read stdin")?;
        buf
    } else {
        o.text.join(" ").into_bytes()
    };
    let text = String::from_utf8_lossy(&data).into_owned();
    if !raw {
        data = enter_keys(&data);
    }
    // `--enter` presses Enter once. A newline at the end of the text, as a
    // heredoc or a file has, would only add a blank line before it.
    if enter {
        while data.last() == Some(&b'\r') || data.last() == Some(&b'\n') {
            data.pop();
        }
    }
    if agent {
        return agent_send(c, s, data, &text, enter).await;
    }
    // A paste, so the program keeps the text's newlines as text. skiffd
    // types it only if the session took no input since the list.
    if !c.submit(&s.id, data, !raw, enter, s.input_gen).await? {
        eprintln!("skiff: {who} took input after the check. Nothing was typed.");
        return Ok(9);
    }
    Ok(0)
}

/// Types `data` (the text `text` as keys) into the known agent `s`, from a
/// list read just now, in two confirmed steps: the paste shows on the
/// screen, then with `enter` a turn starts. The watcher's lines take the
/// same path. Returns `send`'s exit code: 0, 4 (busy line), 9 (input after
/// the check), [`PASTE_UNSEEN`] or [`NO_TURN`]. The last two set the pane to
/// need the person.
pub async fn agent_send(c: &Client, s: &SessionInfo, data: Vec<u8>, text: &str, enter: bool) -> Result<i32> {
    let who = format!("{} ({})", resolve::display_name(s), print::short_id(&s.id));
    let before = watch::read_rows(c, &s.id).await?;
    if watch::front(&before, s.foreground_program.as_deref().unwrap_or("")) == watch::Front::Busy {
        eprintln!("skiff: {who} shows its busy line. Skiff does not type into a working agent.");
        return Ok(4);
    }
    // 1. The paste, alone. Enter waits for the screen to show it.
    let typed = !data.is_empty();
    if typed && !c.submit(&s.id, data, true, false, s.input_gen).await? {
        eprintln!("skiff: {who} took input after the check. Nothing was typed.");
        return Ok(9);
    }
    let (pasted, after) = if typed {
        match paste_shows(c, &s.id, &before, text).await? {
            Some(seen) => seen,
            None => {
                eprintln!("skiff: {who}: the pasted text did not show on its screen. Enter was not pressed. Check its pane.");
                not_delivered(c, s, &who, Unconfirmed::Paste).await;
                return Ok(PASTE_UNSEEN);
            }
        }
    } else {
        (Vec::new(), before)
    };
    if !enter {
        return Ok(0);
    }
    // 2. Enter. Listen before it, so no change falls before the listener.
    let events = c.events();
    let gen = s.input_gen + u64::from(typed);
    if !c.submit(&s.id, b"\r".to_vec(), false, false, gen).await? {
        eprintln!("skiff: {who} took input after the paste. Enter was not pressed. The text can be in its input box. Check its pane.");
        not_delivered(c, s, &who, Unconfirmed::Enter).await;
        return Ok(NO_TURN);
    }
    if enter_took(c, &s.id, events, &pasted, &after).await? {
        return Ok(0);
    }
    // Never press Enter again: it can submit twice or answer a menu.
    eprintln!("skiff: {who}: text sent, but no turn started. It can be in the input box. Check its pane.");
    not_delivered(c, s, &who, Unconfirmed::Enter).await;
    Ok(NO_TURN)
}

/// How long the paste has to show on the screen.
const PASTE_WAIT: Duration = Duration::from_secs(2);
/// How long an agent has to start a turn after a confirmed paste and Enter.
const ENTER_WAIT: Duration = Duration::from_secs(10);
/// How often a send reads the screen while it waits.
const SCREEN_POLL: Duration = Duration::from_millis(250);

/// Reads the screen until the paste of `text` shows, or [`PASTE_WAIT`] ends.
/// Returns the rows that show it, and the screen then.
async fn paste_shows(c: &Client, id: &str, before: &[String], text: &str) -> Result<Option<(Vec<String>, Vec<String>)>> {
    let deadline = tokio::time::Instant::now() + PASTE_WAIT;
    loop {
        tokio::time::sleep(SCREEN_POLL).await;
        let now = watch::read_rows(c, id).await?;
        let rows = paste_rows(before, &now, text);
        if !rows.is_empty() {
            return Ok(Some((rows, now)));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
    }
}

/// The rows of `after` that show the paste of `text` and that `before` did
/// not have as many times: a row with the start of the text's first
/// non-blank line, or a paste placeholder (Claude Code's `[Pasted text #1
/// +12 lines]`). Spaces do not count: an agent can wrap or indent the text.
pub(super) fn paste_rows(before: &[String], after: &[String], text: &str) -> Vec<String> {
    let needle = needle(text);
    let shows = |row: &str| {
        let row = squeeze(row);
        (!needle.is_empty() && row.contains(&needle)) || row.to_lowercase().contains("[pasted")
    };
    let count = |rows: &[String], r: &String| rows.iter().filter(|x| *x == r).count();
    let mut out: Vec<String> = Vec::new();
    for r in after.iter().filter(|r| shows(r)) {
        if !out.contains(r) && count(after, r) > count(before, r) {
            out.push(r.clone());
        }
    }
    out
}

/// The first [`NEEDLE`] characters of the text's first non-blank line, without spaces.
fn needle(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    squeeze(line).chars().take(NEEDLE).collect()
}

/// Enough of the text to tell it from the rest of the screen, and short
/// enough to fit one row.
const NEEDLE: usize = 24;

fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// True when a turn starts within [`ENTER_WAIT`] after Enter: the session
/// leaves idle, its program reports work (OSC 7501), or the rows that showed
/// the paste leave the screen. An exit or a removal also counts.
async fn enter_took(
    c: &Client,
    id: &str,
    mut events: tokio::sync::broadcast::Receiver<skiff_core::protocol::Event>,
    pasted: &[String],
    after: &[String],
) -> Result<bool> {
    use skiff_core::protocol::Event;
    use tokio::sync::broadcast::error::RecvError;
    let deadline = tokio::time::Instant::now() + ENTER_WAIT;
    let mut poll = tokio::time::interval(SCREEN_POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    poll.tick().await;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            e = events.recv() => match e {
                Ok(Event::State { session, state }) if session == id && state != SessionState::Idle => return Ok(true),
                Ok(Event::SessionUpdated { session }) if session.id == id && reports_work(&session) => return Ok(true),
                Ok(Event::Exit { session, .. } | Event::SessionRemoved { session }) if session == id => return Ok(true),
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    if !still_idle(c, id).await? {
                        return Ok(true);
                    }
                }
                Err(RecvError::Closed) => bail!("skiffd closed the connection"),
            },
            _ = poll.tick(), if !pasted.is_empty() => {
                if paste_left(after, &watch::read_rows(c, id).await?, pasted) {
                    return Ok(true);
                }
            }
        }
    }
    // The Enter can start work before the listener reads it. Read the state.
    Ok(!still_idle(c, id).await?)
}

/// The program in front reports work now (OSC 7501).
fn reports_work(s: &SessionInfo) -> bool {
    s.program_status.as_ref().is_some_and(|p| p.state == "working" && !p.ended)
}

/// A row that showed the paste shows fewer times on `now` than on `after`:
/// the text left the input box.
pub(super) fn paste_left(after: &[String], now: &[String], pasted: &[String]) -> bool {
    let count = |rows: &[String], r: &String| rows.iter().filter(|x| *x == r).count();
    pasted.iter().any(|r| count(now, r) < count(after, r))
}

/// A send that did not deliver: the pane waits for the person with the
/// failed step as its cause, and the desktop says so. Best effort: an older
/// skiffd has no request for it.
async fn not_delivered(c: &Client, s: &SessionInfo, who: &str, step: Unconfirmed) {
    if let Err(e) = c.needs_you(&s.id, step.cause()).await {
        eprintln!("skiff: cannot set {who} to need you: {e:#}");
    }
    watch::desktop_notify(&format!("{who} needs you"), &format!("{}. Check its pane.", upper_first(step.cause())));
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The program in front is an agent Skiff knows. The daemon names a known
/// agent by its id ("claude", "codex"...), whatever binary runs it.
pub fn is_agent(s: &SessionInfo) -> bool {
    let prog = s.foreground_program.as_deref().map(basename).unwrap_or("");
    skiff_core::project::KNOWN_AGENTS.iter().any(|(id, _)| *id == prog)
}

async fn still_idle(c: &Client, id: &str) -> Result<bool> {
    let all = c.list_sessions().await?;
    Ok(all.iter().any(|s| s.id == id && s.state == SessionState::Idle))
}

/// Newlines as the Enter key sends them: `\r`.
pub(super) fn enter_keys(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'\r' if data.get(i + 1) == Some(&b'\n') => {
                out.push(b'\r');
                i += 1;
            }
            b'\n' => out.push(b'\r'),
            b => out.push(b),
        }
        i += 1;
    }
    out
}

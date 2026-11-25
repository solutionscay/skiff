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
    Paste(Miss),
    Enter,
}

/// What the screen showed when a paste was not confirmed in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Miss {
    /// The input box changed (with no known box, the screen), but it does
    /// not show the text.
    BoxChanged,
    /// Nothing changed.
    NoChange,
}

impl Unconfirmed {
    /// The pane's waiting cause. Each starts with `send not confirmed: `
    /// and the step.
    pub fn cause(self) -> &'static str {
        match self {
            Unconfirmed::Paste(Miss::BoxChanged) => "send not confirmed: paste (the input box changed)",
            Unconfirmed::Paste(Miss::NoChange) => "send not confirmed: paste (nothing showed)",
            Unconfirmed::Enter => "send not confirmed: enter",
        }
    }
}

/// What came of a send: its exit code, and what its screen checks found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    pub code: i32,
    /// The step not confirmed, with exit [`PASTE_UNSEEN`] or [`NO_TURN`].
    pub unconfirmed: Option<Unconfirmed>,
    /// A confirmed paste: true when the input box held all of the text and
    /// nothing else, or one paste placeholder whose line count fits it.
    pub exact: Option<bool>,
}

impl Outcome {
    fn code(code: i32) -> Self {
        Outcome { code, unconfirmed: None, exact: None }
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
    Ok(send_to_outcome(c, s, o).await?.code)
}

/// [`send_to`], with what its screen checks found.
pub async fn send_to_outcome(c: &Client, s: &SessionInfo, o: Send<'_>) -> Result<Outcome> {
    let here = std::env::var("SKIFF_SESSION").ok().filter(|h| !h.is_empty());
    let caller_shell = std::env::var("SHELL").ok();
    if let Some((code, msg)) = refusal(s, here.as_deref(), o.shell, caller_shell.as_deref()) {
        eprintln!("skiff: {msg}");
        return Ok(Outcome::code(code));
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
        return agent_send_outcome(c, s, data, &text, enter).await;
    }
    // A paste, so the program keeps the text's newlines as text. skiffd
    // types it only if the session took no input since the list.
    if !c.submit(&s.id, data, !raw, enter, s.input_gen).await? {
        eprintln!("skiff: {who} took input after the check. Nothing was typed.");
        return Ok(Outcome::code(9));
    }
    Ok(Outcome::code(0))
}

/// Types `data` (the text `text` as keys) into the known agent `s`, from a
/// list read just now, in two confirmed steps: the paste shows on the
/// screen, then with `enter` a turn starts. The watcher's lines take the
/// same path. Returns `send`'s exit code: 0, 4 (busy line), 9 (input after
/// the check), [`PASTE_UNSEEN`] or [`NO_TURN`]. The last two set the pane to
/// need the person.
pub async fn agent_send(c: &Client, s: &SessionInfo, data: Vec<u8>, text: &str, enter: bool) -> Result<i32> {
    Ok(agent_send_outcome(c, s, data, text, enter).await?.code)
}

/// [`agent_send`], with what its screen checks found.
pub async fn agent_send_outcome(c: &Client, s: &SessionInfo, data: Vec<u8>, text: &str, enter: bool) -> Result<Outcome> {
    let who = format!("{} ({})", resolve::display_name(s), print::short_id(&s.id));
    let agent = s.foreground_program.as_deref().unwrap_or("");
    let before = watch::read_rows(c, &s.id).await?;
    if watch::front(&before, agent) == watch::Front::Busy {
        eprintln!("skiff: {who} shows its busy line. Skiff does not type into a working agent.");
        return Ok(Outcome::code(4));
    }
    // 1. The paste, alone. Enter waits for the screen to show it.
    let typed = !data.is_empty();
    if typed && !c.submit(&s.id, data, true, false, s.input_gen).await? {
        eprintln!("skiff: {who} took input after the check. Nothing was typed.");
        return Ok(Outcome::code(9));
    }
    let (pasted, after, exact) = if typed {
        match paste_shows(c, &s.id, agent, &before, text).await? {
            (Seen::Shown { rows, exact }, after) => (rows, after, Some(exact)),
            (Seen::Missed(miss), _) => {
                let what = match miss {
                    Miss::BoxChanged => "its input box changed, but it does not show the pasted text",
                    Miss::NoChange => "the pasted text did not show on its screen",
                };
                eprintln!("skiff: {who}: {what}. Enter was not pressed. Check its pane.");
                let step = Unconfirmed::Paste(miss);
                not_delivered(c, s, &who, step).await;
                return Ok(Outcome { code: PASTE_UNSEEN, unconfirmed: Some(step), exact: None });
            }
        }
    } else {
        (Vec::new(), before, None)
    };
    let sent = |code, unconfirmed| Outcome { code, unconfirmed, exact };
    if !enter {
        return Ok(sent(0, None));
    }
    // 2. Enter. Listen before it, so no change falls before the listener.
    let events = c.events();
    let gen = s.input_gen + u64::from(typed);
    if !c.submit(&s.id, b"\r".to_vec(), false, false, gen).await? {
        eprintln!("skiff: {who} took input after the paste. Enter was not pressed. The text can be in its input box. Check its pane.");
        not_delivered(c, s, &who, Unconfirmed::Enter).await;
        return Ok(sent(NO_TURN, Some(Unconfirmed::Enter)));
    }
    if enter_took(c, &s.id, events, &pasted, &after).await? {
        return Ok(sent(0, None));
    }
    // Never press Enter again: it can submit twice or answer a menu.
    eprintln!("skiff: {who}: text sent, but no turn started. It can be in the input box. Check its pane.");
    not_delivered(c, s, &who, Unconfirmed::Enter).await;
    Ok(sent(NO_TURN, Some(Unconfirmed::Enter)))
}

/// How long the paste has to show on the screen. A send goes on at the
/// first read that shows it.
const PASTE_WAIT: Duration = Duration::from_secs(5);
/// How long an agent has to start a turn after a confirmed paste and Enter.
const ENTER_WAIT: Duration = Duration::from_secs(10);
/// How often a send reads the screen while it waits.
const SCREEN_POLL: Duration = Duration::from_millis(250);

/// Reads the screen of `agent` until the paste of `text` shows, or
/// [`PASTE_WAIT`] ends. Returns what the last read showed, and that screen.
async fn paste_shows(c: &Client, id: &str, agent: &str, before: &[String], text: &str) -> Result<(Seen, Vec<String>)> {
    let deadline = tokio::time::Instant::now() + PASTE_WAIT;
    loop {
        tokio::time::sleep(SCREEN_POLL).await;
        let now = watch::read_rows(c, id).await?;
        let seen = paste_seen(before, &now, text, agent);
        if matches!(seen, Seen::Shown { .. }) || tokio::time::Instant::now() >= deadline {
            return Ok((seen, now));
        }
    }
}

/// What a screen shows of a paste.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Seen {
    /// The input box shows the text. `rows`: its rows that are new, for the
    /// Enter step. `exact`: see [`Outcome::exact`].
    Shown { rows: Vec<String>, exact: bool },
    Missed(Miss),
}

/// How much of the text the input box must show, without spaces: enough to
/// tell it from a suggestion or a hint.
const HEAD: usize = 64;

/// What `after` shows of the paste of `text` into `agent`, against `before`.
///
/// The rows of the input box are joined into one string, without the prompt
/// mark, box borders and whitespace, so a wrap at any column does not count;
/// the text is flattened the same way. The paste shows when the box holds
/// the text's first [`HEAD`] characters more times than before, or the box
/// is one new paste placeholder. With no known box, the rows that are new
/// since `before` stand for it, joined.
pub(super) fn paste_seen(before: &[String], after: &[String], text: &str, agent: &str) -> Seen {
    let want = flat(text);
    let head: String = want.chars().take(HEAD).collect();
    let (rows, now, was) = match paste_box(after, agent) {
        Some(range) => {
            let rows = &after[range];
            let was = paste_box(before, agent).map(|b| flat_rows(&before[b])).unwrap_or_default();
            (rows.to_vec(), flat_rows(rows), was)
        }
        None => {
            let rows = new_rows(before, after);
            let now = flat_rows(&rows);
            (rows, now, String::new())
        }
    };
    let holds = !head.is_empty() && now.matches(head.as_str()).count() > was.matches(head.as_str()).count();
    let placeholder = (now != was).then(|| placeholder(&now)).flatten();
    if holds || placeholder.is_some() {
        let exact = now == want || placeholder.is_some_and(|p| p.fits(text));
        let mut new = new_rows(before, &rows);
        if new.is_empty() {
            new = rows.into_iter().filter(|r| !flat(r).is_empty()).collect();
        }
        return Seen::Shown { rows: new, exact };
    }
    Seen::Missed(if now != was { Miss::BoxChanged } else { Miss::NoChange })
}

/// The rows that hold the agent's input box: the rows between its rules,
/// or for Codex the `›` row and the wrapped rows under it.
fn paste_box(rows: &[String], agent: &str) -> Option<std::ops::Range<usize>> {
    let r = watch::input_box_rows(rows, agent)?;
    if agent != "codex" {
        return Some(r);
    }
    let more = rows[r.end..].iter().take_while(|a| a.starts_with(' ') && !a.trim().is_empty()).count();
    Some(r.start..r.end + more)
}

/// The non-blank rows of `after` that show more times than on `before`.
fn new_rows(before: &[String], after: &[String]) -> Vec<String> {
    let count = |rows: &[String], r: &String| rows.iter().filter(|x| *x == r).count();
    let mut out: Vec<String> = Vec::new();
    for r in after {
        if !flat(r).is_empty() && !out.contains(r) && count(after, r) > count(before, r) {
            out.push(r.clone());
        }
    }
    out
}

/// Rows joined as [`flat`], without the prompt mark at the start of the
/// first one that holds text.
fn flat_rows(rows: &[String]) -> String {
    let mut out = String::new();
    for r in rows {
        let f = flat(r);
        if out.is_empty() {
            let f = f.strip_prefix(['❯', '>', '›']).unwrap_or(&f);
            out.push_str(f);
        } else {
            out.push_str(&f);
        }
    }
    out
}

/// The text without whitespace and box-drawing characters: what is left of
/// it however an agent wraps, indents or frames it.
fn flat(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && !('\u{2500}'..='\u{259f}').contains(c)).collect()
}

/// A paste placeholder: Claude Code's `[Pasted text #1 +12 lines]`,
/// OpenCode's `[Pasted ~12 lines]`, Codex's `[Pasted Content 1200 chars]`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placeholder {
    lines: Option<usize>,
    chars: Option<usize>,
}

impl Placeholder {
    /// The count it shows fits `text`. Agents count lines in their own way:
    /// newlines with or without the last one, or lines.
    fn fits(self, text: &str) -> bool {
        let trimmed = text.trim_end_matches(['\r', '\n']);
        let newlines = |t: &str| t.matches('\n').count();
        match (self.lines, self.chars) {
            (Some(l), _) => (newlines(trimmed)..=newlines(text) + 1).contains(&l),
            (None, Some(c)) => c == text.chars().count() || c == trimmed.chars().count(),
            (None, None) => false,
        }
    }
}

/// `box`, flattened, as one paste placeholder.
fn placeholder(flat: &str) -> Option<Placeholder> {
    let low = flat.to_lowercase();
    let inner = low.strip_prefix("[pasted")?.strip_suffix(']')?;
    if inner.contains(['[', ']']) {
        return None;
    }
    let count = |unit: &str| {
        let at = inner.find(unit)?;
        let digits: String = inner[..at].chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
        digits.parse().ok()
    };
    Some(Placeholder { lines: count("line"), chars: count("char") })
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

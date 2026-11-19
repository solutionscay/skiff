//! Reading an agent's screen: its front, a busy line, the input box, and the desktop notification.

use std::process::Stdio;

use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::cell::{Cell, Flags},
    vte::ansi::{Color, NamedColor},
};
use anyhow::Result;
use skiff_client::Client;
use skiff_core::session::SessionInfo;
use skiff_core::screen::{box_rule, codex_input, esc_interrupt_hint, interrupt_hint};

use crate::peek;

/// What a known agent's screen shows at its input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Front {
    /// An empty input box, and no busy line.
    Ready,
    /// The busy line: the agent works, though the pane is quiet.
    Busy,
    /// Text in the input box.
    Draft,
    /// No input box shows.
    NoBox,
}

pub(super) fn front(screen: &Screen, agent: &str) -> Front {
    if busy_line(&screen.rows) {
        return Front::Busy;
    }
    match input_box(screen, agent) {
        Some(false) => Front::Ready,
        Some(true) => Front::Draft,
        None => Front::NoBox,
    }
}

/// Reads the screen of session `s`, a known agent, for [`front`].
pub async fn read_front(c: &Client, s: &SessionInfo) -> Result<Front> {
    let agent = s.foreground_program.as_deref().unwrap_or("");
    let (data, cols, rows) = peek::snapshot(c, &s.id).await?;
    Ok(front(&rows_of(&data, cols, rows), agent))
}

/// The line an agent keeps up while it works: "esc to interrupt" and its
/// kin. The daemon calls a quiet stretch idle, so this is the second check.
pub(super) fn busy_line(rows: &[(String, String)]) -> bool {
    let end = rows.iter().rposition(|(a, _)| !a.is_empty()).map_or(0, |i| i + 1);
    rows[end.saturating_sub(8)..end].iter().any(|(a, _)| {
        let t: String = a.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
        interrupt_hint(&t) || esc_interrupt_hint(&t)
    })
}

/// The visible screen, read for [`front`].
pub struct Screen {
    /// Per row: all text, and the text without dim cells.
    pub rows: Vec<(String, String)>,
    /// Per row: the text without dim or colored cells: what the person can
    /// have typed. An agent draws a hint dim (Claude Code) or colored (agy).
    pub plain: Vec<String>,
    /// The cursor: its row, and its char index in that row's text.
    pub cursor: (usize, usize),
}

/// Whether `cell` can be text the person typed: not dim, and in the
/// terminal's default foreground.
fn typed(cell: &Cell) -> bool {
    !cell.flags.contains(Flags::DIM) && cell.fg == Color::Named(NamedColor::Foreground)
}

/// Reads the visible screen of a snapshot. skiffd's snapshot ends with the
/// real cursor position, hidden or not.
pub fn rows_of(data: &[u8], cols: u16, rows: u16) -> Screen {
    let term = peek::term(data, cols, rows);
    let grid = term.grid();
    let point = grid.cursor.point;
    let mut cursor = (point.line.0.max(0) as usize, 0);
    let mut plain = Vec::new();
    let rows = (0..grid.screen_lines() as i32)
        .map(|l| {
            let row = &grid[Line(l)];
            let (mut all, mut solid, mut text) = (String::new(), String::new(), String::new());
            for c in 0..grid.columns() {
                if l == point.line.0 && c == point.column.0 {
                    cursor.1 = all.chars().count();
                }
                let cell = &row[Column(c)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                let ch = if cell.c == '\0' { ' ' } else { cell.c };
                all.push(ch);
                solid.push(if cell.flags.contains(Flags::DIM) { ' ' } else { ch });
                text.push(if typed(cell) { ch } else { ' ' });
            }
            plain.push(text.trim_end().to_string());
            (all.trim_end().to_string(), solid.trim_end().to_string())
        })
        .collect();
    Screen { rows, plain, cursor }
}

/// Whether the agent's input box holds a draft. `None` when no input box
/// shows: then nothing is typed.
///
/// Codex marks its input line with `›`. The others draw the box between two
/// rules of box-drawing characters (Claude Code's `───`, Gemini's `╭──╮` and
/// `╰──╯`): the last two such rows on the screen.
///
/// With the cursor in the box, any text before the cursor is a draft, in any
/// color: Claude Code draws its bash mode `!` pink. At or after the cursor,
/// only plain text is: a hint there is dim or colored. With the cursor
/// elsewhere, only dim text is a hint: a doubt refuses the send.
pub(super) fn input_box(screen: &Screen, agent: &str) -> Option<bool> {
    let border = |s: &str| s.trim().trim_matches(|c| c == '│' || c == '┃').trim().to_string();
    let content = |s: &str| border(s).trim_start_matches(['>', '❯', '›']).trim().to_string();
    let rows = &screen.rows;
    let range = input_box_rows(rows, agent)?;
    let (line, col) = screen.cursor;
    if !range.contains(&line) {
        return Some(rows[range].iter().any(|(_, solid)| !content(solid).is_empty()));
    }
    let before = (range.start..=line).any(|i| {
        let all = &rows[i].0;
        let text: String = if i == line { all.chars().take(col).collect() } else { all.clone() };
        let rest = if i == range.start { content(&text) } else { border(&text) };
        !rest.is_empty()
    });
    let after = (line..range.end).any(|i| {
        let plain = &screen.plain[i];
        let text: String = if i == line { plain.chars().skip(col).collect() } else { plain.clone() };
        !border(&text).is_empty()
    });
    Some(before || after)
}

/// The visible row range inside the agent's input box.
pub fn input_box_rows(rows: &[(String, String)], agent: &str) -> Option<std::ops::Range<usize>> {
    // An agent that draws inline leaves blank rows under a short screen.
    let end = rows.iter().rposition(|(a, _)| !a.is_empty()).map_or(0, |i| i + 1);
    let start = end.saturating_sub(16);
    let near = &rows[start..end];
    if agent == "codex" {
        let line = start + near.iter().rposition(|(all, _)| codex_input(all))?;
        return Some(line..line + 1);
    }
    let cols = rows.iter().map(|(a, _)| a.chars().count()).max().unwrap_or(0);
    let marks: Vec<usize> = near.iter().enumerate().filter(|(_, (a, _))| a.trim().chars().count() >= 8 && box_rule(a, cols)).map(|(i, _)| i).collect();
    let [.., top, bottom] = marks[..] else { return None };
    if bottom - top < 2 {
        return None;
    }
    Some(start + top + 1..start + bottom)
}

/// Best effort. A missing tool leaves the log line as the only sign.
pub(super) fn desktop_notify(title: &str, body: &str) {
    let mut cmd = if cfg!(target_os = "macos") {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut c = std::process::Command::new("osascript");
        c.arg("-e").arg(format!("display notification \"{}\" with title \"{}\"", esc(body), esc(title)));
        c
    } else {
        let mut c = std::process::Command::new("notify-send");
        c.args(["--urgency=critical", "--app-name=Skiff", "--", title, body]);
        c
    };
    if let Ok(mut ch) = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        // Reap it without blocking the watcher.
        std::thread::spawn(move || {
            let _ = ch.wait();
        });
    }
}

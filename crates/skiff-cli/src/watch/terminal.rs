//! Reading an agent's screen: its front, a busy line, the input box rows, and the desktop notification.

use std::process::Stdio;

use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::cell::Flags,
};
use anyhow::Result;
use skiff_client::Client;
use skiff_core::session::SessionInfo;
use skiff_core::screen::{box_rule, codex_input, esc_interrupt_hint, interrupt_hint};

use crate::peek;

/// What a known agent's screen shows at its input. Skiff does not guess
/// whether the box holds text: a send confirms its paste on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Front {
    /// The input box, and no busy line.
    Ready,
    /// The busy line: the agent works, though the pane is quiet.
    Busy,
    /// No input box shows.
    NoBox,
}

pub fn front(rows: &[String], agent: &str) -> Front {
    if busy_line(rows) {
        return Front::Busy;
    }
    match input_box_rows(rows, agent) {
        Some(_) => Front::Ready,
        None => Front::NoBox,
    }
}

/// Reads the screen of session `s`, a known agent, for [`front`].
pub async fn read_front(c: &Client, s: &SessionInfo) -> Result<Front> {
    let agent = s.foreground_program.as_deref().unwrap_or("");
    Ok(front(&read_rows(c, &s.id).await?, agent))
}

/// The visible rows of session `id` now.
pub async fn read_rows(c: &Client, id: &str) -> Result<Vec<String>> {
    let (data, cols, rows) = peek::snapshot(c, id).await?;
    Ok(rows_of(&data, cols, rows))
}

/// The line an agent keeps up while it works: "esc to interrupt" and its
/// kin. The daemon calls a quiet stretch idle, so this is the second check.
pub(super) fn busy_line(rows: &[String]) -> bool {
    let end = rows.iter().rposition(|a| !a.is_empty()).map_or(0, |i| i + 1);
    rows[end.saturating_sub(8)..end].iter().any(|a| {
        let t: String = a.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
        interrupt_hint(&t) || esc_interrupt_hint(&t)
    })
}

/// The text of each visible row of a snapshot, trimmed at the end.
pub fn rows_of(data: &[u8], cols: u16, rows: u16) -> Vec<String> {
    let term = peek::term(data, cols, rows);
    let grid = term.grid();
    (0..grid.screen_lines() as i32)
        .map(|l| {
            let row = &grid[Line(l)];
            let mut all = String::new();
            for c in 0..grid.columns() {
                let cell = &row[Column(c)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                all.push(if cell.c == '\0' { ' ' } else { cell.c });
            }
            all.trim_end().to_string()
        })
        .collect()
}

/// The visible row range inside the agent's input box.
pub fn input_box_rows(rows: &[String], agent: &str) -> Option<std::ops::Range<usize>> {
    // An agent that draws inline leaves blank rows under a short screen.
    let end = rows.iter().rposition(|a| !a.is_empty()).map_or(0, |i| i + 1);
    let start = end.saturating_sub(16);
    let near = &rows[start..end];
    if agent == "codex" {
        let line = start + near.iter().rposition(|all| codex_input(all))?;
        return Some(line..line + 1);
    }
    let cols = rows.iter().map(|a| a.chars().count()).max().unwrap_or(0);
    let marks: Vec<usize> = near.iter().enumerate().filter(|(_, a)| a.trim().chars().count() >= 8 && box_rule(a, cols)).map(|(i, _)| i).collect();
    let [.., top, bottom] = marks[..] else { return None };
    if bottom - top < 2 {
        return None;
    }
    Some(start + top + 1..start + bottom)
}

/// Best effort. A missing tool leaves the log line as the only sign.
pub fn desktop_notify(title: &str, body: &str) {
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

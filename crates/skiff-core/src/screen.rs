//! Shared patterns for reading agent screens.
use serde::{Deserialize, Serialize};

/// Whether `agent` shows a prompt that stops its turn until the user
/// answers: an approval, a question, or the trust check at start. The
/// agents ring no bell for it, so the daemon reads the screen.
///
/// Each agent draws the prompt in place of its input box, with a line of
/// key hints at the bottom of the screen. The match reads only those last
/// rows, so the same words in the conversation above do not count.
pub fn approval_prompt(agent: &str, bottom_text: impl Fn(usize) -> String) -> bool {
    match agent {
        // "Esc to cancel · Tab to amend", "Enter to confirm · Esc to cancel".
        "claude" => bottom_text(2).contains("esctocancel"),
        "codex" => {
            let t = bottom_text(2);
            t.contains("pressentertoconfirmoresctocancel") || t.contains("entercontinue·escskip")
        }
        // "Allow once   Allow always   Reject".
        "opencode" => {
            let t = bottom_text(4);
            t.contains("allowonce") && t.contains("reject")
        }
        "gemini" => {
            // Antigravity (agy): "↑/↓ Navigate · tab Amend", "↑/↓ Navigate · enter Confirm".
            // Its slash-command menu shows "↑/↓ Navigate · enter Select · tab Complete".
            // Gemini CLI: "Allow once", "Allow for this session", "No, suggest changes (esc)".
            let t = bottom_text(10);
            (t.contains("↑/↓navigate")
                && (t.contains("tabamend") || t.contains("enterconfirm")))
                || (t.contains("allowonce") && t.contains("suggestchanges"))
        }
        "copilot" => {
            // A tool's "Do you want to create …?" ends with "3. No, and tell
            // Copilot what to do differently (Esc to stop)"; the folder trust
            // dialog asks "Do you trust the files in this folder?". Both
            // close with "↑/↓ to navigate · enter to select · esc to cancel".
            let t = bottom_text(14);
            t.contains("↑/↓tonavigate")
                && (t.contains("no,andtellcopilotwhattododifferently")
                    || t.contains("doyoutrustthefilesinthisfolder"))
        }
        "grok" => {
            // "1/4:select │ Tab:next option │ ...", or a command's
            // "Allow once", "Always allow this command", "Reject".
            let t = bottom_text(10);
            let hints = bottom_text(2);
            (hints.contains(":select") && hints.contains("tab:nextoption"))
                || (t.contains("allowonce") && t.contains("reject"))
        }
        _ => false,
    }
}

/// The agent shows the line it keeps up while it works, above its input
/// box. Proof of a turn, however short. Grok and Hermes have no such line known yet.
pub fn busy_line(agent: &str, bottom_text: impl Fn(usize) -> String) -> bool {
    if !has_busy_line(agent) {
        return false;
    }
    let t = bottom_text(6);
    match agent {
        // "esc to interrupt", "• Working (2s • esc to interrupt)". The key
        // follows the user's keymap, so the match leaves it out.
        "claude" | "codex" => interrupt_hint(&t),
        // "esc interrupt", and "esc again to interrupt" after one Esc.
        "opencode" => interrupt_hint(&t) || esc_interrupt_hint(&t),
        // "(esc to cancel, 12s)". Antigravity shows "esc to cancel" at
        // its prompts and its slash-command menu too, under a "↑/↓ Navigate" list.
        "gemini" => t.contains("esctocancel") && !t.contains("↑/↓navigate"),
        // "◎ Working esc esc edit prompt", "◎ Working · 19 B esc esc interrupt".
        "copilot" => t.contains("working") && t.contains("escesc"),
        _ => false,
    }
}


/// What an agent's window title says about its turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TitleState {
    Busy,
    /// The agent waits for the user's answer.
    Action,
    Ready,
}

/// Reads the state an agent puts in its window title. `None` when the agent
/// has no known title state, or this title carries none.
///
/// - Claude Code: "◐ " or "◑ " while busy, "✳ " otherwise.
/// - Codex: a braille spinner while busy, "[ ! ] Action Required" for an
///   answer. Its default title has the spinner first.
/// - Gemini CLI: "✦ " or "⏲ " while busy, "✋ " for an answer, "◇ " when ready.
pub fn title_state(agent: &str, title: &str) -> Option<TitleState> {
    let first = title.trim_start().chars().next()?;
    match agent {
        "claude" => match first {
            '\u{25d0}'..='\u{25d3}' => Some(TitleState::Busy),
            '\u{2733}' => Some(TitleState::Ready),
            _ => None,
        },
        "codex" => {
            if title.contains("Action Required") {
                Some(TitleState::Action)
            } else if title.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)) {
                Some(TitleState::Busy)
            } else {
                Some(TitleState::Ready)
            }
        }
        "gemini" => match first {
            '\u{2726}' | '\u{23f2}' => Some(TitleState::Busy),
            '\u{270b}' => Some(TitleState::Action),
            '\u{25c7}' => Some(TitleState::Ready),
            _ => None,
        },
        // "⚠ Action Required - ⠋ - Running: … - grok" at a prompt,
        // "⠋ - Thinking - <topic> - grok" while it works, "<topic> - grok" after.
        "grok" => {
            if title.contains("Action Required") {
                Some(TitleState::Action)
            } else if ('\u{2801}'..='\u{28ff}').contains(&first) {
                Some(TitleState::Busy)
            } else {
                Some(TitleState::Ready)
            }
        }
        _ => None,
    }
}
/// The agents whose busy line `Screen::busy_line` knows. For them, only that
/// line makes a turn: text they print without it (a recap, a status line) is not one.
pub fn has_busy_line(agent: &str) -> bool {
    matches!(
        agent,
        "claude" | "codex" | "opencode" | "gemini" | "copilot"
    )
}

/// Match interrupt hints after whitespace is removed and text is lowercased.
pub fn interrupt_hint(text: &str) -> bool { text.contains("tointerrupt") }

/// Match the short interrupt hint used by OpenCode.
pub fn esc_interrupt_hint(text: &str) -> bool { text.contains("escinterrupt") }

/// Whether a row starts with the Codex input marker.
pub fn codex_input(row: &str) -> bool { row.trim_start().starts_with('›') }

/// Whether a row is mostly box-drawing characters and spans half the width.
pub fn box_rule(row: &str, cols: usize) -> bool {
    let t = row.trim();
    let n = t.chars().count();
    n * 2 >= cols && t.chars().filter(|c| ('\u{2500}'..='\u{257f}').contains(c)).count() * 10 >= n * 8
}

#[cfg(test)]
mod tests {
    use super::{box_rule, codex_input, esc_interrupt_hint, interrupt_hint};

    #[test]
    fn shared_screen_patterns() {
        assert!(interrupt_hint("esctointerrupt"));
        assert!(esc_interrupt_hint("escinterrupt"));
        assert!(codex_input("  › text"));
        assert!(box_rule("────────", 16));
        assert!(!box_rule("text", 16));
    }
}

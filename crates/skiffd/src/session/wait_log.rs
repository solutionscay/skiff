//! The daemon log's two lines about a wait: why it began and what ended it.
//! They decide nothing. Text from the program or its screen is cleaned here.

use skiff_core::session::SessionInfo;

use super::attention::{live, PROMPT, REPORT, TITLE};
use super::Front;
use crate::log_text::{around, clean, clean_cut};
use crate::screen::PromptMatch;

/// Characters kept on each side of what a screen rule matched.
const ROW_SIDE: usize = 40;
/// Characters kept of a title or a report's message.
const MAX_TEXT: usize = 120;

/// `session ID waits (FRONT, PROGRAM): CAUSE [EVIDENCE] osc=STATE last_report=AGE`.
/// `osc` is the live report's state or `none`. `last_report` is Unix ms, 0 for
/// no report since this daemon began.
pub(super) fn waits_line(
    info: &SessionInfo,
    front: Front,
    cause: &str,
    prompt: Option<&PromptMatch>,
    title: Option<&str>,
    last_report: u64,
    now: u64,
) -> String {
    let program = clean(info.foreground_program.as_deref().unwrap_or("?"));
    let evidence = match cause {
        PROMPT => prompt_evidence(prompt),
        TITLE => title.or(info.title.as_deref()).map_or(String::new(), |t| format!(" title={:?}", clean_cut(t, MAX_TEXT))),
        REPORT => report_evidence(info),
        // A bell has no text.
        _ => String::new(),
    };
    let osc = info.program_status.as_ref().filter(|_| live(info)).map_or("none", |p| p.state.as_str());
    let age = if last_report == 0 { "none".to_string() } else { format!("{}s", now.saturating_sub(last_report) / 1000) };
    format!("session {} waits ({front:?}, {program}): {cause}{evidence} osc={} last_report={age}", info.id, clean(osc))
}

/// `session ID stops waiting after 4s: BY`. `since` is Unix ms, 0 when the
/// wait began before this daemon took the session over.
pub(super) fn stops_line(id: &str, since: u64, now: u64, by: &str) -> String {
    if since == 0 {
        format!("session {id} stops waiting: {by}")
    } else {
        format!("session {id} stops waiting after {}s: {by}", now.saturating_sub(since) / 1000)
    }
}

fn prompt_evidence(prompt: Option<&PromptMatch>) -> String {
    let Some(prompt) = prompt else { return String::new() };
    let row = prompt.row.as_deref().map(|row| around(&clean(row), prompt.needle, ROW_SIDE));
    match row.filter(|row| !row.is_empty()) {
        Some(row) => format!(" rule={} row={row:?}", prompt.rule),
        None => format!(" rule={}", prompt.rule),
    }
}

fn report_evidence(info: &SessionInfo) -> String {
    let Some(status) = &info.program_status else { return String::new() };
    let kind = status.kind.as_deref().map_or(String::new(), |k| format!(" kind={}", clean(k)));
    let msg = status.msg.as_deref().map_or(String::new(), |m| format!(" msg={:?}", clean_cut(m, MAX_TEXT)));
    format!("{kind}{msg}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::attention::BELL;

    fn info(status: serde_json::Value) -> SessionInfo {
        serde_json::from_value(serde_json::json!({
            "id": "3fa9", "label": "a", "role": "agent", "cwd": "/", "command": "claude", "args": [],
            "state": "waiting", "cols": 80, "rows": 24, "pid": null, "exit_code": null,
            "foreground_program": "claude", "title": "before token=old-secret", "program_status": status,
        }))
        .unwrap()
    }

    #[test]
    fn the_wait_line_holds_the_rule_and_only_its_row() {
        let prompt = PromptMatch {
            rule: "claude-esc-to-cancel",
            needle: "esctocancel",
            row: Some(format!("{} Esc to cancel · password=hunter2", "x ".repeat(40))),
        };
        let line = waits_line(&info(serde_json::Value::Null), Front::Agent("claude"), PROMPT, Some(&prompt), None, 0, 9000);
        assert!(line.starts_with("session 3fa9 waits (Agent(\"claude\"), claude): prompt rule=claude-esc-to-cancel row=\"…"), "{line}");
        assert!(line.ends_with("osc=none last_report=none"), "{line}");
        assert!(!line.contains("hunter2") && line.contains("password=[hidden]"), "{line}");
        assert!(line.len() < 200, "more than the row's window: {line}");
    }

    #[test]
    fn a_secret_in_a_title_or_a_message_is_hidden() {
        let none = info(serde_json::Value::Null);
        let line = waits_line(&none, Front::Agent("codex"), TITLE, None, Some("Action Required token=abc\x1b[31m"), 4000, 9000);
        assert!(line.ends_with("title title=\"Action Required token=[hidden]\" osc=none last_report=5s"), "{line}");
        let line = waits_line(&none, Front::Agent("codex"), TITLE, None, None, 0, 9000);
        assert!(line.contains("token=[hidden]") && !line.contains("old-secret"), "{line}");
        let blocked = info(serde_json::json!({ "state": "blocked", "kind": "auth", "msg": "Use sk-abcdefgh1234 please" }));
        let line = waits_line(&blocked, Front::Program, REPORT, None, None, 8000, 9000);
        assert!(line.ends_with("report kind=auth msg=\"Use [hidden] please\" osc=blocked last_report=1s"), "{line}");
    }

    #[test]
    fn the_bell_and_the_end_of_a_wait() {
        let line = waits_line(&info(serde_json::Value::Null), Front::Command, BELL, None, None, 0, 0);
        assert!(line.ends_with("bell osc=none last_report=none"), "{line}");
        assert_eq!(stops_line("3fa9", 1000, 5000, "screen"), "session 3fa9 stops waiting after 4s: screen");
        assert_eq!(stops_line("3fa9", 0, 5000, "input"), "session 3fa9 stops waiting: input");
    }
}

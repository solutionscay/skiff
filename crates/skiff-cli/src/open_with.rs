//! Open with: which command opens a diff or a file, and whether it runs in
//! the peek or in its own window. Edits `[open]` in projects.toml, as the
//! Settings dialog does.

use anyhow::{Result, bail};
use skiff_core::config::{self, OPEN_KEYS};

use crate::{OpenWithCmd, print};

#[derive(serde::Serialize)]
struct Row {
    key: &'static str,
    /// The command set, or `None` for the default.
    command: Option<String>,
    peek: bool,
}

fn rows() -> Result<Vec<Row>> {
    let o = config::load()?.open;
    Ok(OPEN_KEYS
        .into_iter()
        .map(|key| {
            let cmd = match key {
                "diff" => &o.diff,
                "text" => &o.text,
                "markdown" => &o.markdown,
                _ => &o.html,
            };
            let command = cmd.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(String::from);
            Row { key, command, peek: o.in_peek(key) }
        })
        .collect())
}

/// Lists each row with its command and where it runs.
pub fn ls(json: bool) -> Result<()> {
    let rows = rows()?;
    if json {
        return print::json(&rows);
    }
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let (command, place) = match (&r.command, r.key) {
                (Some(c), _) => (c.clone(), if r.peek { "peek" } else { "window" }),
                (None, "diff") => (format!("{} (default)", skiff_core::git::DEFAULT_DIFF), if r.peek { "peek" } else { "window" }),
                (None, _) => ("(system default app)".to_string(), "-"),
            };
            vec![r.key.to_string(), place.to_string(), command]
        })
        .collect();
    print::table(&["KEY", "RUNS IN", "COMMAND"], &table);
    Ok(())
}

pub fn run(cmd: OpenWithCmd) -> Result<()> {
    match cmd {
        OpenWithCmd::Ls { json } => ls(json),
        OpenWithCmd::Set { key, command, peek, window } => {
            let key = key.name();
            if command.is_none() && !peek && !window {
                bail!("give a command, --peek or --window");
            }
            if let Some(c) = &command {
                let c = c.trim();
                if c.is_empty() {
                    bail!("the command is empty; use `skiff open-with clear {key}`");
                }
                config::set_open(key, Some(c))?;
            }
            if peek || window {
                config::set_open_peek(key, peek)?;
            }
            Ok(())
        }
        OpenWithCmd::Clear { key } => {
            let key = key.name();
            config::set_open(key, None)?;
            config::set_open_peek(key, key == "diff")
        }
    }
}

//! Open with: which command opens a diff or a file. Edits `[open]` in
//! projects.toml, as the Settings dialog does.

use anyhow::{Result, bail};
use skiff_core::config::{self, OPEN_KEYS};

use crate::{OpenWithCmd, print};

#[derive(serde::Serialize)]
struct Row {
    key: &'static str,
    /// The command set, or `None` for the default.
    command: Option<String>,
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
            Row { key, command }
        })
        .collect())
}

/// Lists each row with its command.
pub fn ls(json: bool) -> Result<()> {
    let rows = rows()?;
    if json {
        return print::json(&rows);
    }
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let command = match (&r.command, r.key) {
                (Some(c), _) => c.clone(),
                (None, "diff") => format!("{} (default)", skiff_core::git::DEFAULT_DIFF),
                (None, _) => "(system default app)".to_string(),
            };
            vec![r.key.to_string(), command]
        })
        .collect();
    print::table(&["KEY", "COMMAND"], &table);
    Ok(())
}

pub fn run(cmd: OpenWithCmd) -> Result<()> {
    match cmd {
        OpenWithCmd::Ls { json } => ls(json),
        OpenWithCmd::Set { key, command } => {
            let key = key.name();
            let c = command.trim();
            if c.is_empty() {
                bail!("the command is empty; use `skiff open-with clear {key}`");
            }
            config::set_open(key, Some(c))
        }
        OpenWithCmd::Clear { key } => config::set_open(key.name(), None),
    }
}

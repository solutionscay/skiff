//! status, agents, projects.

use std::path::Path;

use anyhow::Result;
use skiff_client::Client;

use crate::print;

pub async fn status(socket: &Path) -> Result<()> {
    let c = crate::connect(socket).await?;
    let info = c.daemon_info().await?;
    let (version, protocol) = (info.version, info.protocol);
    let pid = info.pid.map(|p| p as i32).or(c.daemon_pid).map_or("?".to_string(), |p| p.to_string());
    println!("skiffd    {version}");
    let note = if protocol == skiff_core::PROTOCOL {
        String::new()
    } else {
        format!(" (this skiff speaks {})", skiff_core::PROTOCOL)
    };
    println!("protocol  {protocol}{note}");
    println!("pid       {pid}");
    println!("reload    {}", info.reload_state.map_or("no".to_string(), |v| format!("yes (handover {v})")));
    println!("socket    {}", socket.display());
    Ok(())
}

pub async fn agents(c: &Client, json: bool) -> Result<()> {
    let agents = c.list_agents().await?;
    if json {
        return print::json(&agents);
    }
    let yes = |b: bool| if b { "yes" } else { "no" }.to_string();
    let rows: Vec<Vec<String>> = agents
        .iter()
        .map(|a| vec![a.id.clone(), yes(a.installed), yes(a.enabled), a.command.clone()])
        .collect();
    print::table(&["ID", "INSTALLED", "ENABLED", "COMMAND"], &rows);
    Ok(())
}

/// From projects.toml, read here as the app reads it. Closed projects too.
pub fn projects(json: bool) -> Result<()> {
    let list = skiff_core::project::list()?;
    if json {
        return print::json(&list);
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|p| {
            let state = if p.closed {
                "closed".to_string()
            } else if let Some(e) = &p.error {
                format!("error: {e}")
            } else {
                format!("{} worktrees", p.worktrees.len())
            };
            vec![p.name.clone(), p.short.clone(), print::tilde(&p.path), p.agents.join(","), state]
        })
        .collect();
    print::table(&["NAME", "SHORT", "PATH", "AGENTS", "STATE"], &rows);
    Ok(())
}

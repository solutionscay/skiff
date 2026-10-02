//! Session commands: ls, new, send, rename, kill.

use std::{io::Read, path::PathBuf, time::Duration};

use anyhow::{bail, Context, Result};
use skiff_client::Client;
use skiff_core::{
    group::Group,
    session::{Role, SessionInfo, SessionSpec, SessionState},
};

use crate::{callsign::callsign, groups, layout::Side, print, resolve};

/// Sessions in a stable order: oldest first, then by id.
pub async fn sorted(c: &Client) -> Result<Vec<SessionInfo>> {
    let mut all = c.list_sessions().await?;
    all.sort_by(|a, b| a.started_at.cmp(&b.started_at).then_with(|| a.id.cmp(&b.id)));
    Ok(all)
}

pub async fn ls(c: &Client, json: bool) -> Result<()> {
    let all = sorted(c).await?;
    if json {
        return print::json(&all);
    }
    let rows: Vec<Vec<String>> = all
        .iter()
        .map(|s| {
            let mut cmd = s.command.clone();
            for a in &s.args {
                cmd.push(' ');
                cmd.push_str(a);
            }
            vec![
                print::short_id(&s.id).to_string(),
                resolve::display_name(s).to_string(),
                state(s.state).to_string(),
                role(s.role).to_string(),
                print::tilde(&s.cwd),
                cmd,
            ]
        })
        .collect();
    print::table(&["ID", "NAME", "STATE", "ROLE", "CWD", "COMMAND"], &rows);
    Ok(())
}

pub fn state(s: SessionState) -> &'static str {
    match s {
        SessionState::Working => "working",
        SessionState::Waiting => "waiting",
        SessionState::Idle => "idle",
        SessionState::Done => "done",
    }
}

pub fn role(r: Role) -> &'static str {
    match r {
        Role::Agent => "agent",
        Role::Task => "task",
        Role::Server => "server",
        Role::Shell => "shell",
    }
}

pub struct New {
    pub name: Option<String>,
    pub cwd: Option<PathBuf>,
    pub agent: Option<String>,
    pub role: Option<Role>,
    pub command: Vec<String>,
}

/// Builds the spec the way the app's `newSession` does, so the sidebar
/// shows a CLI session like one made from the + menu.
pub async fn spec(c: &Client, o: New) -> Result<SessionSpec> {
    let here = std::env::current_dir().context("current directory")?;
    let cwd = match o.cwd {
        Some(d) => here.join(d),
        None => here,
    };
    if !cwd.is_dir() {
        bail!("{} is not a directory", cwd.display());
    }
    // `SessionSpec::default()` has 0x0; serde's 120x40 default applies only
    // when decoding. Use the app's size for a pane it has not measured.
    let mut spec = SessionSpec {
        cwd: Some(cwd),
        cols: 120,
        rows: 40,
        ..SessionSpec::default()
    };
    let words: Vec<String> = if let Some(id) = &o.agent {
        let agents = c.list_agents().await?;
        let Some(a) = agents.iter().find(|a| &a.id == id) else {
            let ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
            bail!("no agent {id:?}; known: {}", ids.join(", "));
        };
        if !a.installed {
            bail!("agent {id:?} is not installed: {} is not on skiffd's PATH", a.command);
        }
        a.command.split_whitespace().map(String::from).collect()
    } else {
        o.command
    };
    spec.role = o.role.unwrap_or(if o.agent.is_some() { Role::Agent } else { Role::Shell });
    if let Some((first, rest)) = words.split_first() {
        spec.command = Some(first.clone());
        spec.args = rest.to_vec();
    }
    if spec.role == Role::Agent {
        // The app gives an agent a call sign as both label and name.
        let label = match o.name {
            Some(n) => n,
            None => {
                let live = c.list_sessions().await?;
                callsign(live.iter().map(|s| s.label.as_str()))
            }
        };
        spec.name = Some(label.clone());
        spec.label = label;
    } else {
        // Empty label: the daemon uses the program's file name.
        spec.label = o.agent.unwrap_or_default();
        spec.name = o.name;
    }
    Ok(spec)
}

/// Starts a session. With `group`, the group is found first, so a typo
/// starts nothing.
pub async fn new(c: &Client, mut o: New, group: Option<&str>, split: Option<Side>, json: bool) -> Result<()> {
    let target = match group {
        Some(arg) => {
            let all = c.list_groups().await?;
            let g = resolve::group(&all, arg)?.clone();
            groups::room(&g, &groups::Place { split, of: None })?;
            if o.cwd.is_none() {
                o.cwd = group_cwd(c, &g).await?;
            }
            Some((all, g))
        }
        None => None,
    };
    let spec = spec(c, o).await?;
    let info = c.create_session(spec).await?;
    if let Some((all, g)) = target {
        let live = c.list_sessions().await?;
        let place = groups::Place { split, of: None };
        if let Err(e) = groups::place(c, &all, g, &info.id, place, &live).await {
            eprintln!("skiff: started {} but did not place it", info.id);
            return Err(e);
        }
    }
    if json {
        return print::json(&info);
    }
    println!("{}", info.id);
    Ok(())
}

/// Where the app starts a session in `g`: the group's folder, else the
/// folder of its first session. A folder that is gone counts as none.
async fn group_cwd(c: &Client, g: &Group) -> Result<Option<PathBuf>> {
    let dir = match (&g.cwd, crate::layout::filled(&g.layout).first()) {
        (Some(d), _) => Some(PathBuf::from(d)),
        (None, Some(first)) => c.list_sessions().await?.into_iter().find(|s| &s.id == first).map(|s| s.cwd),
        (None, None) => None,
    };
    Ok(dir.filter(|d| d.is_dir()))
}

/// The pause before Enter, so a TUI reads the text as typing, not a paste
/// that ends in a newline.
const ENTER_DELAY: Duration = Duration::from_millis(80);

pub async fn send(c: &Client, arg: &str, text: &[String], enter: bool, raw: bool) -> Result<()> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    let mut data = if text == ["-"] {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).context("read stdin")?;
        buf
    } else {
        text.join(" ").into_bytes()
    };
    if !raw {
        data = enter_keys(&data);
    }
    if !data.is_empty() {
        c.write(&s.id, data).await?;
    }
    if enter {
        tokio::time::sleep(ENTER_DELAY).await;
        c.write(&s.id, b"\r".to_vec()).await?;
    }
    Ok(())
}

/// Newlines as the Enter key sends them: `\r`.
fn enter_keys(data: &[u8]) -> Vec<u8> {
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

pub async fn rename(c: &Client, arg: &str, name: &str) -> Result<()> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    c.rename_session(&s.id, name).await?;
    Ok(())
}

/// Clears the session's unread mark, as focusing its pane in the app does.
pub async fn seen(c: &Client, arg: &str) -> Result<()> {
    let all = c.list_sessions().await?;
    let s = resolve::session(&all, arg)?;
    c.seen(&s.id).await
}

/// Resolves every argument first, so a typo kills nothing.
pub async fn kill(c: &Client, args: &[String]) -> Result<()> {
    let all = c.list_sessions().await?;
    let mut ids: Vec<String> = Vec::new();
    for a in args {
        let id = resolve::session(&all, a)?.id.clone();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut failed = 0;
    for id in &ids {
        if let Err(e) = c.kill(id).await {
            eprintln!("skiff: kill {}: {e:#}", print::short_id(id));
            failed += 1;
        }
    }
    if failed > 0 {
        bail!("{failed} of {} sessions not killed", ids.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn newlines_become_enter() {
        assert_eq!(super::enter_keys(b"a\nb\r\nc\r"), b"a\rb\rc\r");
    }
}

//! `skiff`: drive skiffd from a shell. Sessions, groups, agents and projects.

use anyhow::{Context, Result};
use clap::Parser;
use skiff_client::Client;
use skiff_core::{group::SplitDir, session::Role};

mod args;
mod attach;
mod drops;
mod callsign;
mod clock;
mod daemon;
mod events;
mod groups;
mod info;
mod layout;
mod ledger;
mod open_with;
mod peek;
mod print;
mod projects;
mod resolve;
mod sessions;
mod skipper;
mod theme;
mod wait;
mod watch;

pub use args::{SkillCmd, SkipperCmd, TemplateCmd, ScheduleTarget, ScheduleGate, ScheduleSpec, ScheduleCmd, LedgerCmd};
use args::{Cli, Cmd, DaemonCmd, GroupCmd, ProjectsCmd, ThemeCmd, DirArg};

fn main() {
    // Rust ignores SIGPIPE, so `skiff ls | head` would panic on a closed pipe.
    // The default action ends the process quietly, like other CLI tools.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match rt.block_on(run(cli)) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("skiff: {e:#}");
            std::process::exit(1);
        }
    }
}

/// The exit code. Only `wait`, `events`, `send`, `kill`, and `new` or
/// `group new` from a Skipper, return other than 0.
async fn run(cli: Cli) -> Result<i32> {
    let socket = cli.socket.unwrap_or_else(skiff_core::socket::socket_path);
    // The state follows the socket: only the live one uses the person's files.
    skiff_core::state::set_socket(socket.clone());
    // Check before command-specific settings or ledger work. Local settings
    // and recovery commands remain available with a different daemon.
    if !matches!(&cli.cmd,
        Cmd::Status { .. } | Cmd::Daemon(_) | Cmd::Projects { cmd: None, .. }
        | Cmd::Projects { cmd: Some(ProjectsCmd::Add { .. }), .. }
        | Cmd::Theme { cmd: ThemeCmd::Ls { .. } | ThemeCmd::App { .. } | ThemeCmd::Project { .. } }
        | Cmd::OpenWith { .. }
        | Cmd::Skill(_)
        | Cmd::Skipper { cmd: Some(_), .. }
    ) {
        if let Ok(c) = Client::connect(&socket).await {
            check_daemon(c).await?;
        }
    }
    match cli.cmd {
        // Read locally: no daemon needed.
        Cmd::Projects { json, cmd: None } => return info::projects(json).map(|_| 0),
        Cmd::Projects { cmd: Some(ProjectsCmd::Add { path, name, fleet, json }), .. } => {
            return projects::add(&path, name, fleet, json).map(|_| 0)
        }
        Cmd::Projects { cmd: Some(ProjectsCmd::Remove { name, end_sessions }), .. } => {
            return projects::remove(&socket, &name, end_sessions).await.map(|_| 0)
        }
        Cmd::Theme { cmd: ThemeCmd::Ls { json } } => return theme::ls(json).map(|_| 0),
        Cmd::Theme { cmd: ThemeCmd::App { cmd } } => return theme::app(cmd).map(|_| 0),
        Cmd::Theme { cmd: ThemeCmd::Project { cmd } } => return theme::project(cmd).map(|_| 0),
        Cmd::OpenWith { cmd } => return open_with::run(cmd).map(|_| 0),
        Cmd::Ledger { person, cmd } => return ledger::run(&socket, cmd, person).await,
        Cmd::Schedule { person, cmd } => return ledger::schedule::run(&socket, cmd, person).await,
        Cmd::Status { json: true } => return Ok(info::status_json(&socket).await),
        Cmd::Status { json: false } => return info::status(&socket).await.map(|_| 0),
        Cmd::Daemon(DaemonCmd::Reload { binary }) => return daemon::reload(&socket, binary).await.map(|_| 0),
        Cmd::Daemon(DaemonCmd::Stop) => return daemon::stop(&socket).await.map(|_| 0),
        Cmd::Skill(SkillCmd::Path) => {
            println!("{}", skiff_client::skipper::skill_file()?.display());
            return Ok(0);
        }
        Cmd::Skipper { cmd: Some(SkipperCmd::Template(cmd)), .. } => return skipper::template(cmd).map(|_| 0),
        // Starts skiffd when it does not run.
        Cmd::Skipper { move_to: Some(to), dry_run, .. } => return skipper::move_to(&socket, &to, dry_run).await.map(|_| 0),
        Cmd::Skipper { project: Some(p), no_attach, .. } => return skipper::run_project(&socket, &p, no_attach).await.map(|_| 0),
        Cmd::Skipper { fleet: Some(f), no_attach, yes, .. } => return skipper::run_fleet(&socket, &f, no_attach, yes).await.map(|_| 0),
        Cmd::Skipper { no_attach, .. } => return skipper::run(&socket, no_attach).await.map(|_| 0),
        Cmd::Attach { session, host: Some(host) } => return attach::attach_remote(&host, &session).await.map(|_| 0),
        // It connects again after a daemon reload, so it holds no connection here.
        Cmd::Watch { skipper, no_notify } => {
            return watch::watch(&socket, watch::Options { skipper, notify: !no_notify }).await
        }
        _ => {}
    }
    let c = connect(&socket).await?;
    if let Cmd::Wait { sessions, target, any, timeout } = &cli.cmd {
        let t = wait::timeout_for(*timeout, wait::agent_caller());
        if timeout.is_some_and(|asked| t != Some(asked)) {
            eprintln!("skiff: --timeout capped at {}s for an agent", t.unwrap_or_default());
        }
        return wait::wait(&c, sessions, *target, *any, t).await;
    }
    if let Cmd::Events { sessions, json, timeout, until } = &cli.cmd {
        return events::events(&c, sessions, *json, *timeout, until).await;
    }
    if let Cmd::Send { session, text, enter, raw, shell } = &cli.cmd {
        let o = sessions::Send { text, enter: *enter, raw: *raw, shell: *shell, stdin: true };
        return sessions::send(&c, session, o).await;
    }
    if let Cmd::Kill { sessions: ids, force } = &cli.cmd {
        return sessions::kill(&c, &socket, ids, *force).await;
    }
    let unnamed = match &cli.cmd {
        Cmd::New { name: None, .. } => Some(false),
        Cmd::Group(GroupCmd::New { name: None, .. }) => Some(true),
        _ => None,
    };
    if let Some(group) = unnamed {
        if sessions::coordinator_calls(&c).await? {
            eprintln!("skiff: {}", sessions::name_rule(group));
            return Ok(sessions::UNNAMED);
        }
    }
    match cli.cmd {
        Cmd::Ls { json } => sessions::ls(&c, json).await,
        Cmd::New { name, cwd, agent, task_type, model, role, group, split, json, command } => {
            let opts = sessions::New { name, cwd, agent, task_type, model, role: role.map(Role::from), command };
            sessions::new(&c, opts, group.as_deref(), split, json).await
        }
        Cmd::Group(g) => group(&c, g).await,
        Cmd::Rename { session, name } => sessions::rename(&c, &session, &name).await,
        Cmd::Attach { session, .. } => attach::attach(&c, &session, None).await,
        Cmd::Peek { session, lines } => peek::peek(&c, &session, lines).await,
        Cmd::Seen { session } => sessions::seen(&c, &session).await,
        Cmd::Agents { json } => info::agents(&c, json).await,
        Cmd::Theme { cmd: ThemeCmd::Session { cmd } } => theme::session(&c, cmd).await,
        Cmd::Status { .. } | Cmd::Daemon(_) | Cmd::Projects { .. } | Cmd::Theme { .. } | Cmd::OpenWith { .. } | Cmd::Ledger { .. } | Cmd::Schedule { .. } | Cmd::Wait { .. } | Cmd::Events { .. } | Cmd::Watch { .. } | Cmd::Send { .. } | Cmd::Kill { .. } | Cmd::Skipper { .. } | Cmd::Skill(_) => {
            unreachable!()
        }
    }
    .map(|_| 0)
}

async fn group(c: &Client, cmd: GroupCmd) -> Result<()> {
    match cmd {
        GroupCmd::Ls { json } => groups::ls(c, json).await,
        GroupCmd::New { name, sessions, dir, cwd, json } => {
            let dir = dir.map(|d| match d {
                DirArg::Row => SplitDir::Row,
                DirArg::Col => SplitDir::Col,
            });
            groups::new(c, groups::NewGroup { name, sessions, dir, cwd }, json).await
        }
        GroupCmd::Add { group, session, split, of } => {
            groups::add(c, &group, &session, groups::Place { split, of }).await
        }
        GroupCmd::Rm { group, session } => groups::rm(c, &group, &session).await,
        GroupCmd::Rename { group, name } => groups::rename(c, &group, &name).await,
        GroupCmd::Delete { groups: ids } => groups::delete(c, &ids).await,
    }
}

/// Connects, with a plain message when no daemon listens.
pub async fn connect(socket: &std::path::Path) -> Result<Client> {
    if !socket.exists() {
        anyhow::bail!("skiffd is not running (no socket at {})", socket.display());
    }
    Client::connect(socket)
        .await
        .with_context(|| format!("skiffd is not running at {}", socket.display()))
}

/// Refuse a different protocol; warn once when only the daemon version differs.
async fn check_daemon(c: Client) -> Result<Client> {
    let info = c.daemon_info().await?;
    if info.protocol != skiff_core::PROTOCOL {
        let cli = c.daemon_pid
            .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
            .map(|p| p.with_file_name("skiff"));
        let matching = cli.map_or_else(|| "unknown".to_string(), |p| p.display().to_string());
        anyhow::bail!(
            "skiffd is {} (protocol {}), and this skiff was built for skiffd {} (protocol {}). Use the matching CLI: {}",
            info.version, info.protocol, skiff_core::VERSION, skiff_core::PROTOCOL, matching
        );
    }
    if info.version != skiff_core::VERSION {
        static WARNING: std::sync::Once = std::sync::Once::new();
        WARNING.call_once(|| eprintln!(
            "skiff: warning: skiffd is {}, and this skiff was built for skiffd {}",
            info.version, skiff_core::VERSION
        ));
    }
    Ok(c)
}

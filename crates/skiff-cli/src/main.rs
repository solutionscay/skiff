//! `skiff`: drive skiffd from a shell. Sessions, groups, agents and projects.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use skiff_client::Client;
use skiff_core::{group::SplitDir, session::Role};
use layout::Side;

mod attach;
mod callsign;
mod groups;
mod info;
mod layout;
mod peek;
mod print;
mod resolve;
mod sessions;
mod wait;

#[derive(Parser)]
#[command(
    name = "skiff",
    version,
    about = "Command-line client for skiffd",
    after_help = "A session is a full id, a unique id prefix, or the name the sidebar shows. A group is an id, prefix or name.\nInside a Skiff pane, `.` is this pane's session, or the group that holds it."
)]
struct Cli {
    /// The skiffd socket. Default: $SKIFF_SOCKET, else the daemon's own default.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the daemon's version, protocol, pid and socket.
    Status,
    /// List sessions.
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Start a session and print its id.
    New {
        /// The session's name.
        #[arg(long)]
        name: Option<String>,
        /// Working directory. Default: the current directory.
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
        /// Start this agent, as the app's + menu does.
        #[arg(long, value_name = "ID", conflicts_with = "command")]
        agent: Option<String>,
        /// Default: agent with --agent, else shell.
        #[arg(long, value_enum)]
        role: Option<RoleArg>,
        /// Put the new session into this group.
        #[arg(long, value_name = "GROUP")]
        group: Option<String>,
        /// With --group: the side of the focused pane it goes on.
        #[arg(long, value_enum, requires = "group")]
        split: Option<Side>,
        /// Print the new session as JSON.
        #[arg(long)]
        json: bool,
        /// Program and arguments, after `--`. Default: the login shell.
        #[arg(last = true, value_name = "CMD")]
        command: Vec<String>,
    },
    /// Type text into a session. `-` reads the text from stdin. Put text
    /// that starts with `-` after `--`.
    Send {
        session: String,
        #[arg(required = true)]
        text: Vec<String>,
        /// Press Enter after the text.
        #[arg(long)]
        enter: bool,
        /// Send the bytes as they are. Default: newlines become Enter.
        #[arg(long)]
        raw: bool,
    },
    /// Rename a session. An empty name clears it.
    Rename { session: String, name: String },
    /// Run a session in this terminal. Ctrl-\ detaches. The session
    /// takes this terminal's size, in the app too.
    Attach { session: String },
    /// Print a session's screen as plain text.
    Peek {
        session: String,
        /// The last N lines of history and screen. Default: the screen.
        #[arg(long, short = 'n', value_name = "N")]
        lines: Option<usize>,
    },
    /// Wait until sessions reach a state. Exit 0 when all do, 1 on timeout,
    /// 2 when one closes or exits first.
    Wait {
        #[arg(required = true)]
        sessions: Vec<String>,
        #[arg(long = "for", value_enum, value_name = "STATE")]
        target: wait::Target,
        /// Give up after this many seconds.
        #[arg(long, value_name = "SECS")]
        timeout: Option<f64>,
    },
    /// Mark a session seen, as focusing its pane does.
    Seen { session: String },
    /// Stop sessions.
    Kill {
        #[arg(required = true)]
        sessions: Vec<String>,
    },
    /// Named splits of sessions.
    #[command(subcommand, visible_alias = "g")]
    Group(GroupCmd),
    /// List the agents skiffd knows.
    Agents {
        #[arg(long)]
        json: bool,
    },
    /// List the projects in projects.toml.
    Projects {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum GroupCmd {
    /// List groups with their panes. `*` marks the focused pane.
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Make a group and print its id. With no sessions it holds one empty pane.
    New {
        /// Default: its agents' names, as the app names a group.
        #[arg(long)]
        name: Option<String>,
        sessions: Vec<String>,
        /// Put every pane in one line. Default: the app's shapes.
        #[arg(long, value_enum)]
        dir: Option<DirArg>,
        /// The group's folder, where its empty panes start sessions.
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Put a session into a group. It fills an empty pane, else splits the
    /// focused pane. The session leaves any other group.
    Add {
        group: String,
        session: String,
        /// The side of the target pane it goes on. Default: right.
        #[arg(long, value_enum)]
        split: Option<Side>,
        /// Split this pane instead of the focused one.
        #[arg(long, value_name = "SESSION")]
        of: Option<String>,
    },
    /// Take a session out of a group. The session keeps running.
    Rm { group: String, session: String },
    /// Rename a group.
    Rename { group: String, name: String },
    /// Delete groups. Their sessions keep running.
    Delete {
        #[arg(required = true)]
        groups: Vec<String>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum DirArg {
    /// Side by side.
    Row,
    /// Stacked.
    Col,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum RoleArg {
    Agent,
    Task,
    Server,
    Shell,
}

impl From<RoleArg> for Role {
    fn from(r: RoleArg) -> Self {
        match r {
            RoleArg::Agent => Role::Agent,
            RoleArg::Task => Role::Task,
            RoleArg::Server => Role::Server,
            RoleArg::Shell => Role::Shell,
        }
    }
}

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

/// The exit code. Only `wait` returns other than 0.
async fn run(cli: Cli) -> Result<i32> {
    let socket = cli.socket.unwrap_or_else(skiff_core::socket::socket_path);
    match cli.cmd {
        // Read locally: no daemon needed.
        Cmd::Projects { json } => return info::projects(json).map(|_| 0),
        Cmd::Status => return info::status(&socket).await.map(|_| 0),
        _ => {}
    }
    let c = connect(&socket).await?;
    if let Cmd::Wait { sessions, target, timeout } = &cli.cmd {
        return wait::wait(&c, sessions, *target, *timeout).await;
    }
    match cli.cmd {
        Cmd::Ls { json } => sessions::ls(&c, json).await,
        Cmd::New { name, cwd, agent, role, group, split, json, command } => {
            let opts = sessions::New { name, cwd, agent, role: role.map(Role::from), command };
            sessions::new(&c, opts, group.as_deref(), split, json).await
        }
        Cmd::Group(g) => group(&c, g).await,
        Cmd::Send { session, text, enter, raw } => sessions::send(&c, &session, &text, enter, raw).await,
        Cmd::Rename { session, name } => sessions::rename(&c, &session, &name).await,
        Cmd::Kill { sessions: ids } => sessions::kill(&c, &ids).await,
        Cmd::Attach { session } => attach::attach(&c, &session).await,
        Cmd::Peek { session, lines } => peek::peek(&c, &session, lines).await,
        Cmd::Seen { session } => sessions::seen(&c, &session).await,
        Cmd::Agents { json } => info::agents(&c, json).await,
        Cmd::Status | Cmd::Projects { .. } | Cmd::Wait { .. } => unreachable!(),
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

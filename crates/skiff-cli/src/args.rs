mod ledger;

pub use ledger::{LedgerCmd, ScheduleCmd, ScheduleGate, ScheduleSpec, ScheduleTarget};

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use skiff_core::session::Role;

use crate::{events, layout::Side, wait};

#[derive(Parser)]
#[command(
    name = "skiff",
    version,
    about = "Command-line client for one Skiff Host's session service",
    after_help = "The CLI controls the Host whose skiffd socket it reaches. The desktop selects local and SSH Hosts; this command stays on the Host that owns its socket. A session is a full id, a unique id prefix, or the name the sidebar shows. A group is an id, prefix or name.\nInside a Skiff pane, `.` is this pane's session, or the group that holds it."
)]
pub(super) struct Cli {
    /// The skiffd socket. Default: $SKIFF_SOCKET, else the daemon's own default.
    #[arg(long, global = true, value_name = "PATH")]
    pub(super) socket: Option<PathBuf>,
    #[command(subcommand)]
    pub(super) cmd: Cmd,
}

#[derive(Subcommand)]
pub enum SkillCmd {
    /// Print the path of the skill's SKILL.md, the copy that this skiff
    /// keeps. The file exists when this prints. Read it before any skiff
    /// command when your agent cannot load the skill.
    Path,
}

#[derive(Subcommand)]
pub enum SkipperCmd {
    /// See and apply the template of a Skipper instruction file.
    #[command(subcommand)]
    Template(TemplateCmd),
}

#[derive(Subcommand)]
#[command(after_help = "SCOPE is `agents` for AGENTS.md, or project:NAME or fleet:NAME for a SKIPPER.md. A bare NAME is a project or a fleet, and a NAME that is both is refused.")]
pub enum TemplateCmd {
    /// List each instruction file with its scope and status: none, template,
    /// edited or edited-template-changed.
    Ls,
    /// Show how a file differs from its template: `-` lines are the file,
    /// `+` lines the template. Changes nothing.
    Diff { scope: String },
    /// Replace a file with its template. The old file goes to <name>.bak,
    /// which replaces the last .bak. A file that is the same as the template
    /// stays as it is. Run this only when the person asks.
    Apply { scope: String },
}

#[derive(Subcommand)]
pub(super) enum Cmd {
    /// Show this skiff's version and path, and the daemon's version,
    /// protocol, pid and socket. Warns when another skiff comes first on
    /// PATH, or skiffd's version is not the one this skiff was built for.
    #[command(after_help = "It also prints one warning for each Skipper instruction file (AGENTS.md or a SKIPPER.md) that is edited and whose template changed, with the command that shows the change. It writes no file.\n--json prints one object and no warnings:\n  {\"skiff\":VERSION,\"protocol\":N,\"socket\":PATH,\"skiffd\":{\"version\":V,\"protocol\":N,\"pid\":PID,\"host_id\":ID},\"templates\":[{\"file\":PATH,\"scope\":SCOPE,\"command\":COMMAND}]}\nskiffd is null when no skiffd answers on the socket; the exit code is then 1. templates lists those same files.")]
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Manage the daemon itself.
    #[command(subcommand)]
    Daemon(DaemonCmd),
    /// List sessions.
    #[command(
        after_help = "STATE adds what a program reports about itself (OSC 7501): `waiting (permission)`, `waiting (question)` or `waiting (auth)` for what it waits for, `idle (done)` or `idle (failed)` for how its turn ended, `working 40%` for its progress, `working (no output)` after 5 minutes without output.\nWith --json, each session carries `program_status` while its program reports: {state, kind, msg, title, progress, app, stalled, ended, children}. `ended`: the program left, and a done or error stays until the pane is seen."
    )]
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Start a session and print its id.
    #[command(
        after_help = "--type starts a task type from Settings › Task types ([skiff_agent.tasks] in projects.toml): its agent, with its model flag. With no --name, the session takes the type's name, numbered when one runs (review, review 2).\nFrom a Skipper (Admiral, a project or a fleet Skipper), new and `group new` need --name: with none, they exit 2 and start nothing. Name a session for one issue `#N feature`, such as `#136 kanban and calendar`; any other session 2 to 4 words that say the task, without the project name; a group after the work it holds.\nWith no row for the type, it exits 1 and starts nothing.\n--model replaces the row's model for this session. With --agent ID, --model adds the agent's model flag to its command. A custom agent, opencode and hermes take their model in the command, so they refuse --model: it exits 1 and starts nothing.\nThe type's instruction file, types/TYPE.md in the agent folder, goes to the agent at start: claude --append-system-prompt-file, codex -c developer_instructions, grok --rules, opencode OPENCODE_CONFIG_CONTENT, kimi --agent-file. An empty file is none. gemini, hermes and custom agents cannot take one: --type exits 1 and starts nothing.\n`skiff agents` lists the types with the command each one starts."
    )]
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
        /// Start this task type's agent with its model and instruction file. Not with --agent.
        #[arg(long = "type", value_name = "TYPE", conflicts_with_all = ["agent", "command"])]
        task_type: Option<String>,
        /// With --type: this model in place of the type's model. With --agent: the agent's model flag. Not with a command.
        #[arg(long, value_name = "MODEL", conflicts_with = "command")]
        model: Option<String>,
        /// Default: agent with --agent or --type, else shell.
        #[arg(long, value_enum)]
        role: Option<RoleArg>,
        /// Put the new session into this group.
        #[arg(long, value_name = "GROUP")]
        group: Option<String>,
        /// With --group: the side of the focused pane it goes on. Default:
        /// the group's next template shape.
        #[arg(long, value_enum, requires = "group")]
        split: Option<Side>,
        /// Print the new session as JSON.
        #[arg(long)]
        json: bool,
        /// Program and arguments, after `--`. Default: the login shell.
        #[arg(last = true, value_name = "CMD")]
        command: Vec<String>,
    },
    /// Type text into an idle session. `-` reads the text from stdin. Put
    /// text that starts with `-` after `--`.
    #[command(
        after_help = "send checks the session before it types any bytes. To a known agent, it pastes the text, then confirms that the screen shows it. With --enter, it then presses Enter and confirms that a turn starts. A step not confirmed sets the pane to need you, with the step as its cause, and shows a desktop notification. Exit codes:\n  0  sent and confirmed\n  1  error\n  3  the session is waiting (approval prompt or bell). A person answers it in the pane. No flag overrides this.\n  4  the session is working, or a known agent shows its busy line. Skiff does not type into a busy session.\n  5  a shell or an unknown program is in front. Use --shell only to type a shell command. A program started with `skiff new -- CMD` or a custom agent shows its launch shell, so a send to it needs --shell.\n  6  the session is done\n  7  the session is your own ($SKIFF_SESSION)\n  8  send not confirmed: enter. Enter went to a known agent, but no turn started in 10 seconds. The text can be in its input box. Run `skiff peek` on the session.\n  9  the session took input after the check. Nothing was typed.\n  11 send not confirmed: paste. The pasted text did not show on the agent's screen in 2 seconds. Enter was not pressed: it could answer a menu or a prompt. Run `skiff peek` on the session."
    )]
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
        /// Permit an idle session with a shell or an unknown program in
        /// front. It does not override any other refusal.
        #[arg(long)]
        shell: bool,
    },
    /// Rename a session. An empty name clears it.
    Rename { session: String, name: String },
    /// Run a session in this terminal. Ctrl-\ detaches. The session
    /// takes this terminal's size, in the app too, and gets its own size
    /// back on detach. A quick second Ctrl-C is held back once.
    Attach {
        session: String,
        /// A remote Host from Settings › Hosts: its name, id or ssh destination.
        #[arg(long, value_name = "HOST")]
        host: Option<String>,
    },
    /// Attach to the Skiff agent (Admiral). Start it first if it does not
    /// run. Ctrl-\ detaches; the agent keeps running.
    #[command(
        after_help = "Reads [skiff_agent] in projects.toml: the folder, the name, the agent and whether it is on.\nWith no Skiff agent session, it sets up the folder (missing files come back, existing ones stay), starts the agent, and starts `skiff watch` for it. One watcher runs per daemon.\nA Skiff agent session that is a bare shell (after a restore, or after its agent quit) gets the agent again: its last conversation when Settings continues on restore, else a new one.\nWith no skiffd running, it starts one, as the app does.\nThe app and this command find the same session: skiffd marks it, and two starts at once give one.\nAttach gives the session this terminal's size, in the app too, until you detach. Then the pane gets its own size back.\n--move-to resolves a relative PATH against this shell's folder and stores the full path. The target must be new or empty. It refuses while ledger work is open or an agent session works. The folder moves whole: nothing stays at the old path. The Skipper sessions end first, and Admiral starts a new conversation in the new folder."
    )]
    #[command(args_conflicts_with_subcommands = true)]
    Skipper {
        #[command(subcommand)]
        cmd: Option<SkipperCmd>,
        /// Do not attach. Print the session id.
        #[arg(long)]
        no_attach: bool,
        /// Move the Skiff agent's folder to PATH, a new or empty folder, as
        /// Settings › Disk use › Folder › Move… does. It asks first.
        #[arg(long, value_name = "PATH", conflicts_with = "no_attach")]
        move_to: Option<std::path::PathBuf>,
        /// The project Skipper of PROJECT in place of Admiral: find it,
        /// or start it in the project's folder with SKIPPER.md as its first
        /// message.
        #[arg(long, value_name = "PROJECT", conflicts_with = "move_to")]
        project: Option<String>,
        /// The fleet Skipper of FLEET: find it, or start it in its folder
        /// with SKIPPER.md as its first message. When project Skippers run
        /// in the fleet, it names them and asks: the fleet Skipper takes
        /// their work over and they stop when idle.
        #[arg(long, value_name = "FLEET", conflicts_with_all = ["move_to", "project"])]
        fleet: Option<String>,
        /// With --fleet: take the project Skippers over without asking.
        #[arg(long, requires = "fleet")]
        yes: bool,
        /// With --move-to: run the checks and change nothing.
        #[arg(long, requires = "move_to")]
        dry_run: bool,
    },
    /// Find the skiff-cli skill, with no Skipper and no daemon.
    #[command(subcommand)]
    Skill(SkillCmd),
    /// Print a session's screen as plain text.
    Peek {
        session: String,
        /// The last N lines of history and screen. Default: the screen.
        #[arg(long, short = 'n', value_name = "N")]
        lines: Option<usize>,
    },
    /// Wait until sessions reach a state. Exit 0 when all do, 1 on timeout,
    /// 2 when one closes or exits first, 3 when one is waiting, 4 when a
    /// turn ended in an error.
    #[command(
        after_help = "Exit codes:\n  0  every session reached the state. With --any: one session reached it. Its id, name and state print on stdout.\n  1  timeout\n  2  a session closed or exited before it reached the state. With --any: every session did.\n  3  a session is waiting (approval prompt or bell). A person answers it in its pane.\n  4  with --for turn: a turn ended in an error the program reported.\n--for turn waits until each turn ends, a result to read: the program reported done or error (OSC 7501), or the pane's turn ended and is unread. A session whose turn ended and is not read counts at once.\nWith --for idle, done, any-not-working or turn, wait stops at once when any session is waiting. --for waiting is not changed.\nWith --for idle, a known agent that is quiet but shows its busy line (\"esc to interrupt\") is not idle yet.\nIn a Skiff pane ($SKIFF_SESSION), or with no terminal on stdout, the timeout defaults to 45 seconds and is at most 60.\nAfter exit 0, read the screen with `skiff peek`: idle does not prove that an agent finished."
    )]
    Wait {
        #[arg(required = true)]
        sessions: Vec<String>,
        #[arg(long = "for", value_enum, value_name = "STATE")]
        target: wait::Target,
        /// Stop when the first session reaches the state.
        #[arg(long)]
        any: bool,
        /// Give up after this many seconds. For an agent: default 45, at most 60.
        #[arg(long, value_name = "SECS")]
        timeout: Option<f64>,
    },
    /// Print each state change, exit, removal, program status and turn end
    /// as one line, for every session or for the named ones. Runs until Ctrl+C, --timeout or --until.
    #[command(
        after_help = "Lines: a state change prints the new state, an exit prints `exit` and the code, a closed session prints `removed`. No output or screen text prints.\nWith --json, each line is one JSON object:\n  {\"event\":\"state\",\"session\":ID,\"name\":NAME,\"state\":\"idle\"}\n  {\"event\":\"exit\",\"session\":ID,\"name\":NAME,\"code\":0}\n  {\"event\":\"removed\",\"session\":ID,\"name\":NAME}\n  {\"event\":\"status\",\"session\":ID,\"name\":NAME,\"program_status\":{\"state\":\"blocked\",\"kind\":\"permission\",\"msg\":\"…\"}}\n  {\"event\":\"turn\",\"session\":ID,\"name\":NAME}\nA `status` line prints when what the program reports (OSC 7501) changes its state, what it waits for or its message: `status blocked (permission): MSG`, `status done`, `status none` once it reports nothing. `program_status` is null then. A `turn` line (`turn ended`) prints when a turn ends: a result to read.\n--until KIND[,KIND] prints the first line of one of those kinds, from any of the sessions, then exits 0. Kinds: turn (a `turn` line), exit, removed, blocked (a `status blocked` line), waiting (a `waiting` state line). Only changes after the start count: a session that is already waiting or blocked prints nothing.\nskiff events exits when its stdout closes, also with no line to write: `| head -n1` or a read loop that stops ends it.\nExit codes:\n  0  --timeout ran out, every named session was removed, or stdout closed. With --until: a line matched, or stdout closed.\n  1  error. With --until: --timeout ran out first.\n  2  a name matches no session. With --until: every named session was removed first.\nRun it in the background so your own turn stays free. idle means quiet, not done. After any line, read the screen with `skiff peek`."
    )]
    Events {
        /// Default: every session, also the ones started later.
        sessions: Vec<String>,
        /// One JSON object for each line.
        #[arg(long)]
        json: bool,
        /// Stop after this many seconds.
        #[arg(long, value_name = "SECS")]
        timeout: Option<f64>,
        /// Print the first line of one of these kinds, then exit.
        #[arg(long, value_enum, value_delimiter = ',', value_name = "KIND")]
        until: Vec<events::Until>,
    },
    /// Follow the open tasks in the Skiff agent's ledger for that agent.
    /// Skiff starts it with the agent's session; it stops when that session
    /// ends. One runs per daemon.
    #[command(
        after_help = "A pane in an open ledger row (sent, pending, blocked) that:\n  waits (bell, prompt, title or report): the person gets a desktop notification, unless --no-notify. The agent gets nothing. The app starts it with --no-notify and sends its own.\n  ends a turn, goes idle after a prompt, exits or is gone: the agent gets one line, typed while it is idle and its input box is empty. Lines that wait for it are joined.\n  is idle with a pending row: the line names the oldest pending uid and its `skiff ledger send` command, once per idle stretch. The row is read again just before the line is typed, and the line is dropped if the task left pending.\n  restarted since the send: the agent gets one line that names the task as interrupted, once per run.\n  is gone with open rows: the line names their uids. A gone pane cannot be marked seen: reassign each task with `ledger update UID --pane ID --state pending`, or mark it done.\nskiffd types a line only if the person typed nothing in the agent's pane since the check. If no turn starts, the log says undelivered and Enter is not pressed again.\nA pane stays unread until the agent or the person marks it seen, so a new run tells it again, as it does a pending row.\nIt logs one line for each thing it does."
    )]
    Watch {
        /// The Skiff agent's session.
        skipper: String,
        /// Do not send desktop notifications.
        #[arg(long)]
        no_notify: bool,
    },
    /// Mark a session seen, as focusing its pane does.
    Seen { session: String },
    /// Stop sessions.
    #[command(
        after_help = "kill refuses a session that has an open ledger task (pending, sent, blocked or interrupted), from any Skipper. It names each such session with the task's uid and title, and stops none of the sessions. Close the task with `skiff ledger update UID --state done`, or use --force.\nWith a terminal, kill asks y/N for each Skipper among the sessions: Admiral, a project Skipper or a fleet Skipper. The line names it and counts its open ledger tasks. A no kills nothing. With no terminal, it does not ask.\nExit codes:\n  0  killed\n  1  error, or a no to a Skipper\n  10  a session has an open ledger task. Nothing was killed."
    )]
    Kill {
        #[arg(required = true)]
        sessions: Vec<String>,
        /// Kill even a session that has an open ledger task.
        #[arg(long)]
        force: bool,
    },
    /// Named splits of sessions.
    #[command(subcommand, visible_alias = "g")]
    Group(GroupCmd),
    /// List the agents skiffd knows.
    Agents {
        #[arg(long)]
        json: bool,
    },
    /// List the projects in projects.toml, or add or remove one.
    #[command(
        args_conflicts_with_subcommands = true,
        after_help = "add and remove write projects.toml, as Add project and Remove do in the app. The app shows the change within a second or two."
    )]
    Projects {
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        cmd: Option<ProjectsCmd>,
    },
    /// List and assign terminal color themes.
    #[command(after_help = "A session theme overrides its project's theme. A project theme overrides the app theme. Clear returns to the next setting.")]
    Theme {
        #[command(subcommand)]
        cmd: ThemeCmd,
    },
    /// Show and set the command that opens a diff or a file.
    #[command(
        name = "open-with",
        after_help = "Keys: diff, text, markdown, html, image. `{path}` in a command becomes the file; without it the file goes at the end. `{target}` in the diff command becomes what to compare.\nA terminal tool shows in the peek over the panes. An app with its own window just starts. A file with no command opens in the system's default app.\nOPENS IN is what selecting a file does: peek runs the command, app shows an Open button. An explicit Open always runs it. diff always opens in the peek. image always opens in an app.\nChanges apply to the next file or diff the app opens."
    )]
    OpenWith {
        #[command(subcommand)]
        cmd: OpenWithCmd,
    },
    /// Schedules: commands and agent steps that skiffd runs at set times,
    /// also while the app is closed. Each run is a ledger task.
    #[command(after_help = "A schedule is when + an optional command + an optional agent step.\nWhen: --every day --at 02:00, --every weekday --at 08:00, --every week --on mon,thu --at 09:00, --every month --day 1 --at 06:00, --every 30m --between 09:00-18:00 --on mon-fri, or --cron \"0 9 1,15 * *\". Times are this computer's local time.\nThe command runs in the project folder with a time limit (--timeout, default 600 s). Its output is the run's result. With --gate on-fail or on-output, the agent step runs only when the command failed or printed something.\nThe agent step: --to type (a new session of a task type), --to pane (a send to a named session; a busy session keeps the run pending until it is idle) or --to skipper (a hand-off to Admiral, a fleet Skipper or a project Skipper; a project in a fleet with a running fleet Skipper goes to the fleet Skipper).\nA schedule has at most one open run: a due time while a run is open is skipped. A run missed while skiffd was not running is not caught up.")]
    Schedule {
        /// From a shell outside Skiff: I am a person, not an agent.
        #[arg(long, global = true)]
        person: bool,
        #[command(subcommand)]
        cmd: ScheduleCmd,
    },
    /// The Skiff agent's task ledger: ledger.db in its folder.
    #[command(after_help = "States: sent, pending (not delivered: the pane was working), blocked (the pane needs the user), done.\nls and show print interrupted for a sent or blocked task whose pane restarted since the send: a restore runs a new shell under the old pane id. They print gone for an open task whose pane no longer exists. A task sent on another daemon shows elsewhere in LIVE and its socket: it is never gone from here, and send refuses it. A plan with tasks shows done when every task is done, else sent.\nID is the row number or the 8-character uid that add prints. Neither is ever reused.\nWork folders: add and handoff make one folder for each row, <uid>-<slug>/, in results/ next to the ledger, or in projects/<project>/results/ for a project Skipper's rows and a hand-off to it, or in fleets/<fleet>/results/ for a fleet Skipper's. A task in a plan, or under a hand-off, gets its folder in that row's folder. The result is result.md in the folder (plan.md for a plan). add prints the folder and the path, and makes no file in the folder. dir prints the folder. A row from before work folders keeps its result path and has no folder. rm and prune keep the files; with --flush they flush the work folders by the Disk use rule.\nWork: a plan with its tasks, or a task with no plan. Work is working while an agent is on one of its tasks (the task is sent, and its pane works or ended its turn), open otherwise, and finished once closed. close stamps it on the person's word; reopen clears the stamp. Skiff also closes work by itself when every task in it is closed. A task is closed when it is done and reported, or done and flushed by a person (a person's flush is their word; auto flush takes only closed work), or when its session closed: the session no longer exists, or its program and shell exited. An idle session is not closed. A session closed in any state sets an open task done, unobserved, on the next ledger ls, ledger show or watcher step. An interrupted pane, a task on another daemon and an unanswered session list close nothing. A skiffd that failed to load its workspace, or that does not report its load, also closes nothing until it restarts healthy or updates: these commands say so on stderr and show LIVE as ?. A saved session that cannot start at a restore is closed: its task closes. If no saved session starts, that is a failed load. A hand-off with tasks stays open until its Skipper reports it or its Skipper's session closes. Then it follows its tasks. A hand-off with no tasks follows its Skipper's session. A task that was not reported and did not succeed goes to Admiral in one [watch] line. stale lists what stays open, and closes nothing. A task added or moved under closed work, or set from done to another state, reopens it.\nReported: report stamps a done task once the person was told. An update to a state other than done clears the stamp. A flush needs no report. ls --unreported lists the done tasks not yet reported and not flushed.\nThe ledger is ~/skiff-agent/ledger.db, or ledger.db in the [skiff_agent] folder override. On a socket other than the live one, it is in the socket's private state. SKIFF_LEDGER overrides the path.\nOwners: each row is Admiral's (grand), a project Skipper's (its project) or a fleet Skipper's (fleet:<fleet>). add, send, update, report, close, reopen, prune and rm refuse unless the caller owns the row: the Skiff agent session that owns the ledger for Admiral's rows, a project or fleet Skipper for its own. A project or fleet Skipper may also set the state and result of the hand-off it was given. A fleet Skipper's add and update --pane refuse a pane outside its fleet's projects. A Skipper a fleet Skipper took over is refused. skiff skipper and the agent's watcher record the Skipper's session; a new Skiff agent session on the owner's daemon, or on the live daemon, takes it over. The refusal names the owner. A shell pane is a person and may change rows. A shell outside Skiff has no SKIFF_SESSION, and an agent there looks the same, so it is refused too: a person there passes --person. ls, show and stale work from anywhere.\nA ledger from before version 3 moves aside to ledger.db.old on first use, and a new one starts empty.")]
    Ledger {
        /// From a shell outside Skiff: I am a person, not an agent. Without
        /// it, add, send, update, report, close, reopen, prune and rm refuse
        /// there.
        #[arg(long, global = true)]
        person: bool,
        #[command(subcommand)]
        cmd: LedgerCmd,
    },
}

#[derive(Subcommand)]
pub(super) enum DaemonCmd {
    /// Move skiffd onto a new binary and keep every session, with its
    /// process, screen and scrollback. Checks afterwards that each one survived.
    Reload {
        /// The skiffd to run. Default: the running daemon's executable as it
        /// is on disk now, for example after a rebuild.
        #[arg(long, value_name = "PATH")]
        binary: Option<PathBuf>,
    },
    /// Stop skiffd. It saves the layout and ends every session. From a
    /// pane, this is the way to stop it: skiffd ignores SIGTERM and SIGINT
    /// from its own sessions.
    Stop,
}

#[derive(Subcommand)]
pub(super) enum GroupCmd {
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
    /// Put a session into a group. It fills an empty pane, else the group
    /// takes its next template shape. The session leaves any other group.
    Add {
        group: String,
        session: String,
        /// Split the target pane in half, on this side. Default: the next
        /// template shape.
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

#[derive(Subcommand)]
pub(super) enum OpenWithCmd {
    /// List each key and its command.
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Set a key's command, where it opens, or both.
    #[command(override_usage = "skiff open-with set <KEY> [COMMAND] [--peek | --app]", group = clap::ArgGroup::new("what").required(true).multiple(true).args(["command", "peek", "app"]))]
    Set {
        #[arg(value_enum)]
        key: OpenKey,
        /// The command, quoted as one argument.
        command: Option<String>,
        /// Selecting the file runs the command in the peek.
        #[arg(long, conflicts_with = "app")]
        peek: bool,
        /// Selecting the file shows an Open button.
        #[arg(long)]
        app: bool,
    },
    /// Clear a key. The default applies.
    Clear {
        #[arg(value_enum)]
        key: OpenKey,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(super) enum OpenKey {
    Diff,
    Text,
    Markdown,
    Html,
    Image,
}

impl OpenKey {
    pub(super) fn name(self) -> &'static str {
        match self {
            OpenKey::Diff => "diff",
            OpenKey::Text => "text",
            OpenKey::Markdown => "markdown",
            OpenKey::Html => "html",
            OpenKey::Image => "image",
        }
    }
}

#[derive(Subcommand)]
pub(super) enum ProjectsCmd {
    /// Add the git repository that holds PATH, or the folder itself when it
    /// is in no repository. A folder in a linked worktree adds the main
    /// worktree. Defaults: the folder's name, a free color, a theme of its
    /// own. A closed project for the folder opens again with its own
    /// settings; --name and --fleet do not apply to it.
    Add {
        path: PathBuf,
        /// The project's name. Default: the folder's name.
        #[arg(long)]
        name: Option<String>,
        /// Put the project in this fleet on the rail.
        #[arg(long)]
        fleet: Option<String>,
        /// Print the project as one item of `skiff projects --json`.
        #[arg(long)]
        json: bool,
    },
    /// Remove the project and its settings. The folder and its worktrees
    /// stay on disk. With running sessions in the project, it refuses and
    /// names them.
    #[command(visible_alias = "rm")]
    Remove {
        name: String,
        /// End the project's sessions first, then remove it.
        #[arg(long)]
        end_sessions: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum ThemeCmd {
    /// List available themes.
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Set or clear the app theme.
    App {
        #[command(subcommand)]
        cmd: AppThemeCmd,
    },
    /// Set or clear a project's theme.
    Project {
        #[command(subcommand)]
        cmd: ProjectThemeCmd,
    },
    /// Set or clear a session's theme override.
    #[command(visible_alias = "terminal")]
    Session {
        #[command(subcommand)]
        cmd: SessionThemeCmd,
    },
}

#[derive(Subcommand)]
pub(super) enum AppThemeCmd {
    /// Set the app theme.
    Set { theme: String },
    /// Clear the app theme. The default applies.
    Clear,
}

#[derive(Subcommand)]
pub(super) enum ProjectThemeCmd {
    /// Set the project theme.
    Set { project: String, theme: String },
    /// Clear the project theme. The app theme applies.
    Clear { project: String },
}

#[derive(Subcommand)]
pub(super) enum SessionThemeCmd {
    /// Set the session theme override.
    Set { session: String, theme: String },
    /// Clear the session theme override. The project or app theme applies.
    Clear { session: String },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(super) enum DirArg {
    /// Side by side.
    Row,
    /// Stacked.
    Col,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(super) enum RoleArg {
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

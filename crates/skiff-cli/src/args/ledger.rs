//! Schedule and ledger command arguments.
use clap::Subcommand;

/// The agent step's target.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ScheduleTarget {
    /// A new session of a task type in a project. Each run starts fresh.
    Type,
    /// A send to a named session. A busy session keeps the run pending.
    Pane,
    /// A hand-off to a Skipper: Admiral (grand) or a project's Skipper.
    Skipper,
    /// No agent step: command only.
    None,
}

/// When the agent step runs after the command.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ScheduleGate {
    Always,
    OnFail,
    OnOutput,
}

/// What `schedule add` and `schedule edit` set. On edit, an empty value
/// clears a text field.
#[derive(clap::Args, Default)]
pub struct ScheduleSpec {
    /// day, weekday, week, month, or an interval: 5m, 10m, 15m, 20m, 30m,
    /// 1h, 2h, 3h, 4h, 6h, 8h, 12h.
    #[arg(long, value_name = "WHEN", conflicts_with = "cron")]
    pub every: Option<String>,
    /// The time, HH:MM: with day, weekday, week and month.
    #[arg(long, value_name = "HH:MM")]
    pub at: Option<String>,
    /// The days: mon,thu or mon-fri. With week, and with an interval
    /// (default: every day).
    #[arg(long, value_name = "DAYS")]
    pub on: Option<String>,
    /// The day of the month, 1 to 28: with month.
    #[arg(long, value_name = "N")]
    pub day: Option<u8>,
    /// Whole hours, 09:00-18:00: with an interval. No run starts at the end
    /// hour. Default: all day.
    #[arg(long, value_name = "HH:MM-HH:MM")]
    pub between: Option<String>,
    /// A 5-field cron line in this computer's local time, or @hourly,
    /// @daily, @weekly, @monthly.
    #[arg(long, value_name = "CRON")]
    pub cron: Option<String>,
    /// A command skiffd runs in the project folder. Its output is the run's result.
    #[arg(long, value_name = "CMD")]
    pub command: Option<String>,
    /// The command's time limit in seconds. Default: 600.
    #[arg(long, value_name = "SECS")]
    pub timeout: Option<i64>,
    /// With a command and an agent step: when the agent step runs. Default: always.
    #[arg(long, value_enum)]
    pub gate: Option<ScheduleGate>,
    /// The agent step's target.
    #[arg(long = "to", value_enum)]
    pub to: Option<ScheduleTarget>,
    /// With --to type: the task type.
    #[arg(long = "type", value_name = "TYPE")]
    pub task_type: Option<String>,
    /// The project: the command's folder, and the new session's for --to type.
    #[arg(long, value_name = "PROJECT")]
    pub project: Option<String>,
    /// With --to pane: the session's name.
    #[arg(long, value_name = "NAME")]
    pub pane: Option<String>,
    /// With --to skipper: Admiral, a project or fleet:NAME. A name that is both a
    /// project and a fleet needs project:NAME or fleet:NAME. Default: Admiral.
    #[arg(long, value_name = "SCOPE")]
    pub skipper: Option<String>,
    /// The prompt for the agent step. `-` reads it from stdin.
    #[arg(long)]
    pub text: Option<String>,
}

#[derive(Subcommand)]
pub enum ScheduleCmd {
    /// Add a schedule. Give the time with --every and its words, or with --cron.
    Add {
        name: String,
        #[command(flatten)]
        spec: ScheduleSpec,
    },
    /// Change a schedule. Only the flags given change.
    Edit {
        name: String,
        /// A new name.
        #[arg(long = "name", value_name = "NAME")]
        rename: Option<String>,
        #[command(flatten)]
        spec: ScheduleSpec,
    },
    /// List the schedules: when, target, next run and the last run.
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Show a schedule and its runs, newest first.
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a schedule. Its runs stay in the ledger.
    Rm { name: String },
    /// Stop a schedule's runs until resume.
    Pause { name: String },
    /// Start a paused schedule's runs again from the next due time.
    Resume { name: String },
    /// Start a run now. Refused while a run is open.
    RunNow { name: String },
    /// skiffd runs this each half minute: it starts the due runs and sends
    /// the pending agent steps again.
    #[command(hide = true)]
    Tick,
    /// One run, started by tick or run-now.
    #[command(hide = true)]
    Fire { run: String },
}

#[derive(Subcommand)]
pub enum LedgerCmd {
    /// Add a task, or a plan with --plan, and make its work folder. Prints
    /// its id, uid, folder and result path, and the plan's uid for a task in
    /// a plan.
    Add {
        /// The pane the task went to: an id, a unique prefix or a name. It
        /// must exist. A plan has no pane.
        #[arg(long, required_unless_present = "plan", conflicts_with = "plan")]
        pane: Option<String>,
        /// The task text. `-` reads it from stdin.
        #[arg(long)]
        task: String,
        /// A short title for the app's Ledger panel: 3 to 6 words. Default:
        /// the panel takes one from the task text.
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        /// A plan: a row with no pane that groups tasks under it.
        #[arg(long)]
        plan: bool,
        /// The work this task joins: a plan or any task in it, or a task with
        /// no plan, which a new plan then wraps. Its id or uid. Closed work
        /// opens again.
        #[arg(long, value_name = "ID", conflicts_with = "plan")]
        parent: Option<String>,
        /// Default: the project that holds the pane's folder.
        #[arg(long)]
        project: Option<String>,
        /// Default: pending for a task, sent for a plan.
        #[arg(long)]
        state: Option<String>,
        /// Where the child writes its result. Default: result.md in the row's work folder.
        #[arg(long, value_name = "PATH")]
        result: Option<String>,
        /// Free text that names the work elsewhere: a URL, a ticket key,
        /// anything. Skiff stores it and never reads it.
        #[arg(long = "ref", value_name = "TEXT")]
        reference: Option<String>,
        /// The task type the task went out as, from `skiff agents`. Skiff
        /// stores it and never acts on it.
        #[arg(long = "type", value_name = "TYPE")]
        task_type: Option<String>,
        /// The evidence the task should leave: result (its result file) or
        /// commit (a commit, named at the end with update --commit).
        /// `update --state done` checks it.
        #[arg(long, value_name = "EVIDENCE")]
        expect: Option<String>,
        /// Add the task as pending and send it at once, as `ledger send` does.
        #[arg(long, conflicts_with_all = ["plan", "state"])]
        send: bool,
        /// With --send: press Enter after the text.
        #[arg(long, requires = "send")]
        enter: bool,
        /// With --send: as `send --shell`.
        #[arg(long, requires = "send")]
        shell: bool,
        /// With --send: close the sent or blocked task that holds the pane
        /// as done, outcome interrupted, and reported.
        #[arg(long, requires = "send")]
        replace: bool,
    },
    /// Hand a task to a project's or a fleet's Skipper: add a hand-off row,
    /// start that Skipper if none runs, and send it the task. Exits with
    /// `send`'s code.
    #[command(
        after_help = "SCOPE is a project or a fleet, by its name in `skiff projects`. A name that is both is refused: use project:NAME or fleet:NAME. Both spellings always work.\nThe hand-off row is Admiral's: its pane is the project or fleet Skipper, and its result path is in that Skipper's results folder. That Skipper adds its own tasks under it with --parent UID, and reports it with `skiff ledger update UID --state done --result PATH`. A project or fleet Skipper cannot hand off.\nA project in a fleet whose Skipper runs is refused: hand off to the fleet. A fleet with project Skippers and no fleet Skipper is refused: the person starts the fleet Skipper, which takes them over.\nA new Skipper gets SKIPPER.md as its first message, with the task after it. A busy one keeps the row pending: `skiff watch` names it when that Skipper is idle.\n--parent may name a hand-off for a follow-up: the new hand-off joins its plan, or a new plan of Admiral's wraps both.\nSkiff writes brief.md in the hand-off's folder: the state of the rows the hand-off cites or joins, and the open work of the scope, at most 8 KiB. The message names it when the brief differs from the last one this Skipper got, and always for a new Skipper. `skiff ledger brief UID` prints it as of now."
    )]
    Handoff {
        /// The project or the fleet: NAME, project:NAME or fleet:NAME.
        #[arg(value_name = "SCOPE")]
        project: String,
        /// The task text. `-` reads it from stdin.
        task: String,
        /// A short title for the app's Ledger panel: 3 to 6 words.
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        /// The work it joins: a plan of yours, any task in it, or an
        /// earlier hand-off that this one follows up.
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        /// The evidence the task should leave: result (its result file) or
        /// commit (a commit, named at the end with update --commit).
        /// `update --state done` checks it.
        #[arg(long, value_name = "EVIDENCE")]
        expect: Option<String>,
        /// Write no brief.md, and name none in the message.
        #[arg(long)]
        no_brief: bool,
    },
    /// Send a pending or interrupted task to its pane, then mark it sent.
    /// Exits with `send`'s code.
    #[command(
        after_help = "Only a pending or interrupted task with a live pane is sent. Else it exits 1 and sends nothing.\nThe text defaults to the task and its result path. `-` reads the text from stdin.\nIt runs every `send` check. On exit 0 the task is sent. On exit 8 (no turn started) it is blocked: the text can be in the input box. On a refusal it keeps its state, and `skiff watch` names it again when the pane is idle.\nA pane works on one task. A send to a pane that holds another of your tasks, sent or blocked, exits 1, names that task and sends nothing. Set that task done first, or pass --replace: it closes as done, outcome interrupted, and reported, once the new task is sent. `add --send` refuses the same way, and adds no row. A hand-off and a schedule step hold no pane."
    )]
    Send {
        id: String,
        /// Default: the task and its result path.
        text: Vec<String>,
        /// Press Enter after the text.
        #[arg(long)]
        enter: bool,
        /// As `send --shell`.
        #[arg(long)]
        shell: bool,
        /// Close the sent or blocked task that holds the pane as done,
        /// outcome interrupted, and reported.
        #[arg(long)]
        replace: bool,
    },
    /// Change a task. Only the given fields change. A new pane, or the state
    /// sent, records the pane's current generation.
    Update {
        id: String,
        #[arg(long)]
        state: Option<String>,
        #[arg(long, value_name = "PATH")]
        result: Option<String>,
        #[arg(long)]
        task: Option<String>,
        /// The short title for the app's Ledger panel.
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        project: Option<String>,
        /// Move the task into this work, as add --parent does.
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        #[arg(long = "ref", value_name = "TEXT")]
        reference: Option<String>,
        #[arg(long = "type", value_name = "TYPE")]
        task_type: Option<String>,
        /// How the task ended: succeeded, failed, interrupted or unobserved.
        /// An outcome sets the task done. Default with --state done:
        /// succeeded, or unobserved when its expected evidence is missing.
        #[arg(long)]
        outcome: Option<String>,
        /// The evidence the task should leave: result (its result file) or
        /// commit (a commit, named at the end with update --commit).
        /// `update --state done` checks it.
        #[arg(long, value_name = "EVIDENCE")]
        expect: Option<String>,
        /// The commit the task leaves, for --expect commit.
        #[arg(long, value_name = "REF")]
        commit: Option<String>,
    },
    /// List the work that is not closed: working first, then open, oldest
    /// first, each plan's tasks under it. WORK names the group. LIVE says if
    /// the pane still exists. An open task whose pane is gone is set done
    /// first, and shows `gone`.
    #[command(after_help = "--state filters inside the list. --json adds bucket (working, open or finished), closed_at and gone_at to each row.")]
    Ls {
        /// sent, pending, blocked, done, interrupted or gone.
        #[arg(long)]
        state: Option<String>,
        /// Every row, closed work included.
        #[arg(long, conflicts_with_all = ["closed", "unreported"])]
        all: bool,
        /// Only closed work.
        #[arg(long, conflicts_with = "unreported")]
        closed: bool,
        /// Only done tasks not yet reported, under their plans.
        #[arg(long)]
        unreported: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show one task, and each attempt to deliver its text: the session and
    /// its generation, the payload's bytes and SHA-256, and what came of it.
    #[command(
        after_help = "Each send by ledger send, add --send and handoff records an attempt before the text goes in, then its evidence: turn started, text accepted, no turn (exit 8), refused (with send's exit code), or uncertain. Uncertain: the send failed after it could have typed, or the process ended before the evidence. A child's report that skiff watch types into its parent is recorded on its hand-off the same way. An attempt is kept, never retried or guessed. --json adds deliveries."
    )]
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Mark done tasks as told to the person.
    #[command(
        after_help = "A task that is not done is refused, and nothing changes. A reported task keeps its first stamp.\nA plan's ID reports its done tasks and leaves its open ones. A hand-off's ID also stamps the done tasks under it, whoever owns them, and names each that failed, was interrupted or is unobserved on stderr."
    )]
    Report {
        #[arg(required_unless_present = "all", value_name = "ID")]
        ids: Vec<String>,
        /// Every done task not yet reported.
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
    /// List what stays open, and why. It closes nothing and writes nothing.
    #[command(after_help = "It lists each row that holds open work with no change for the given age: a done row not reported and not flushed with a live session, a blocked, sent or pending task, a hand-off with no tasks, and a hand-off with tasks that its Skipper did not report. It also lists work that ledger reopen holds. WHY says the reason, the session's name and its state: close an idle session to close its task. A work item with a working session is left out. The session of a hand-off with tasks is the Skipper: it does not leave the work out.\n--json prints work, uid, kind, state, outcome, owner, project, pane, session, changed and why.")]
    Stale {
        /// List work with no change for this long: 7d, 12h, 30m.
        #[arg(long, value_name = "AGE", default_value = "7d")]
        older_than: String,
        #[arg(long)]
        json: bool,
    },
    /// Close work on the person's word: a plan, or a task with no plan. It
    /// stamps the done tasks not yet reported too.
    #[command(after_help = "Work with a task that is not done is refused, and nothing changes: set the task done or remove it first. A task in a plan is refused: close its plan. Closed work keeps its first stamp.")]
    Close {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
    },
    /// Open closed work again: a plan, or a task with no plan.
    Reopen {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
    },
    /// Remove closed work, with its tasks. Open work stays. Without --yes, it
    /// prints the rows and the size of their work folders, and changes nothing.
    #[command(after_help = "The work folders stay unless you add --flush. --flush removes each folder that Settings › Disk use would flush: the work is done, its pane is not working, nothing in it changed in the last 15 minutes, and no git checkout in it has changes or unpushed commits. A folder in use stays, and the output says why.")]
    Prune {
        /// Remove work closed longer ago than this: 7d, 12h, 30m.
        #[arg(long, value_name = "AGE", default_value = "7d")]
        older_than: String,
        #[arg(long)]
        yes: bool,
        /// Also flush the rows' work folders, by the Disk use rule.
        #[arg(long, requires = "yes")]
        flush: bool,
    },
    /// Take a row out of the ledger now. Its work folder and result file stay
    /// unless you add --flush. For finished work, use close, and later prune.
    Rm {
        id: String,
        /// Also flush the row's work folder, by the Disk use rule.
        #[arg(long)]
        flush: bool,
    },
    /// Print a hand-off's brief as of now: the state of the rows it cites or
    /// joins, and the open work of its scope. It writes nothing.
    #[command(after_help = "Exit 0: printed, or the hand-off has no cited work and no open work. Exit 1: no such row, or the row is not a hand-off.\n--json prints uid, scope, bytes, items and left_out.")]
    Brief {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Print a row's work folder: everything for the task goes in it.
    #[command(after_help = "A row from before work folders has none: it exits 1 and names its result path.")]
    Dir { id: String },
}

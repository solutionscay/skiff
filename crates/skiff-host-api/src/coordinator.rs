//! M6 data plane: Skipper's ledger as the Ledger panel reads it (`ledger@1`),
//! Skipper's settings and task types (`skipper@1`), their changes and the
//! folder change (`skipper_config@1`), and Skipper's session and watcher on
//! the Host (`skipper_run@1`). Served by `skiff-host` from the Host's own
//! `ledger.db` and config: `skiff ledger` on the Host stays the ledger's only
//! writer. A result is read by its row's
//! uid, never by a path the client names, and comes back as bounded bytes or
//! text. Paths here are the Host's, for display only.
use serde::{Deserialize, Serialize};

/// One ledger row as the panel takes it: a task, or a plan with the state
/// its tasks give it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostLedgerRow {
    pub id: i64,
    /// 8 hex characters; names the row in `ledger_result`.
    pub uid: String,
    /// `task`; `plan`: a row with no pane that groups tasks; or `handoff`:
    /// a Skipper's task for a project Skipper, with its tasks under it.
    pub kind: String,
    /// The coordinator whose row it is: `grand`, or a project Skipper's project.
    #[serde(default = "grand")]
    pub owner: String,
    /// The plan's or the hand-off's uid.
    #[serde(default)]
    pub parent: Option<String>,
    /// The full session id on the Host's skiffd. A plan has none.
    #[serde(default)]
    pub pane_id: Option<String>,
    /// The pane's generation at the send.
    #[serde(default)]
    pub pane_gen: Option<i64>,
    /// Empty: no project.
    pub project: String,
    pub task: String,
    pub sent_at: String,
    #[serde(default)]
    pub title: Option<String>,
    /// `sent`, `pending`, `blocked` or `done`. A plan's comes from its tasks.
    pub state: String,
    pub updated_at: String,
    /// The task type from `[skiff_agent.tasks]`.
    #[serde(default, rename = "type")]
    pub task_type: Option<String>,
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    #[serde(default)]
    pub reported_at: Option<String>,
    /// When the person closed the work. Only a work head has it.
    #[serde(default)]
    pub closed_at: Option<String>,
    /// When skiff set the task done because its pane was gone.
    #[serde(default)]
    pub gone_at: Option<String>,
    /// The pane is on another skiffd than the Host's: it was not checked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elsewhere: bool,
    /// The Host can serve the row's result (`ledger_result`): a regular
    /// file inside the ledger's folder, reached without a link.
    pub result_exists: bool,
}

fn grand() -> String {
    "grand".into()
}

/// A task type: Skipper starts a task of this type with this agent and model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTaskType {
    #[serde(rename = "type")]
    pub name: String,
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
    /// The command line the Host's skiffd builds. `None` when the agent is gone.
    #[serde(default)]
    pub command: Option<String>,
    /// Why the row does not start what it says.
    #[serde(default)]
    pub error: Option<String>,
}

/// `[skiff_agent]` as the Host reads it, with the folder's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSkipper {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    pub enabled: bool,
    /// The Host's Skipper folder. Absent when the override is refused.
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub folder_error: Option<String>,
    /// The folder the Host's config names, refused or not.
    pub configured: String,
    /// `ready`, `new`, `other` or `refused`.
    pub folder_status: String,
    pub continue_on_restore: bool,
    /// The project menu offers Start project Skipper.
    #[serde(default)]
    pub project_skippers: bool,
    /// The agent project Skippers start. Absent: the Skipper's.
    #[serde(default)]
    pub project_agent: Option<String>,
    /// Where project Skippers keep `SKIPPER.md` and their results, on the
    /// Host. Absent when the folder is refused.
    #[serde(default)]
    pub projects_folder: Option<String>,
    /// `[skiff_agent.tasks]` in file order.
    pub tasks: Vec<HostTaskType>,
}

/// One change to the Host's `[skiff_agent]` (`skipper_config@1`), in the
/// JSON Settings sends: `{ "key": …, "value": … }`. `None` or empty takes
/// the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "key", content = "value", rename_all = "snake_case")]
pub enum SkipperEdit {
    Name(Option<String>),
    Agent(Option<String>),
    /// On makes the folder ready first: a refused folder stays off.
    Enabled(bool),
    ContinueOnRestore(bool),
    ProjectSkippers(bool),
    /// `None`: Same as Skipper.
    ProjectAgent(Option<String>),
    /// A new row at the end of Task types.
    AddTask(TaskEdit),
    /// A row's agent or model.
    Task(TaskEdit),
    RenameTask { from: String, to: String },
    RemoveTask(String),
}

/// A task type row. An empty or absent model is the agent's default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEdit {
    #[serde(rename = "type")]
    pub name: String,
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
}

/// A Skipper folder's contents, for the preview and the done step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipperFolderCounts {
    /// Rows in the ledger, plans included.
    pub tasks: u64,
    /// Rows that are not done.
    pub open: u64,
    pub done: u64,
    /// Files under `results/`, and their size in bytes.
    pub results: u64,
    pub bytes: u64,
    /// `AGENTS.md` differs from the template.
    pub agents_edited: bool,
    /// Top-level files and folders the person added.
    pub other: Vec<String>,
}

/// An open task whose pane still runs: its child writes in the old folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipperRunningTask {
    pub uid: String,
    pub task: String,
    pub pane_id: String,
}

/// What a change to a folder would do, read on the Host. Changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipperFolderPreview {
    /// The folder Skipper uses now, as the Host's config names it.
    pub from: String,
    /// The chosen folder: absolute and canonical. A change names only this.
    pub to: String,
    /// `current`, `new`, `empty`, `skiff` or `refused`.
    pub kind: String,
    #[serde(default)]
    pub refused: Option<String>,
    /// The current folder is a Skiff folder, so `copy` can run.
    pub can_copy: bool,
    #[serde(default)]
    pub copy_refused: Option<String>,
    #[serde(default)]
    pub from_counts: Option<SkipperFolderCounts>,
    #[serde(default)]
    pub to_counts: Option<SkipperFolderCounts>,
    /// `MOVED.md` in the chosen folder: Skipper moved away from it before.
    #[serde(default)]
    pub moved: Option<String>,
    pub running: Vec<SkipperRunningTask>,
    /// `SKIFF_LEDGER` on the Host, when set.
    #[serde(default)]
    pub ledger_env: Option<String>,
    /// Skipper's session runs: the change ends it first.
    pub skipper_runs: bool,
}

/// How the folder changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipperFolderMode {
    /// Copy the current folder into a new or empty folder, then switch.
    Copy,
    /// Set up a new or empty folder with a new ledger, then switch.
    Empty,
    /// Switch to a Skiff folder as it is.
    Switch,
}

/// What a folder change did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipperFolderDone {
    pub from: String,
    pub to: String,
    pub mode: SkipperFolderMode,
    #[serde(default)]
    pub copied: Option<SkipperFolderCounts>,
    /// The note in the old folder, when the copy left one.
    #[serde(default)]
    pub moved_note: Option<String>,
    /// A step after the switch that did not work. The switch stands.
    #[serde(default)]
    pub warning: Option<String>,
}

/// Skipper's session on the Host, found or started (`skipper_run@1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipperOpened {
    pub session: crate::InventorySession,
    pub created: bool,
    /// The Host typed the agent or its continue line into a bare shell.
    pub typed: bool,
    /// Why the Host's watcher did not start for it.
    #[serde(default)]
    pub watch_error: Option<String>,
}

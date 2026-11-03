use serde::{Deserialize, Serialize};

pub const CONNECTION_STATUS: &str = "connection_status";
pub const SESSION_SCREEN: &str = "session_screen";
pub const SESSION_INVENTORY: &str = "session_inventory";
pub const SESSION_UPDATES: &str = "session_updates";
pub const SESSION_TERMINAL: &str = "session_terminal";
pub const SESSION_LIFECYCLE: &str = "session_lifecycle";
pub const SESSION_GROUPS: &str = "session_groups";
pub const SESSION_AGENTS: &str = "session_agents";
pub const PROJECTS: &str = "projects";
pub const GIT_STATUS: &str = "git_status";
pub const GIT_DIFF: &str = "git_diff";
pub const PROJECT_CONFIG: &str = "project_config";
pub const WORKTREES: &str = "worktrees";
pub const FILES: &str = "files";
pub const FILE_REVEAL: &str = "file_reveal";
pub const FOLDERS: &str = "folders";
pub const PROJECT_ASSETS: &str = "project_assets";
pub const LEDGER: &str = "ledger";
pub const SKIPPER: &str = "skipper";
pub const SKIPPER_CONFIG: &str = "skipper_config";
pub const SKIPPER_FILE: &str = "skipper_file";
pub const SKIPPER_RUN: &str = "skipper_run";
pub const SKIPPER_WATCH_LOG: &str = "skipper_watch_log";
pub const AGENT_CONFIG: &str = "agent_config";
pub const DROPS: &str = "drops";
pub const PROGRAM_STATUS: &str = "program_status";
pub const DATA_EVENTS: &str = "data_events";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    /// Feature major; unrelated to API_MAJOR and skiffd's PROTOCOL.
    pub version: u32,
}
impl Capability {
    fn new(name: &str, version: u32) -> Self {
        Self { name: name.into(), version }
    }

    /// Create, end (kill), rename, theme and mark seen one session. Grants
    /// no terminal I/O, inventory or group operation.
    pub fn session_lifecycle() -> Self {
        Self::new(SESSION_LIFECYCLE, 1)
    }
    /// List, save and delete the Host's session groups.
    pub fn session_groups() -> Self {
        Self::new(SESSION_GROUPS, 1)
    }
    /// List the agent CLIs the Host account can start, from its own
    /// environment. Read-only; grants no create and no settings change.
    pub fn session_agents() -> Self {
        Self::new(SESSION_AGENTS, 1)
    }
    /// Read the Host's projects, fleets and worktrees. Data plane
    /// (`skiff-host`); grants no project, config or worktree change.
    pub fn projects() -> Self {
        Self::new(PROJECTS, 1)
    }
    /// What `projects@1` grants, and `projects_stamp`: a value that changes
    /// with the Host's `projects.toml`, so a client can show an edit made
    /// outside it (`skiff projects add`, a hand edit) without reading the
    /// projects every second. Read-only. A Host that offers it offers
    /// `projects@1` too.
    pub fn projects_watch() -> Self {
        Self::new(PROJECTS, 2)
    }
    /// Pushed invalidations for Host-owned projects, Git, files and ledger
    /// data. A receiver that misses one must read a fresh snapshot.
    pub fn data_events() -> Self {
        Self::new(DATA_EVENTS, 1)
    }
    /// Read the changed files of one of the Host's project worktrees. Data
    /// plane; grants no diff text, file read or Git write.
    pub fn git_status() -> Self {
        Self::new(GIT_STATUS, 1)
    }
    /// Read one changed file's diff text, or a whole worktree's, in one of
    /// the Host's project worktrees. Data plane; bounded plain text only:
    /// grants no file read beyond the diff, no external tool and no write.
    pub fn git_diff() -> Self {
        Self::new(GIT_DIFF, 1)
    }
    /// Change the Host's `[[project]]` tables: add, settings, order and
    /// fleets, close, open and remove. Each change names the revision it was
    /// made from, and a changed revision refuses it (`conflict`). Data plane;
    /// grants no image, file, worktree or Git write.
    pub fn project_config() -> Self {
        Self::new(PROJECT_CONFIG, 1)
    }
    /// Add a worktree on a new branch, or on a branch with no worktree, to
    /// one of the Host's open projects; list its branches; remove a linked
    /// worktree that no live session uses. Data plane; grants no other Git
    /// write.
    pub fn worktrees() -> Self {
        Self::new(WORKTREES, 1)
    }
    /// List the folders and read the files of one of the Host's open project
    /// worktrees, bounded, without following links or opening anything but
    /// a regular file. Data plane; grants no write, no Git and no path the
    /// client may open.
    pub fn files() -> Self {
        Self::new(FILES, 1)
    }
    /// Show a file or folder that is already in an open project worktree in
    /// this Host's file manager. It grants no free path or client-side opener.
    pub fn file_reveal() -> Self {
        Self::new(FILE_REVEAL, 1)
    }
    /// List the Host account's folders from its home, and make one new
    /// folder in a folder it lists. Folders and links to folders only:
    /// grants no file name, no read, no other write and no path the client
    /// may open.
    pub fn folders() -> Self {
        Self::new(FOLDERS, 1)
    }
    /// Read a project's configured background image as validated bytes.
    /// Data plane; grants no write and no other file.
    pub fn project_assets() -> Self {
        Self::new(PROJECT_ASSETS, 1)
    }
    /// Read the Host's ledger as the Ledger panel shows it, a stamp that
    /// changes with it, and a row's result as bounded text or bytes. Data
    /// plane; grants no ledger write, no path the client may open and no
    /// file outside the ledger's folder.
    pub fn ledger_read() -> Self {
        Self::new(LEDGER, 1)
    }
    /// Read the Host's `[skiff_agent]` settings and task types. Data plane;
    /// grants no settings change, no Skipper launch and no watcher control.
    pub fn skipper_settings() -> Self {
        Self::new(SKIPPER, 1)
    }
    /// Change the Host's `[skiff_agent]` settings and task types, preview a
    /// new Skipper folder and change to it (copy, start empty or switch), as
    /// Settings does on the Host's own desktop. Data plane; grants no
    /// Skipper launch, no watcher control and no ledger write.
    pub fn skipper_config() -> Self {
        Self::new(SKIPPER_CONFIG, 1)
    }
    /// Read, write and reset the Host's instruction files by what they are
    /// for, never by path: `AGENTS.md`, each `SKIPPER.md` and each task
    /// type's file. A write carries the revision it was made from. Data
    /// plane; grants no other file, no settings change and no Skipper launch.
    pub fn skipper_file() -> Self {
        Self::new(SKIPPER_FILE, 1)
    }
    /// Find or start Skipper's session on the Host, and its watcher, which
    /// `skiff-host` then keeps running. Data plane; grants no other session
    /// and no settings change.
    pub fn skipper_run() -> Self {
        Self::new(SKIPPER_RUN, 1)
    }
    /// What `skipper_run@1` grants, and fleet Skippers: find or start one,
    /// preview and run the takeover of a fleet's project Skippers on the
    /// person's confirm, and rename a fleet with its Skipper. Data plane;
    /// grants no other session, no ledger write of the client's own and no
    /// settings change. A Host that offers it offers `skipper_run@1` too.
    pub fn skipper_run_fleets() -> Self {
        Self::new(SKIPPER_RUN, 2)
    }
    /// Read Skipper's watcher log as bounded text. The Host chooses the log
    /// beside its own skiffd socket; a client supplies no path and never
    /// opens one locally.
    pub fn skipper_watch_log() -> Self {
        Self::new(SKIPPER_WATCH_LOG, 1)
    }
    /// Read and change the Host's `[agents]` table: which agents are on,
    /// their commands, custom agents. Each change names the revision it was
    /// made from, and a changed revision refuses it (`conflict`). Data
    /// plane; grants no session and no other setting.
    pub fn agent_config() -> Self {
        Self::new(AGENT_CONFIG, 1)
    }
    /// Put a file dropped or pasted on a pane into the Host's image cache, so
    /// the pane can type its Host path. Data plane; grants no read, no other
    /// write and no path outside the cache.
    pub fn drops() -> Self {
        Self::new(DROPS, 1)
    }
    /// Attach/output, input, resize and detach of existing sessions; grants
    /// no create, kill, rename or inventory operation.
    pub fn session_terminal() -> Self {
        Self::new(SESSION_TERMINAL, 1)
    }
    /// Sessions carry what their program reports with OSC 7501
    /// (`program_status`). Read-only; grants no request.
    pub fn program_status() -> Self {
        Self::new(PROGRAM_STATUS, 1)
    }
    /// Read-only non-output session events; grants no request or output.
    pub fn session_updates() -> Self {
        Self::new(SESSION_UPDATES, 1)
    }
    pub fn session_inventory() -> Self {
        Self::new(SESSION_INVENTORY, 1)
    }

    /// Read-only text snapshot; does not grant session lifecycle or events.
    pub fn session_screen() -> Self {
        Self::new(SESSION_SCREEN, 1)
    }
    pub fn connection_status() -> Self {
        Self::new(CONNECTION_STATUS, 1)
    }
}

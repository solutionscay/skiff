use serde::{Deserialize, Serialize};

use crate::wire::bytes;
use crate::{AgentEdit, Group, HostFileScope, ProjectEdit, ProjectFleet, ReadPurpose, SessionSpec, SkipperEdit, SkipperFolderMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Hello,
    ConnectionStatus,
    SessionInventory,
    SessionScreen {
        session: String,
        rows: u16,
    },
    /// (Re)subscribe: a `terminal_snapshot` precedes later output and can
    /// arrive before this reply. Take the output receiver first.
    TerminalAttach {
        session: String,
    },
    TerminalDetach {
        session: String,
    },
    /// A session has one PTY size for every attached client. The Host
    /// applies resizes in the order it receives them: the last one applied
    /// is the size, and every client hears it as a session update. A client
    /// does not answer another client's size with its own; it sends again
    /// only when its own view changes size. Answers `ok`.
    TerminalResize {
        session: String,
        cols: u16,
        rows: u16,
    },
    /// Ordered keystrokes or a paste. Sent without waiting for its reply;
    /// never replayed after loss.
    TerminalInput {
        session: String,
        #[serde(with = "bytes")]
        data: Vec<u8>,
        #[serde(default)]
        paste: bool,
    },
    /// Answers `session` with the new row. Not retried after a timeout:
    /// the session may have started.
    SessionCreate {
        spec: SessionSpec,
    },
    /// Ends the session's process. Answers `ok`. The caller asks first.
    SessionKill {
        session: String,
    },
    /// Answers `session` with the renamed row.
    SessionRename {
        session: String,
        name: String,
    },
    /// `None` clears the session's own theme. Answers `session`.
    SessionSetTheme {
        session: String,
        theme: Option<String>,
    },
    /// Clears unread output and the agent exit mark. Answers `ok`.
    SessionSeen {
        session: String,
    },
    GroupList,
    /// Creates (empty id) or replaces a group. Answers `group` as stored.
    GroupSave {
        group: Group,
    },
    GroupDelete {
        group: String,
    },
    /// Puts the groups in this order of ids; the rest follow in theirs.
    /// Answers `ok`.
    GroupOrder {
        groups: Vec<String>,
    },
    /// Answers `agents` with the Host account's agent list.
    AgentList,
    /// Answers `projects` with every project in the Host's config, open and
    /// closed, in config order.
    ProjectList,
    /// Answers `project` with one project read again: git runs in it only.
    ProjectGet {
        name: String,
    },
    /// Answers `changes`, sorted by path. `worktree` is a Host path, and
    /// must be a worktree of one of the Host's open projects: the Host runs
    /// git nowhere else (`not_found`).
    GitChanges {
        worktree: String,
    },
    /// Answers `diff`: `file`'s diff against HEAD, or the whole worktree's
    /// without untracked files. `worktree` as for `git_changes`; `file` is
    /// relative to it, else `not_found`.
    GitDiff {
        worktree: String,
        #[serde(default)]
        file: Option<String>,
    },
    /// Answers `folder`: what `project_add` would do for `path`, a folder on
    /// the Host. Writes nothing.
    ProjectInspect {
        path: String,
    },
    /// Runs `git init` in `path`, a Host folder, unless it is in a
    /// repository already. Stages nothing and commits nothing. Answers
    /// `folder`, as `project_inspect` does after it.
    ProjectInit {
        path: String,
    },
    /// Adds the repository that holds `path` (a Host folder) as a project, or
    /// opens it again when it is a closed one. Answers `project_added`.
    /// `revision` as for `project_edit`.
    ProjectAdd {
        path: String,
        revision: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        short: Option<String>,
        #[serde(default)]
        color: Option<String>,
    },
    /// Changes one key of one project. Answers `revision`, the new one.
    /// Refused (`conflict`) when the Host's `[[project]]` tables are no
    /// longer at `revision`, from `projects` or an earlier write: then the
    /// client reads again. A change is never retried by the client.
    ProjectEdit {
        project: String,
        revision: String,
        edit: ProjectEdit,
    },
    /// Puts the projects in `order` (the rest after them) and sets the
    /// fleets given, in one write. Every name must be a project. Answers
    /// `revision`.
    ProjectArrange {
        revision: String,
        order: Vec<String>,
        #[serde(default)]
        fleets: Vec<ProjectFleet>,
    },
    /// Drops the project's table and settings; its folder stays. Answers `revision`.
    ProjectRemove {
        project: String,
        revision: String,
    },
    /// Adds a worktree on the new `branch`, beside the project's folder.
    /// With `checkout`, `branch` is one `branch_list` shows without a
    /// worktree (a remote's by its name after the remote), and `base` is
    /// absent: git checks it out. Answers `worktree`. Not retried: git may
    /// have made it.
    WorktreeAdd {
        project: String,
        branch: String,
        #[serde(default)]
        base: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        checkout: bool,
    },
    /// Answers `branches`: the open project's local and remote branches,
    /// newest commit first, and the branch its folder has checked out.
    BranchList {
        project: String,
    },
    /// Removes a linked worktree of the project. Refused (`conflict`) while
    /// a session of the Host's skiffd runs in it, or with uncommitted changes
    /// unless `force`, which deletes them; the main worktree is `invalid`.
    /// Answers `ok`.
    WorktreeRemove {
        project: String,
        path: String,
        #[serde(default)]
        force: bool,
    },
    /// Answers `files`: the folder `dir` (relative; empty for the worktree
    /// itself) of `worktree`, checked as for `git_changes`. A path that
    /// leaves the worktree, or passes through a link, is `not_found`.
    /// With `follow_links`, links are followed: a link to a folder lists as
    /// a folder and can be listed. A client sets it for This Mac only.
    FileList {
        worktree: String,
        #[serde(default)]
        dir: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        follow_links: bool,
        /// Cursor returned by the preceding reply. This is an opaque integer,
        /// rather than a person-entered path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<usize>,
    },
    /// Shows a worktree folder, or one regular file in it, in this Host's
    /// file manager. `file` is relative to the named open worktree.
    FileReveal {
        worktree: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    /// Answers `file`: `file` (relative) of `worktree`, read for `purpose`.
    FileRead {
        worktree: String,
        file: String,
        #[serde(default)]
        purpose: ReadPurpose,
    },
    /// Answers `path`: the Host account's home folder, links resolved.
    FolderHome,
    /// Answers `folders`: the folders in `path`, a full Host path. Hidden
    /// folders only with `hidden`. A path that is not a folder is
    /// `not_found`; one the account cannot open is `permission`.
    /// `filter`: only names that hold it, ignoring case, before the bound.
    FolderList {
        path: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        hidden: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<String>,
    },
    /// Answers `programs`: the names of the executable files in `path`, a
    /// full Host path, sorted. A path that is not a folder is `not_found`.
    ProgramList { path: String },
    /// Answers `places`: the Recent list, and the git repositories a
    /// bounded scan of home found (depth 4, 5 s, 200 repositories, no
    /// hidden folder, `node_modules`, `target`, `vendor` or `.cache`). The
    /// scan runs in the background: while `scanning`, ask again.
    FolderPlaces,
    /// Makes the folder `name` (one name, no `/`) in `parent`. Answers
    /// `path`, the new folder's real path. A bad name is `invalid`; a name
    /// that is taken is `conflict`.
    FolderCreate {
        parent: String,
        name: String,
    },
    /// Writes a file dropped or pasted on a pane into the Host's cache.
    /// Answers `path`, the Host path to type. An image (by its first bytes)
    /// is `cache/<hash>.<ext>`; any other file is `cache/files/<hash>/<name>`.
    /// `name` is one file name, no `/`; a bad name is `invalid`.
    DropPut {
        name: String,
        #[serde(with = "bytes")]
        bytes: Vec<u8>,
        /// One upload id and its complete size make this a framed upload.
        /// Absent keeps the original one-frame request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upload: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
        #[serde(default)]
        offset: u64,
        /// Only this frame stores the completed file in the Host cache.
        #[serde(default)]
        finish: bool,
    },
    /// Answers the Host's image cache, newest first.
    ImageList,
    /// Answers one Host cache image's validated bytes.
    ImageRead { path: String },
    /// Deletes Host cache images. Each path must be one returned by
    /// `image_list`; an absent image is already deleted.
    ImageDelete { paths: Vec<String> },
    /// Pins or unpins Host cache images.
    ImagePin { paths: Vec<String>, on: bool },
    /// Answers the Host cache's total size, image count and image size.
    CacheUse,
    /// Answers `image`: the project's configured background, read on the
    /// Host. No background is `not_found`; a file that is not an image, or
    /// is over 5 MiB, is `invalid`.
    ProjectBackground {
        project: String,
    },
    /// Answers `ledger`: the panel's rows, oldest first. No `ledger.db` on
    /// the Host is no rows; one older than the readers take is `invalid`.
    LedgerRows,
    /// Answers `ledger_stamp`: a value that changes when the ledger does.
    LedgerStamp,
    /// Answers `projects_stamp`: a value that changes with any change to
    /// the Host's `projects.toml` (`projects@2`).
    ProjectsStamp,
    /// Answers `file`: the result of the row `uid`, read as a preview. A
    /// row with no result, or one outside the ledger's folder, through a
    /// link or not a regular file, is `not_found`.
    LedgerResult {
        uid: String,
    },
    /// Answers `skipper`.
    SkipperSettings,
    /// Changes one setting, then answers `skipper`, read again.
    SkipperEdit {
        edit: SkipperEdit,
    },
    /// Answers `skipper_file`: the file of `scope` and its revision. A
    /// missing file is written from the template first, as Edit does on
    /// the Host's own desktop.
    SkipperFileRead {
        scope: HostFileScope,
    },
    /// Writes `text` to the file of `scope`, then answers `skipper_file`
    /// with the new revision. Refused (`conflict`) when the file changed
    /// since `revision`. Over 1 MiB is `invalid`.
    SkipperFileWrite {
        scope: HostFileScope,
        text: String,
        revision: String,
    },
    /// Writes the template again and keeps the old file as `<name>.bak`,
    /// then answers `skipper`, read again. A task type's file has no
    /// template: `invalid`.
    SkipperFileReset {
        scope: HostFileScope,
    },
    /// Answers `skipper_folder`: what a change to `path` would do. A full
    /// Host path, or one under home as `~/`. Writes nothing.
    SkipperFolderPreview {
        path: String,
    },
    /// Changes Skipper's folder to `path`, the preview's `to`. A running
    /// Skipper session is ended first when `end_skipper`, else it refuses
    /// the change (`conflict`). `anyway` runs it while tasks still run.
    /// Answers `skipper_folder_done`. Not retried: files may have moved.
    SkipperFolderChange {
        path: String,
        mode: SkipperFolderMode,
        #[serde(default)]
        end_skipper: bool,
        #[serde(default)]
        anyway: bool,
    },
    /// Finds Skipper's session or starts it, and its watcher. With
    /// `restore`, the `input_gen` the client read: only continue a restored
    /// shell, never start one; then `ok` when there is none. Answers
    /// `skipper_opened`. Not retried: the session may have started.
    SkipperOpen {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore: Option<u64>,
    },
    /// Starts the watcher for Skipper's session `session` unless one runs.
    /// Answers `watch`.
    SkipperWatch {
        session: String,
    },
    /// Answers `file`: Skipper's watcher log as a bounded preview. The Host
    /// chooses the log beside its own skiffd socket; it never accepts a path.
    SkipperWatchLog,
    /// Finds the project Skipper of `project` or starts it in the project's
    /// folder, with `SKIPPER.md` as its first message, and the watcher.
    /// Answers `session`. Not retried: the session may have started.
    SkipperProject {
        project: String,
    },
    /// Finds the fleet Skipper of `fleet` or starts it in its folder, with
    /// `SKIPPER.md` as its first message, and the watcher. The client asked
    /// the person first: a start takes the fleet's project Skippers over.
    /// Answers `session`. Not retried: the session may have started.
    SkipperFleet {
        fleet: String,
    },
    /// The project Skippers a takeover of `fleet` would stop: those of its
    /// projects, and of `join` when it joins. Writes nothing. Answers
    /// `fleet_takeover`.
    FleetTakeoverPreview {
        fleet: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        join: Option<String>,
    },
    /// On the person's confirm: the running fleet Skipper of `fleet` takes
    /// `join`'s project Skipper over, then `join` joins the fleet. The
    /// ledger, then skiffd, then `projects.toml`. Answers `fleet_takeover`
    /// with what it took over. Not retried: a repeat is the repair.
    FleetTakeover {
        fleet: String,
        join: String,
    },
    /// Rename fleet…: the fleet, its Skipper's folder, rows, session and
    /// mark. Answers `ok`. Not retried.
    FleetRename {
        old: String,
        new: String,
    },
    /// Answers `agent_config`: the agent list and the `[agents]` revision.
    AgentConfig,
    /// Changes the `[agents]` table, only while it is still at `revision`.
    /// Answers `agent_config` after it. Refused (`conflict`) when the table
    /// changed: then the client reads again. Never retried by the client.
    AgentEdit {
        revision: String,
        edit: AgentEdit,
    },
}

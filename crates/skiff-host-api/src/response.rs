use serde::{Deserialize, Serialize};

use crate::{
    FolderListing, FolderPlaces, Group, Hello, HostAgent, HostBranches, HostCacheImage, HostCacheUse, HostChange, HostDiff, HostFile, HostFolder, HostImage, HostLedgerRow, HostListing, HostProject, HostSkipper, HostSkipperFile,
    HostWorktree, InventorySession, SkipperFolderDone, SkipperFolderPreview, SkipperOpened, TakenSkipper,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// `Session` is boxed: a row would otherwise size every reply. Same JSON.
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Session { session: Box<InventorySession> },
    Groups { groups: Vec<Group> },
    Group { group: Group },
    Agents { agents: Vec<HostAgent> },
    Projects {
        projects: Vec<HostProject>,
        /// The `[[project]]` tables these rows were read from, for a
        /// `project_config` change. Absent from a Host without it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
    },
    /// Boxed, as `Session`. Same JSON.
    Project { project: Box<HostProject> },
    /// Same JSON as `HostChanges`: `changes`, and `base` when there is one.
    Changes {
        changes: Vec<HostChange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
    },
    Diff { diff: HostDiff },
    Folder { folder: HostFolder },
    /// The `[[project]]` tables' revision after a change.
    Revision { revision: String },
    /// Boxed, as `Project`. Same JSON.
    ProjectAdded { project: Box<HostProject>, revision: String },
    Worktree { worktree: HostWorktree },
    Branches { branches: HostBranches },
    Files { listing: HostListing },
    /// Boxed, as `Project`. Same JSON.
    File { file: Box<HostFile> },
    Image { image: HostImage },
    Images { images: Vec<HostCacheImage> },
    CacheUse { usage: HostCacheUse },
    /// A folder on the Host.
    Path { path: String },
    Folders { listing: FolderListing },
    Places { places: FolderPlaces },
    Ledger { rows: Vec<HostLedgerRow> },
    LedgerStamp { stamp: String },
    ProjectsStamp { stamp: String },
    /// Boxed, as `Project`. Same JSON.
    Skipper {
        #[serde(rename = "skipper")]
        settings: Box<HostSkipper>,
    },
    SkipperFile { file: HostSkipperFile },
    /// Boxed, as `Project`. Same JSON.
    SkipperFolder { preview: Box<SkipperFolderPreview> },
    SkipperFolderDone { done: SkipperFolderDone },
    /// Boxed, as `Session`. Same JSON.
    SkipperOpened { opened: Box<SkipperOpened> },
    /// `started`: this request started a watcher. False: one ran already.
    Watch { started: bool },
    /// The project Skippers a fleet Skipper's takeover stops, or stopped.
    FleetTakeover { taken: Vec<TakenSkipper> },
    /// The Host's agents, and the `[agents]` revision they were read at.
    AgentConfig { agents: Vec<HostAgent>, revision: String },
    SessionInventory { sessions: Vec<InventorySession> },
    SessionScreen { lines: Vec<String> },
    ConnectionStatus { version: String, protocol: u32 },
    Hello { hello: Hello },
    Ok,
}

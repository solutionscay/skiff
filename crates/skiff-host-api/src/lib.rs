//! The Host contract. No I/O, service implementation, application state or core types.
//! This contract is not sent by the existing desktop/CLI connection.
pub mod agents;
pub mod files;
pub mod folders;
pub mod groups;
pub mod inventory;
pub mod coordinator;
pub mod lifecycle;
pub mod projects;
pub mod terminal;
pub mod updates;
mod capability;
mod error;
mod hello;
mod host;
mod message;
mod request;
mod request_rules;
mod response;
mod wire;
pub use agents::{AgentEdit, HostAgent};
pub use files::{EntryKind, FileContent, HostEntry, HostFile, HostImage, HostListing, ReadPurpose};
pub use folders::{FolderEntry, FolderKind, FolderListing, FolderPlaces, FolderRepo, PlaceRepo};
pub use groups::Group;
pub use inventory::InventorySession;
pub use coordinator::{
    HostFileScope, HostInstruction, HostLedgerRow, HostSkipper, HostSkipperFile, HostTaskType, SkipperEdit, SkipperFolderCounts, SkipperFolderDone, SkipperFolderMode, SkipperFolderPreview,
    SkipperOpened, SkipperRunningTask, TakenSkipper, TakenTask, TaskEdit,
};
pub use lifecycle::SessionSpec;
pub use projects::{ChangeStatus, HostBranch, HostBranches, HostChange, HostChanges, HostDiff, HostFolder, HostProject, HostWorktree, ProjectEdit, ProjectFleet};
pub use terminal::{EchoTimes, TerminalFrame};
pub use updates::SessionUpdate;
pub use capability::{Capability, AGENT_CONFIG, CONNECTION_STATUS, DATA_EVENTS, DROPS, FILES, FOLDERS, GIT_DIFF, GIT_STATUS, LEDGER, PROGRAM_STATUS, PROJECTS, PROJECT_ASSETS, PROJECT_CONFIG, SESSION_AGENTS, SESSION_GROUPS, SESSION_INVENTORY, SESSION_LIFECYCLE, SESSION_SCREEN, SESSION_TERMINAL, SESSION_UPDATES, SKIPPER, SKIPPER_CONFIG, SKIPPER_FILE, SKIPPER_RUN, WORKTREES};
pub use error::Error;
pub use hello::{Hello, Limits, API_MAJOR};
pub use host::{Endpoint, Host, HostId, InstanceId};
pub use message::{Envelope, Event, Message};
pub use request::Request;
pub use response::Response;

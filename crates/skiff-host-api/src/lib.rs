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
pub mod usage;
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
pub use files::{chunks, EntryKind, FileContent, HostCacheImage, HostCacheUse, HostEntry, HostFile, HostImage, HostListing, HostWho, ReadPurpose, DATA_CHUNK};
pub use folders::{FolderEntry, FolderKind, FolderListing, FolderPlaces, FolderRepo, PlaceRepo};
pub use groups::Group;
pub use inventory::InventorySession;
pub use coordinator::{
    HostCronPreview, HostFileScope, HostInstruction, HostLedgerRow, HostSchedule, HostScheduleRun, HostSkipper, HostSkipperFile, HostTaskType, SkipperEdit, SkipperFolderMoved,
    SkipperAgent, SkipperAgentEdit, SkipperOpened, TakenSkipper, TakenTask, TaskEdit,
};
pub use lifecycle::SessionSpec;
pub use projects::{ChangeStatus, HostBranch, HostBranches, HostChange, HostChanges, HostDiff, HostFolder, HostProject, HostWorktree, ProjectEdit, ProjectFleet};
pub use terminal::{EchoTimes, TerminalFrame};
pub use updates::SessionUpdate;
pub use usage::{AgentUsage, UsageCredits, UsageError, UsageRefresh, UsageWindow};
pub use capability::{Capability, AGENT_CONFIG, AGENT_PROGRAMS, AGENT_USAGE, CONNECTION_STATUS, DATA_EVENTS, DROPS, FILES, FILE_REVEAL, FOLDERS, GIT_DIFF, GIT_STATUS, IMAGE_CACHE, LEDGER, PROGRAM_STATUS, PROJECTS, PROJECT_ASSETS, PROJECT_CONFIG, SCHEDULES, SESSION_AGENTS, SESSION_GROUPS, SESSION_INVENTORY, SESSION_LIFECYCLE, SESSION_SCREEN, SESSION_TERMINAL, SESSION_UPDATES, SKIPPER, SKIPPER_CONFIG, SKIPPER_FILE, SKIPPER_RUN, WORKTREES};
pub use error::Error;
pub use hello::{Hello, Limits, API_MAJOR};
pub use host::{Endpoint, Host, HostId, InstanceId};
pub use message::{Envelope, Event, Message};
pub use request::Request;
pub use response::Response;

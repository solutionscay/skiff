//! Shared types for Skiff: the session model, the daemon wire protocol,
//! the `projects.toml` config, and the socket location.

pub mod alias;
pub mod config;
pub mod files;
pub mod git;
pub mod group;
pub mod open;
pub mod project;
pub mod protocol;
pub mod session;
pub mod socket;
mod shell;
pub mod theme;

pub use group::{Group, Layout, SplitDir};
pub use project::{Project, Worktree};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bumped when a request or reply changes. A client that sees another
/// number is talking to an older or newer daemon.
pub const PROTOCOL: u32 = 20;

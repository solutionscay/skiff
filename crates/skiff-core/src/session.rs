use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub type SessionId = String;

/// Derived from the PTY stream only. Skiff installs nothing into agent configs.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    /// Output arrived recently.
    Working,
    /// The process rang the bell and has had no input since.
    Waiting,
    /// No output for a while.
    Idle,
    /// The process exited.
    Done,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Agent,
    Task,
    Server,
    #[default]
    Shell,
}

/// What a client asks the daemon to start.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SessionSpec {
    /// Display name. Empty means: use the command's file name.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub role: Role,
    /// Working directory. `None` means the daemon's home directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Program to run. `None` or empty means the login shell.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

fn default_cols() -> u16 {
    120
}
fn default_rows() -> u16 {
    40
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub label: String,
    pub role: Role,
    pub cwd: PathBuf,
    pub command: String,
    pub args: Vec<String>,
    pub state: SessionState,
    pub cols: u16,
    pub rows: u16,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    /// This terminal's own theme id; wins over the project's.
    #[serde(default)]
    pub theme: Option<String>,
    /// A name the user gave. It wins over `title`.
    #[serde(default)]
    pub name: Option<String>,
    /// The terminal title the program set (OSC 0 or 2), if any.
    #[serde(default)]
    pub title: Option<String>,
    /// Unix ms. Defaulted so an older daemon still deserializes.
    #[serde(default)]
    pub started_at: u64,
    /// Unix ms of the last PTY read.
    #[serde(default)]
    pub last_output_at: u64,
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

//! Agent plan usage (`agent_usage@1`): each agent's limit windows as its own
//! usage command reports them on the Host. Served by `skiff-host`. Every
//! number is percent **used**, whatever the source reports: a reader turns
//! "left" into "used" before a value leaves it.
use serde::{Deserialize, Serialize};

/// What a `usage_read` asks the Host to read before it answers. The answer
/// never waits for a read: it gives the last values, with `reading` set on
/// each agent a read runs for. Ask again to see the result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageRefresh {
    /// Read nothing: the last values only.
    #[default]
    None,
    /// The page opened: read each agent whose last read is older than 60 s,
    /// running or not.
    Stale,
    /// Read now: read every agent, running or not.
    All,
}

/// One agent with a usage reader, as its last read left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentUsage {
    /// The agent id, as Settings › Agents has it: "claude", "codex", "gemini", "grok".
    pub agent: String,
    /// The plan, only when the source gives one ("Pro Lite", "SuperGrok").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Unix ms of the last read that worked. `None`: none has yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<u64>,
    /// The windows of that read, in the source's order. Empty: the read
    /// worked and returned no window.
    #[serde(default)]
    pub windows: Vec<UsageWindow>,
    /// Codex's credit balance. `None`: the source gives no credits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<UsageCredits>,
    /// The last read failed, after `read_at`. The values above stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<UsageError>,
    /// A read runs now.
    #[serde(default)]
    pub reading: bool,
}

/// One limit window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// The vendor's group or model name ("Gemini Models", "Fable"). `None`
    /// for the agent's main limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The window's length in minutes, from the source: 300 is 5 hours,
    /// 10080 a week. The client names the window from it.
    pub minutes: u32,
    /// A credit window (Grok's weekly credits), not a request limit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub credits: bool,
    /// Percent used, 0 to 100, rounded to the nearest whole percent. 100
    /// only when the source says the window is used up: 99.6 is 99.
    pub used: u8,
    /// Unix ms when the window resets. `None`: the source gives no time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    /// The percent used from which the window is near its limit: 90 for a
    /// window of 5 hours or less, 80 for a longer one.
    pub threshold: u8,
}

/// A credit balance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageCredits {
    /// The balance. `None` when `unlimited`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unlimited: bool,
}

/// A failed read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageError {
    /// Unix ms when the read failed.
    pub at: u64,
    /// What failed, as plain text: "grok agent stdio exited with code 1".
    pub message: String,
}

use crate::Client;
use anyhow::{bail, Result};
use skiff_core::{project::AgentInfo, protocol::{Request, Response}};

impl Client {
    /// The agents, installed or not on the sessions' PATH.
    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        match self.request(Request::ListAgents).await? {
            Response::Agents { agents } => Ok(agents),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    /// What an agent's session list command printed in `cwd`.
    pub async fn list_agent_sessions(&self, command: Vec<String>, cwd: std::path::PathBuf) -> Result<String> {
        match self.request(Request::ListAgentSessions { command, cwd }).await? {
            Response::Output { stdout } => Ok(stdout),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }
}

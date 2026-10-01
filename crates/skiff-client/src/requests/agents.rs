use crate::Client;
use anyhow::{bail, Result};
use skiff_core::{project::AgentInfo, protocol::{Request, Response}};

impl Client {
    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::ListAgents).await
    }

    pub async fn set_agents(&self, enabled: Vec<String>) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::SetAgents { enabled }).await
    }

    pub async fn set_agent_command(&self, agent: String, command: String) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::SetAgentCommand { agent, command }).await
    }

    pub async fn rename_agent(&self, agent: String, name: String) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::RenameAgent { agent, name }).await
    }

    pub async fn remove_agent(&self, agent: String) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::RemoveAgent { agent }).await
    }

    pub async fn restore_agents(&self) -> Result<Vec<AgentInfo>> {
        self.agents_reply(Request::RestoreAgents).await
    }

    async fn agents_reply(&self, req: Request) -> Result<Vec<AgentInfo>> {
        match self.request(req).await? {
            Response::Agents { agents } => Ok(agents),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }
}

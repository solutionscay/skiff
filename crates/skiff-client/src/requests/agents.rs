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

    async fn agents_reply(&self, req: Request) -> Result<Vec<AgentInfo>> {
        match self.request(req).await? {
            Response::Agents { agents } => Ok(agents),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }
}

use crate::Client;
use anyhow::{bail, Result};
use skiff_core::{group::Group, protocol::{Request, Response}};

impl Client {
    pub async fn list_groups(&self) -> Result<Vec<Group>> {
        match self.request(Request::ListGroups).await? {
            Response::Groups { groups } => Ok(groups),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn save_group(&self, group: Group) -> Result<Group> {
        match self.request(Request::SaveGroup { group }).await? {
            Response::Group { group } => Ok(group),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn delete_group(&self, id: &str) -> Result<()> {
        self.expect_ok(Request::DeleteGroup {
            group: id.to_string(),
        })
        .await
    }
}

use crate::Client;
use std::path::PathBuf;
use anyhow::{bail, Result};
use skiff_core::{config::FolderInfo, project::{Project, Worktree}, protocol::{Request, Response}};

impl Client {
    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        match self.request(Request::ListProjects).await? {
            Response::Projects { projects } => Ok(projects),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn add_worktree(
        &self,
        project: &str,
        branch: &str,
        base: Option<String>,
    ) -> Result<Worktree> {
        let request = Request::AddWorktree {
            project: project.to_string(),
            branch: branch.to_string(),
            base,
        };
        match self.request(request).await? {
            Response::Worktree { worktree } => Ok(worktree),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn read_icon(&self, path: PathBuf) -> Result<Option<String>> {
        match self.request(Request::ReadIcon { path }).await? {
            Response::Icon { icon } => Ok(icon),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn set_project_icon(&self, project: String, icon: Option<String>) -> Result<()> {
        self.expect_ok(Request::SetProjectIcon { project, icon }).await
    }

    pub async fn reorder_projects(&self, order: Vec<String>) -> Result<()> {
        self.expect_ok(Request::ReorderProjects { order }).await
    }

    pub async fn set_project_background(&self, project: String, background: Option<String>) -> Result<()> {
        self.expect_ok(Request::SetProjectBackground { project, background }).await
    }

    pub async fn set_project_color(&self, project: String, color: String) -> Result<()> {
        self.expect_ok(Request::SetProjectColor { project, color }).await
    }

    pub async fn set_project_closed(&self, project: String, closed: bool) -> Result<()> {
        self.expect_ok(Request::SetProjectClosed { project, closed }).await
    }

    pub async fn remove_project(&self, project: String) -> Result<()> {
        self.expect_ok(Request::RemoveProject { project }).await
    }

    pub async fn inspect_folder(&self, path: PathBuf) -> Result<FolderInfo> {
        match self.request(Request::InspectFolder { path }).await? {
            Response::Folder { folder } => Ok(folder),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn add_project(
        &self,
        path: PathBuf,
        name: Option<String>,
        short: Option<String>,
        color: Option<String>,
        icon: Option<String>,
        agents: Vec<String>,
    ) -> Result<Project> {
        let req = Request::AddProject {
            path,
            name,
            short,
            color,
            icon,
            agents,
        };
        match self.request(req).await? {
            Response::Project { project } => Ok(project),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn remove_worktree(&self, project: &str, path: PathBuf) -> Result<()> {
        self.expect_ok(Request::RemoveWorktree {
            project: project.to_string(),
            path,
        })
        .await
    }
}

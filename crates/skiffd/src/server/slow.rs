use std::{path::Path, sync::Arc};
use anyhow::{anyhow, bail, Result};
use skiff_core::{git, project, protocol::{Event, Request, Response}, session::SessionState};
use crate::session::SessionPool;
use super::error;

/// Requests that wait on the disk, git, or a new process. They run in order
/// on their own task, so input and resizes behind them are not held up.
pub(super) async fn answer_slow(pool: Arc<SessionPool>, request: Request) -> Response {
    match request {
        Request::CreateSession { spec } => {
            let pool = pool.clone();
            match blocking(move || pool.create(spec)).await {
                Ok(session) => Response::Session { session },
                Err(e) => error(e),
            }
        }
        Request::ListProjects => match blocking(project::list).await {
            Ok(projects) => Response::Projects { projects },
            Err(e) => error(e),
        },
        Request::AddWorktree {
            project,
            branch,
            base,
        } => match blocking(move || project::add_worktree(&project, &branch, base.as_deref()))
            .await
        {
            Ok(worktree) => {
                let _ = pool.events.send(Event::ProjectsChanged {});
                Response::Worktree { worktree }
            }
            Err(e) => error(e),
        },
        Request::GetKeys => match blocking(|| Ok(skiff_core::config::load()?.keys)).await {
            Ok(keys) => Response::Keys { keys },
            Err(e) => error(e),
        },
        Request::GetAppearance => appearance(blocking(|| Ok(skiff_core::config::load()?.appearance)).await),
        Request::SetAppearance { theme } => appearance(
            blocking(move || {
                skiff_core::config::set_app_theme(theme.as_deref())?;
                Ok(skiff_core::config::load()?.appearance)
            })
            .await,
        ),
        Request::SetFontSize { size } => appearance(
            blocking(move || {
                skiff_core::config::set_font_size(size)?;
                Ok(skiff_core::config::load()?.appearance)
            })
            .await,
        ),
        Request::ListThemes => match blocking(|| Ok(skiff_core::theme::list())).await {
            Ok(themes) => Response::Themes { themes },
            Err(e) => error(e),
        },
        Request::ReadIcon { path } => {
            match blocking(move || Ok(skiff_core::project::data_url(&path))).await {
                Ok(icon) => Response::Icon { icon },
                Err(e) => error(e),
            }
        }
        Request::SetProjectBackground { project, background } => {
            match blocking(move || skiff_core::config::set_project_background(&project, background.as_deref())).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        Request::SetProjectColor { project, color } => {
            match blocking(move || skiff_core::config::set_project_color(&project, &color)).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        Request::ReorderProjects { order } => {
            match blocking(move || skiff_core::config::reorder_projects(&order)).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        Request::SetProjectClosed { project, closed } => {
            match blocking(move || skiff_core::config::set_project_closed(&project, closed)).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        Request::RemoveProject { project } => {
            match blocking(move || skiff_core::config::remove_project(&project)).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        Request::SetProjectIcon { project, icon } => {
            match blocking(move || skiff_core::config::set_project_icon(&project, icon.as_deref())).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        Request::InspectFolder { path } => {
            match blocking(move || Ok(skiff_core::config::inspect_folder(&path))).await {
                Ok(folder) => Response::Folder { folder },
                Err(e) => error(e),
            }
        }
        Request::ListAgents => match blocking(|| Ok(skiff_core::project::agents())).await {
            Ok(agents) => Response::Agents { agents },
            Err(e) => error(e),
        },
        Request::ListAgentSessions { command, cwd } => {
            match blocking(move || agent_sessions(&command, &cwd)).await {
                Ok(stdout) => Response::Output { stdout },
                Err(e) => error(e),
            }
        }
        Request::SetAgentCommand { agent, command } => {
            match blocking(move || {
                skiff_core::config::set_agent_command(&agent, &command)?;
                Ok(skiff_core::project::agents())
            })
            .await
            {
                Ok(agents) => Response::Agents { agents },
                Err(e) => error(e),
            }
        }
        Request::RenameAgent { agent, name } => agents_after(move || skiff_core::config::rename_agent(&agent, &name)).await,
        Request::RemoveAgent { agent } => agents_after(move || skiff_core::config::remove_agent(&agent)).await,
        Request::RestoreAgents => agents_after(skiff_core::config::restore_agents).await,
        Request::SetAgents { enabled } => {
            match blocking(move || {
                skiff_core::config::set_enabled_agents(&enabled)?;
                Ok(skiff_core::project::agents())
            })
            .await
            {
                Ok(agents) => Response::Agents { agents },
                Err(e) => error(e),
            }
        }
        Request::AddProject {
            path,
            name,
            short,
            color,
            icon,
            agents,
        } => {
            match blocking(move || {
                let p = skiff_core::config::add_project(&path, name, short, color, icon, agents)?;
                Ok(skiff_core::project::Project::from_config(&p))
            })
            .await
            {
                Ok(project) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Project { project }
                }
                Err(e) => error(e),
            }
        }
        Request::RemoveWorktree { project, path, force } => {
            let pool2 = pool.clone();
            match blocking(move || remove_worktree(&pool2, &project, &path, force)).await {
                Ok(()) => {
                    let _ = pool.events.send(Event::ProjectsChanged {});
                    Response::Ok
                }
                Err(e) => error(e),
            }
        }
        _ => unreachable!("not a slow request"),
    }
}

/// Runs a list command with the login shell's PATH and aliases. A slow agent
/// CLI is stopped after `LIST_TIMEOUT`, so the menu never hangs on it.
fn agent_sessions(command: &[String], cwd: &Path) -> Result<String> {
    const LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    let (first, rest) = command.split_first().ok_or_else(|| anyhow!("empty command"))?;
    let mut words: Vec<String> = skiff_core::alias::expand(first).map(<[String]>::to_vec).unwrap_or_else(|| vec![first.clone()]);
    words.extend(rest.iter().cloned());
    let mut child = std::process::Command::new(&words[0])
        .args(&words[1..])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("{}: {e}", words[0]))?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
        out
    });
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let out = reader.join().unwrap_or_default();
            if !status.success() {
                bail!("{} exited with {status}", words[0]);
            }
            return Ok(out);
        }
        if start.elapsed() > LIST_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{} took longer than {}s", words[0], LIST_TIMEOUT.as_secs());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(f).await?
}

/// Refuses the main worktree and live sessions inside. Refuses uncommitted
/// changes unless `force`.
fn remove_worktree(pool: &SessionPool, name: &str, path: &Path, force: bool) -> Result<()> {
    let p = project::find(name)?;
    let wt = git::list_worktrees(&p.path)?
        .into_iter()
        .find(|w| project::same_path(&w.path, path))
        .ok_or_else(|| anyhow!("{} is not a worktree of {name}", path.display()))?;
    if wt.is_main {
        bail!("refusing to remove the main worktree");
    }
    // git reports realpaths; session cwds may go through symlinks.
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = real(&wt.path);
    let live = pool
        .list()
        .into_iter()
        .filter(|s| s.state != SessionState::Done)
        .find(|s| real(&s.cwd).starts_with(&root) || s.cwd.starts_with(path));
    if let Some(s) = live {
        bail!("session \"{}\" is still running in this worktree", s.label);
    }
    if !force && wt.path.is_dir() && git::is_dirty(&wt.path)? {
        bail!("worktree has uncommitted changes");
    }
    git::remove_worktree(&p.path, &wt.path, force)
}

fn appearance(a: Result<skiff_core::config::Appearance>) -> Response {
    match a {
        Ok(a) => Response::Appearance {
            theme: a.theme,
            font_size: a.font_size,
        },
        Err(e) => error(e),
    }
}

/// Runs a config edit, then answers with the agent list.
async fn agents_after(f: impl FnOnce() -> anyhow::Result<()> + Send + 'static) -> Response {
    match blocking(move || {
        f()?;
        Ok(skiff_core::project::agents())
    })
    .await
    {
        Ok(agents) => Response::Agents { agents },
        Err(e) => error(e),
    }
}

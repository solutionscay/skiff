//! One task per client connection. Fast requests are answered in order, slow
//! ones in order on a second task; events and subscribed output are
//! interleaved on the same connection.

use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{anyhow, bail, Result};
use skiff_core::{
    protocol::{Envelope, Event, Reply, Request, Response, ServerMessage},
    git, project,
    session::{SessionId, SessionState},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use crate::session::{Session, SessionPool};

pub async fn handle(stream: UnixStream, pool: Arc<SessionPool>) -> Result<()> {
    let (rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(4096);

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let mut line = serde_json::to_vec(&msg)?;
            line.push(b'\n');
            wr.write_all(&line).await?;
        }
        anyhow::Ok(())
    });

    // Every client hears every state change.
    let mut pool_events = pool.events.subscribe();
    let tx_events = tx.clone();
    let events_task = tokio::spawn(async move {
        loop {
            match pool_events.recv().await {
                Ok(e) => {
                    if tx_events.send(ServerMessage::Event(e)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("client lagged {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Slow requests answer from here, in the order they came. A reply can
    // pass one from this queue; the client matches replies by id.
    let (slow_tx, mut slow_rx) = mpsc::unbounded_channel::<(u64, Request)>();
    let slow_task = {
        let tx = tx.clone();
        let pool = pool.clone();
        tokio::spawn(async move {
            while let Some((id, request)) = slow_rx.recv().await {
                let response = answer_slow(pool.clone(), request).await;
                if tx.send(ServerMessage::Reply(Reply { id, response })).await.is_err() {
                    break;
                }
            }
        })
    };

    let mut subs: HashMap<SessionId, JoinHandle<()>> = HashMap::new();
    let mut lines = BufReader::new(rd).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let env: Envelope = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                // Answer with the request's id when it has one, so the
                // client does not wait forever on a command we do not know.
                let id = serde_json::from_str::<serde_json::Value>(&line)
                    .ok()
                    .and_then(|v| v.get("id").and_then(|i| i.as_u64()))
                    .unwrap_or(0);
                let reply = Reply {
                    id,
                    response: Response::Error {
                        message: format!("bad request: {e}"),
                    },
                };
                tx.send(ServerMessage::Reply(reply)).await?;
                continue;
            }
        };

        let response = match env.request {
            Request::Ping => Response::Pong {
                version: skiff_core::VERSION.to_string(),
                protocol: skiff_core::PROTOCOL,
            },
            Request::ListSessions => Response::Sessions {
                sessions: pool.list(),
            },
            req @ (Request::CreateSession { .. }
            | Request::ListProjects
            | Request::AddWorktree { .. }
            | Request::GetKeys
            | Request::GetAppearance
            | Request::SetAppearance { .. }
            | Request::SetFontSize { .. }
            | Request::ListThemes
            | Request::ReadIcon { .. }
            | Request::SetProjectBackground { .. }
            | Request::SetProjectColor { .. }
            | Request::ReorderProjects { .. }
            | Request::SetProjectIcon { .. }
            | Request::InspectFolder { .. }
            | Request::ListAgents
            | Request::SetAgentCommand { .. }
            | Request::SetAgents { .. }
            | Request::AddProject { .. }
            | Request::RemoveWorktree { .. }) => {
                if slow_tx.send((env.id, req)).is_err() {
                    break;
                }
                continue;
            }
            Request::Write { session, data } => match pool.write(&session, &data) {
                Ok(()) => Response::Ok,
                Err(e) => error(e),
            },
            Request::SetSessionTheme { session, theme } => match pool.set_theme(&session, theme) {
                Ok(session) => Response::Session { session },
                Err(e) => error(e),
            },
            Request::RenameSession { session, name } => match pool.rename(&session, &name) {
                Ok(session) => Response::Session { session },
                Err(e) => error(e),
            },
            Request::Resize {
                session,
                cols,
                rows,
            } => match pool.resize(&session, cols, rows) {
                Ok(()) => Response::Ok,
                Err(e) => error(e),
            },
            Request::Subscribe { session } => match pool.get(&session) {
                Some(s) => {
                    // Stop the old stream before the snapshot. Otherwise it can
                    // still send chunks the snapshot already holds.
                    if let Some(old) = subs.remove(&session) {
                        old.abort();
                        let _ = old.await;
                    }
                    let (snapshot, mut out) = s.attach();
                    tx.send(snapshot_event(&session, &s, snapshot)).await?;
                    let tx = tx.clone();
                    let sid = session.clone();
                    // Weak, so a killed session still drops and closes `out`.
                    let s = Arc::downgrade(&s);
                    let task = tokio::spawn(async move {
                        loop {
                            match out.recv().await {
                                Ok(chunk) => {
                                    let ev = Event::Output {
                                        session: sid.clone(),
                                        data: chunk.to_vec(),
                                    };
                                    if tx.send(ServerMessage::Event(ev)).await.is_err() {
                                        break;
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    // Lost chunks corrupt the client's screen.
                                    // Redraw it from a fresh snapshot.
                                    tracing::warn!("subscriber to {sid} lagged {n} chunks");
                                    let Some(s) = s.upgrade() else { break };
                                    let (snapshot, rx) = s.attach();
                                    out = rx;
                                    if tx.send(snapshot_event(&sid, &s, snapshot)).await.is_err() {
                                        break;
                                    }
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    });
                    subs.insert(session, task);
                    Response::Ok
                }
                None => Response::Error {
                    message: format!("no such session: {session}"),
                },
            },
            Request::Unsubscribe { session } => {
                if let Some(task) = subs.remove(&session) {
                    task.abort();
                }
                Response::Ok
            }
            Request::Kill { session } => match pool.kill(&session) {
                Ok(()) => Response::Ok,
                Err(e) => error(e),
            },
            Request::ListGroups => Response::Groups {
                groups: pool.list_groups(),
            },
            Request::SaveGroup { group } => match pool.save_group(group) {
                Ok(group) => Response::Group { group },
                Err(e) => error(e),
            },
            Request::DeleteGroup { group } => match pool.delete_group(&group) {
                Ok(()) => Response::Ok,
                Err(e) => error(e),
            },
        };

        let reply = Reply {
            id: env.id,
            response,
        };
        if tx.send(ServerMessage::Reply(reply)).await.is_err() {
            break;
        }
    }

    for (_, task) in subs {
        task.abort();
    }
    events_task.abort();
    // Answer what is still queued before the writer closes.
    drop(slow_tx);
    let _ = slow_task.await;
    drop(tx);
    let _ = writer.await;
    Ok(())
}

/// Requests that wait on the disk, git, or a new process. They run in order
/// on their own task, so input and resizes behind them are not held up.
async fn answer_slow(pool: Arc<SessionPool>, request: Request) -> Response {
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
        Request::RemoveWorktree { project, path } => {
            let pool2 = pool.clone();
            match blocking(move || remove_worktree(&pool2, &project, &path)).await {
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

fn snapshot_event(session: &str, s: &Session, data: Vec<u8>) -> ServerMessage {
    let info = s.info();
    ServerMessage::Event(Event::Snapshot {
        session: session.to_string(),
        data,
        cols: info.cols,
        rows: info.rows,
    })
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(f).await?
}

/// Refuses the main worktree, uncommitted changes, and live sessions inside.
/// Never forces.
fn remove_worktree(pool: &SessionPool, name: &str, path: &Path) -> Result<()> {
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
    if wt.path.is_dir() && git::is_dirty(&wt.path)? {
        bail!("worktree has uncommitted changes");
    }
    git::remove_worktree(&p.path, &wt.path)
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

fn error(e: anyhow::Error) -> Response {
    Response::Error {
        message: format!("{e:#}"),
    }
}

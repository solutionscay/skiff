//! Track the current foreground program without shell hooks or agent changes.
use super::{away::proc, SessionPool};
use skiff_core::{protocol::Event, session::{SessionState, Was}};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::Arc,
    time::Duration,
};

impl SessionPool {
    pub fn spawn_foreground_watcher(self: &Arc<Self>) {
        let pool = self.clone();
        let _ = std::thread::Builder::new()
            .name("foreground".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                pool.scan_foreground();
            });
    }

    fn scan_foreground(&self) {
        let sessions: Vec<_> = self.sessions.read().unwrap().values().cloned().collect();
        let mut tree = None;
        for session in sessions {
            if session.info.lock().unwrap().state == SessionState::Done {
                continue;
            }
            #[cfg(unix)]
            let group = session
                .master
                .lock()
                .unwrap()
                .process_group_leader()
                .map(|pid| pid as u32);
            #[cfg(not(unix))]
            let group: Option<u32> = None;
            let Some(group) = group else { continue };
            let Some(name) = proc::name(group) else {
                continue;
            };
            let argv = proc::argv(group);
            let agent = agent_program(&name, &argv).map(str::to_owned).or_else(|| {
                // The launch wrapper shares its group with the initial agent.
                // An interactive shell can also start a pipeline in one group.
                let tree = tree.get_or_insert_with(proc::children);
                agent_in_group(group, tree)
            });
            let program = agent.clone().unwrap_or(name);
            let (changed, was_changed) = {
                let mut info = session.info.lock().unwrap();
                // Keep the agent in front and its title, so a restore can offer it.
                // Agents lead the title with a status glyph that changes as they work.
                let title = info
                    .title
                    .as_deref()
                    .map(|t| t.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string())
                    .filter(|t| !t.is_empty());
                // An agent's own name ("Claude Code") is no title. Keep the last real one.
                let title = title.filter(|t| !generic_title(t)).or_else(|| {
                    info.was.as_ref().filter(|w| Some(&w.agent) == agent.as_ref()).and_then(|w| w.title.clone())
                });
                let was = agent.map(|agent| Was { agent, title });
                // An agent in front ends the offer to resume.
                let resumed = was.is_some() && info.resume.take().is_some();
                let was_changed = was.is_some() && info.was != was;
                if was_changed {
                    info.was = was;
                }
                if info.foreground_program.as_deref() == Some(&program) && !resumed {
                    (None, was_changed)
                } else {
                    info.foreground_program = Some(program);
                    (Some(info.clone()), was_changed)
                }
            };
            if was_changed {
                self.dirty.notify_one();
            }
            if let Some(session) = changed {
                let _ = self.events.send(Event::SessionUpdated { session });
            }
        }
    }
}

/// The title an agent shows before a conversation has a topic: its own name.
pub(crate) fn generic_title(title: &str) -> bool {
    let t = title.trim().to_lowercase();
    matches!(
        t.as_str(),
        "claude" | "claude code" | "codex" | "openai codex" | "opencode" | "gemini" | "gemini cli" | "antigravity" | "agy" | "grok" | "grok build"
    )
}

fn agent_in_group(group: u32, tree: &HashMap<u32, Vec<u32>>) -> Option<String> {
    let mut queue = VecDeque::from([group]);
    while let Some(pid) = queue.pop_front() {
        if let Some(children) = tree.get(&pid) {
            for &child in children {
                if proc::group(child) != Some(group) {
                    continue;
                }
                if let Some(name) = proc::name(child) {
                    if let Some(agent) = agent_program(&name, &proc::argv(child)) {
                        return Some(agent.to_owned());
                    }
                }
                queue.push_back(child);
            }
        }
    }
    None
}

fn agent_program(name: &str, argv: &[String]) -> Option<&'static str> {
    fn known(name: &str) -> Option<&'static str> {
        match name {
            "claude" => Some("claude"),
            "codex" => Some("codex"),
            "gemini" | "agy" => Some("gemini"),
            "grok" => Some("grok"),
            "opencode" => Some("opencode"),
            _ => None,
        }
    }
    let base = |s: &str| {
        Path::new(s)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    };
    if let Some(agent) = known(name).or_else(|| argv.first().and_then(|s| known(&base(s)))) {
        return Some(agent);
    }
    // JavaScript CLIs run as node/bun. Only inspect the script, never prompt arguments.
    if !matches!(name, "node" | "bun" | "deno") {
        return None;
    }
    let script = argv.get(1)?;
    if script.starts_with('-') {
        return None;
    }
    if let Some(agent) = known(
        base(script)
            .trim_end_matches(".js")
            .trim_end_matches(".mjs"),
    ) {
        return Some(agent);
    }
    for (package, agent) in [
        ("/@anthropic-ai/claude-code/", "claude"),
        ("/@openai/codex/", "codex"),
        ("/@google/gemini-cli/", "gemini"),
    ] {
        if script.contains(package) {
            return Some(agent);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_binary_and_script_agents_without_reading_prompt_arguments() {
        assert_eq!(agent_program("codex", &[]), Some("codex"));
        assert_eq!(
            agent_program(
                "node",
                &[
                    "node".into(),
                    "/opt/node_modules/@anthropic-ai/claude-code/cli.js".into()
                ]
            ),
            Some("claude")
        );
        assert_eq!(
            agent_program("node", &["node".into(), "/opt/bin/gemini".into()]),
            Some("gemini")
        );
        assert_eq!(
            agent_program("bash", &["bash".into(), "-c".into(), "codex".into()]),
            None
        );
        assert_eq!(
            agent_program(
                "node",
                &["node".into(), "server.js".into(), "claude".into()]
            ),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn foreground_follows_agent_restart_and_shell_return() {
        use skiff_core::session::SessionSpec;
        let dir = std::env::temp_dir().join(format!("skiff-foreground-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for agent in ["claude", "codex"] {
            std::os::unix::fs::symlink("/bin/sleep", dir.join(agent)).unwrap();
        }
        let pool = SessionPool::new();
        let info = pool
            .create(SessionSpec {
                command: Some("/bin/bash".into()),
                args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
                name: Some("My session".into()),
                ..Default::default()
            })
            .unwrap();
        async fn wait(pool: &SessionPool, id: &str, expected: &str) {
            let until = std::time::Instant::now() + Duration::from_secs(3);
            loop {
                pool.scan_foreground();
                if pool.get(id).unwrap().info().foreground_program.as_deref() == Some(expected) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < until,
                    "foreground did not become {expected}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        wait(&pool, &info.id, "bash").await;
        for agent in ["claude", "codex"] {
            pool.write(
                &info.id,
                format!("{} 30\r", dir.join(agent).display()).as_bytes(),
            )
            .unwrap();
            wait(&pool, &info.id, agent).await;
            pool.write(&info.id, b"\x03").unwrap();
            wait(&pool, &info.id, "bash").await;
        }
        assert_eq!(
            pool.get(&info.id).unwrap().info().name.as_deref(),
            Some("My session")
        );
        pool.kill(&info.id).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}

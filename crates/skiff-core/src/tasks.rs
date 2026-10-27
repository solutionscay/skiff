//! Task types: `[skiff_agent.tasks]` names the agent and the model that each
//! kind of work starts with. Skipper reads the task and picks the type. Skiff
//! stores the table and builds the command line. It reads no task text.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{config::TaskTypes, project::AgentInfo, shell::{quote_word, shell_quote}};

/// Where a person sets the table, for error messages.
pub const SETTINGS: &str = "Settings › Task types";

/// The model flag of each known agent, from its `--help` (2026-10-15:
/// claude 2.1.301, codex 0.162.1, grok 1.0.44). An agent with no flag here
/// takes its model in its command, as a custom agent does. Keep in step with
/// `MODEL_FLAGS` in apps/desktop/src/appearance/taskTypes.ts.
pub fn model_flag(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" => Some("--model"),
        "codex" | "grok" | "kimi" => Some("-m"),
        _ => None,
    }
}

/// A task type name: lowercase letters, digits and `-`, starting with a
/// letter or a digit. It is the TOML key and the `--type` value.
pub fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        bail!("A task type needs a name.");
    }
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !first_ok || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        bail!("Use lowercase letters, digits and -, as in code-review. {name:?} is not a task type name.");
    }
    Ok(name)
}

/// A model name: one word. Empty is `None`, the agent's default.
pub fn check_model(model: Option<&str>) -> Result<Option<&str>> {
    let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) else { return Ok(None) };
    if m.chars().any(char::is_whitespace) {
        bail!("A model is one word, as in opus. {m:?} has spaces.");
    }
    Ok(Some(m))
}

/// The folder of the instruction files, in the agent's folder.
pub const TYPES: &str = "types";

/// A type's instruction file: `types/<type>.md` in the agent's folder `dir`.
/// Skiff names the file. The person writes it.
pub fn file(dir: &Path, name: &str) -> PathBuf {
    dir.join(TYPES).join(format!("{name}.md"))
}

/// What Edit writes when the file is missing. The stub alone counts as none.
pub fn stub(name: &str) -> String {
    format!("# {name}\n")
}

/// The file's text when it has instructions: not missing, not empty, not
/// the stub alone.
pub fn text(dir: &Path, name: &str) -> Option<String> {
    let bytes = std::fs::read(file(dir, name)).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let t = text.trim();
    (!t.is_empty() && t != stub(name).trim()).then_some(text)
}

/// Makes the file with its stub when it is missing, for Edit. Keeps a file
/// that is there. Returns the file.
pub fn edit_file(dir: &Path, name: &str) -> Result<PathBuf> {
    let name = check_name(name)?;
    let f = file(dir, name);
    let types = dir.join(TYPES);
    std::fs::create_dir_all(&types).with_context(|| format!("make {}", types.display()))?;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&f) {
        Ok(mut out) => std::io::Write::write_all(&mut out, stub(name).as_bytes()).with_context(|| format!("write {}", f.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("make {}", f.display())),
    }
    Ok(f)
}

/// Moves `from`'s file to `to`'s name, before the row's rename. A stub gets
/// the new name in its line. No file: nothing to move. A file that `to`
/// has already (a removed type kept it) is refused, and nothing moves.
pub fn rename_file(dir: &Path, from: &str, to: &str) -> Result<()> {
    let to = check_name(to)?;
    let (src, dst) = (file(dir, from), file(dir, to));
    if from == to || !src.exists() {
        return Ok(());
    }
    if dst.exists() {
        bail!("{TYPES}/{to}.md exists. Move it out of {}, or pick another name.", dir.join(TYPES).display());
    }
    let stubbed = std::fs::read_to_string(&src).is_ok_and(|t| t.trim() == stub(from).trim());
    if stubbed {
        std::fs::write(&dst, stub(to)).with_context(|| format!("write {}", dst.display()))?;
        std::fs::remove_file(&src).with_context(|| format!("remove {}", src.display()))
    } else {
        std::fs::rename(&src, &dst).with_context(|| format!("move {} to {}", src.display(), dst.display()))
    }
}

/// How a known agent takes the file at start, from its docs, help and source
/// (research 499fa18a, 2026-10-26). Each one adds to the agent's own prompt
/// and keeps the project's `AGENTS.md` / `CLAUDE.md` and the agent's own
/// config.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Delivery {
    /// claude `--append-system-prompt-file <path>`.
    Claude,
    /// codex `-c developer_instructions=<text>`: a TOML string, not a path.
    Codex,
    /// grok `--rules <text>`.
    Grok,
    /// opencode `OPENCODE_CONFIG_CONTENT={"instructions":[<path>]}`. opencode
    /// joins the list with the `instructions` of its global config.
    Opencode,
    /// kimi `--agent-file <wrapper>`: Skiff writes the wrapper in the agent
    /// folder's cache. `${base_prompt}` keeps kimi's own prompt.
    Kimi,
}

/// The way `agent` takes the file. gemini (`agy`) has none. hermes takes
/// its variable in place of the person's own system prompt, so it gets
/// none. Skiff does not know a custom agent's command.
pub fn delivery(agent: &AgentInfo) -> Result<Delivery> {
    let d = match agent.id.as_str() {
        _ if agent.custom => None,
        "claude" => Some(Delivery::Claude),
        "codex" => Some(Delivery::Codex),
        "grok" => Some(Delivery::Grok),
        "opencode" => Some(Delivery::Opencode),
        "kimi" => Some(Delivery::Kimi),
        _ => None,
    };
    d.ok_or_else(|| anyhow::anyhow!("{} cannot take an instruction file. Empty the file or change the agent.", agent.id))
}

/// The largest file codex and grok get on their command line. argv has a limit.
pub const MAX_INLINE: usize = 64 * 1024;

/// The kimi wrapper of a type's file, in the agent folder's cache.
pub fn kimi_wrapper(dir: &Path, name: &str) -> PathBuf {
    dir.join("cache").join(TYPES).join(format!("{name}.md"))
}

/// The kimi agent file: frontmatter, kimi's own prompt, then the text.
fn kimi_agent(name: &str, text: &str) -> String {
    // kimi takes a name of lowercase words joined by single dashes.
    let slug: Vec<&str> = name.split('-').filter(|w| !w.is_empty()).collect();
    format!("---\nname: {}\ndescription: Skiff task type {name}\n---\n${{base_prompt}}\n\n{text}", slug.join("-"))
}

/// What a start adds for the file: argv words after the model, and variables.
#[derive(Debug, Default, PartialEq)]
pub struct Added {
    pub words: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The words and variables that give `agent` the text of `name`'s file. kimi
/// gets its wrapper written first.
pub fn instruction_args(agent: &AgentInfo, dir: &Path, name: &str, text: &str) -> Result<Added> {
    let path = file(dir, name).display().to_string();
    let inline = |agent: &str| {
        if text.len() > MAX_INLINE {
            bail!("The instruction file {path} has {} bytes. {agent} takes it on its command line, so keep it under {MAX_INLINE}.", text.len());
        }
        Ok(())
    };
    let words = |w: &[&str]| w.iter().map(|w| w.to_string()).collect();
    Ok(match delivery(agent)? {
        Delivery::Claude => Added { words: words(&["--append-system-prompt-file", &path]), env: vec![] },
        Delivery::Codex => {
            inline("codex")?;
            // codex reads the value as TOML: a quoted string stays text, whatever the file holds.
            Added { words: vec!["-c".into(), format!("developer_instructions={}", toml::Value::String(text.into()))], env: vec![] }
        }
        Delivery::Grok => {
            inline("grok")?;
            Added { words: words(&["--rules", text]), env: vec![] }
        }
        Delivery::Opencode => {
            let config = serde_json::json!({ "instructions": [path] }).to_string();
            Added { words: vec![], env: vec![("OPENCODE_CONFIG_CONTENT".into(), config)] }
        }
        Delivery::Kimi => {
            let wrapper = kimi_wrapper(dir, name);
            if let Some(d) = wrapper.parent() {
                std::fs::create_dir_all(d).with_context(|| format!("make {}", d.display()))?;
            }
            std::fs::write(&wrapper, kimi_agent(name, text)).with_context(|| format!("write {}", wrapper.display()))?;
            Added { words: words(&["--agent-file", &wrapper.display().to_string()]), env: vec![] }
        }
    })
}

/// `~/…` for a path under the home folder, for the preview and the labels.
pub fn tilde(p: &Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// The file's part of the command line as Settings and `skiff agents` show
/// it: the flag and the file, never the text. `<file>` stands for what Skiff
/// builds from the file at start. The variable goes in front of the command.
pub fn instruction_preview(agent: &AgentInfo, dir: &Path, name: &str) -> Result<(Option<String>, String)> {
    let f = tilde(&file(dir, name));
    Ok(match delivery(agent)? {
        Delivery::Claude => (None, format!("--append-system-prompt-file {}", quote_word(&f))),
        Delivery::Codex => (None, format!("-c developer_instructions=<{f}>")),
        Delivery::Grok => (None, format!("--rules <{f}>")),
        Delivery::Opencode => (Some(format!("OPENCODE_CONFIG_CONTENT={}", shell_quote(&serde_json::json!({ "instructions": [f] }).to_string()))), String::new()),
        Delivery::Kimi => (None, format!("--agent-file <{f}>")),
    })
}

/// The agent's command with the model flag at the end, as `continue_line`
/// adds `--continue`. Flags the person put in the command stay. Without a
/// model, the plain command. An agent with no model flag refuses a model.
pub fn command(agent: &AgentInfo, model: Option<&str>) -> Result<String> {
    let cmd = agent.command.trim();
    let Some(model) = check_model(model)? else { return Ok(cmd.to_string()) };
    match model_flag(&agent.id).filter(|_| !agent.custom) {
        Some(flag) => Ok(format!("{cmd} {flag} {model}")),
        None => bail!("{} takes its model in its command. Change the command in Settings › Agents.", agent.id),
    }
}

/// The flag that lets the built-in Claude Code read Skiff's image cache
/// without a prompt, as argv words for the end of its start line. A path
/// the agent reads with a tool (a `skiff send` task, a later look) is
/// outside the project, and Claude Code asks or refuses. It is a flag on the
/// line: Skiff writes nothing into the agent's config. A command that has
/// `--settings` already gets `--add-dir=<cache>`, which also allows writes
/// in acceptEdits and auto mode. Codex reads the whole disk; other agents
/// get nothing.
pub fn cache_flags(agent: &AgentInfo, cache: &std::path::Path) -> Option<Vec<String>> {
    if agent.custom || agent.id != "claude" || !cache.is_absolute() {
        return None;
    }
    let dir = cache.to_string_lossy();
    let dir = dir.trim_end_matches('/');
    let has_settings = agent.command.split_whitespace().any(|w| w == "--settings" || w.starts_with("--settings="));
    if has_settings {
        return Some(vec![format!("--add-dir={dir}")]);
    }
    // `//` marks an absolute path in a Claude Code rule.
    let rule = serde_json::json!({ "permissions": { "allow": [format!("Read(/{dir}/**)")] } });
    Some(vec!["--settings".into(), rule.to_string()])
}

/// [`cache_flags`] as text to type after a start line in a shell: each word
/// quoted, with a space in front. Empty when the agent gets none.
pub fn cache_flags_typed(agent: &AgentInfo, cache: &std::path::Path) -> String {
    cache_flags(agent, cache)
        .unwrap_or_default()
        .iter()
        .map(|w| format!(" {}", crate::shell::quote_word(w)))
        .collect()
}

/// One task type as Settings and `skiff agents --json` show it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskType {
    #[serde(rename = "type")]
    pub name: String,
    pub agent: String,
    /// `None`: the agent's default. Always `None` for an agent that takes its
    /// model in its command.
    pub model: Option<String>,
    /// The type's instruction file, there or not: `types/<type>.md` in the
    /// agent's folder.
    pub file: PathBuf,
    /// The file's size in bytes when it has instructions. `None`: none.
    pub instructions: Option<u64>,
    /// The command line the type starts. `None` when the agent is gone, or
    /// when it cannot take the file.
    pub command: Option<String>,
    /// Why the row does not start what it says.
    pub error: Option<String>,
}

/// Every row with its built command, in file order. `dir` is the agent's folder.
pub fn resolve(rows: &TaskTypes, agents: &[AgentInfo], dir: &Path) -> Vec<TaskType> {
    rows.0
        .iter()
        .map(|(name, row)| {
            let text = text(dir, name);
            let mut t = TaskType {
                name: name.clone(),
                agent: row.agent.clone(),
                model: None,
                file: file(dir, name),
                instructions: text.as_ref().map(|t| t.len() as u64),
                command: None,
                error: None,
            };
            let Some(a) = agents.iter().find(|a| a.id == row.agent) else {
                t.error = Some(format!("There is no agent {}. Pick another agent in {SETTINGS}.", row.agent));
                return t;
            };
            let takes_model = !a.custom && model_flag(&a.id).is_some();
            match check_model(row.model.as_deref()) {
                Ok(m) if takes_model => t.model = m.map(str::to_string),
                Ok(None) => {}
                Ok(Some(_)) => t.error = Some(format!("{} takes its model in its command. The model here is not used.", a.id)),
                Err(e) => {
                    t.error = Some(e.to_string());
                    return t;
                }
            }
            t.command = command(a, t.model.as_deref()).ok();
            // A file the agent cannot take starts nothing: the row says why. No file: the plain command.
            if t.instructions.is_some() {
                match instruction_preview(a, dir, name) {
                    Ok((env, words)) => t.command = t.command.map(|c| [env.unwrap_or_default(), c, words].into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")),
                    Err(e) => {
                        t.command = None;
                        t.error = Some(e.to_string());
                    }
                }
            }
            t
        })
        .collect()
}

/// What `skiff new --type` starts.
#[derive(Debug)]
pub struct Start<'a> {
    pub agent: &'a AgentInfo,
    /// The command line as argv words: the command split on spaces, the
    /// model, then the file's words, each one word whatever it holds.
    pub words: Vec<String>,
    /// Variables for the program, as opencode's file.
    pub env: Vec<(String, String)>,
}

/// What `skiff new --type` starts: the row's agent, its command line and
/// its instruction file. A `model` replaces the row's model for this
/// session. A file the agent cannot take starts nothing. `dir` is the
/// agent's folder.
pub fn start<'a>(rows: &TaskTypes, agents: &'a [AgentInfo], name: &str, model: Option<&str>, dir: &Path) -> Result<Start<'a>> {
    let Some(row) = rows.get(name) else {
        let known: Vec<&str> = rows.0.iter().map(|(k, _)| k.as_str()).collect();
        let list = if known.is_empty() { "There are none.".to_string() } else { format!("Known: {}.", known.join(", ")) };
        bail!("There is no task type {name}. {list} Add it in {SETTINGS}.");
    };
    let Some(a) = agents.iter().find(|a| a.id == row.agent) else {
        bail!("Task type {name} starts agent {}, and there is no such agent. Pick another agent in {SETTINGS}.", row.agent);
    };
    let takes_model = !a.custom && model_flag(&a.id).is_some();
    // A model in the row of an agent that takes none is not used; a model the person names is refused.
    let model = match check_model(model)? {
        Some(m) => Some(m),
        None if takes_model => check_model(row.model.as_deref())?,
        None => None,
    };
    let mut words: Vec<String> = command(a, model)?.split_whitespace().map(String::from).collect();
    let mut env = Vec::new();
    if let Some(text) = text(dir, name) {
        let added = instruction_args(a, dir, name, &text).map_err(|e| anyhow::anyhow!("Task type {name}: {e}"))?;
        words.extend(added.words);
        env = added.env;
    }
    Ok(Start { agent: a, words, env })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TaskRow;

    fn agent(id: &str, command: &str, custom: bool) -> AgentInfo {
        AgentInfo {
            id: id.into(),
            command: command.into(),
            default_command: if custom { String::new() } else { id.into() },
            custom,
            installed: true,
            enabled: true,
        }
    }

    fn rows(list: &[(&str, &str, Option<&str>)]) -> TaskTypes {
        TaskTypes(list.iter().map(|(k, a, m)| (k.to_string(), TaskRow { agent: a.to_string(), model: m.map(str::to_string) })).collect())
    }

    /// An agent folder of its own, with a space in its name.
    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("skiff-types {tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir.join(TYPES)).unwrap();
        std::fs::write(file(dir, name), text).unwrap();
    }

    #[test]
    fn a_missing_empty_or_stub_file_is_none() {
        let dir = folder("none");
        assert_eq!(text(&dir, "review"), None);
        // Edit writes the stub, and it alone is none.
        let f = edit_file(&dir, "review").unwrap();
        assert_eq!(f, dir.join("types/review.md"));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "# review\n");
        assert_eq!(text(&dir, "review"), None);
        write(&dir, "review", "  \n");
        assert_eq!(text(&dir, "review"), None);
        // A line below the stub counts. Edit keeps a file that is there.
        write(&dir, "review", "# review\nReview only.\n");
        assert_eq!(text(&dir, "review").as_deref(), Some("# review\nReview only.\n"));
        edit_file(&dir, "review").unwrap();
        assert!(text(&dir, "review").is_some());
        assert!(edit_file(&dir, "../x").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_rename_moves_the_file_and_its_stub() {
        let dir = folder("rename");
        rename_file(&dir, "none", "other").unwrap();
        write(&dir, "review", "Review only.");
        rename_file(&dir, "review", "code-review").unwrap();
        assert!(!file(&dir, "review").exists());
        assert_eq!(std::fs::read_to_string(file(&dir, "code-review")).unwrap(), "Review only.");
        edit_file(&dir, "spec").unwrap();
        rename_file(&dir, "spec", "design").unwrap();
        assert_eq!(std::fs::read_to_string(file(&dir, "design")).unwrap(), "# design\n");
        // A file kept from a removed type is not overwritten.
        let e = rename_file(&dir, "design", "code-review").unwrap_err().to_string();
        assert!(e.contains("types/code-review.md exists"), "{e}");
        assert!(file(&dir, "design").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_agent_takes_the_file_its_own_way() {
        let dir = folder("args");
        let text = "Review only. Say \"done\".\nkey = 1\n";
        let path = file(&dir, "review").display().to_string();
        let args = |id: &str| instruction_args(&agent(id, id, false), &dir, "review", text);
        assert_eq!(args("claude").unwrap(), Added { words: vec!["--append-system-prompt-file".into(), path.clone()], env: vec![] });
        // codex: the text, quoted as a TOML string, so a line like `key = 1` stays text.
        let codex = args("codex").unwrap().words;
        assert_eq!(codex[0], "-c");
        let value = codex[1].strip_prefix("developer_instructions=").unwrap();
        let parsed: toml::Value = toml::from_str(&format!("v = {value}")).unwrap();
        assert_eq!(parsed["v"].as_str(), Some(text));
        assert_eq!(args("grok").unwrap().words, ["--rules", text]);
        // opencode: the full path in its inline config, in the variable.
        let open = args("opencode").unwrap();
        assert!(open.words.is_empty());
        assert_eq!(open.env, [("OPENCODE_CONFIG_CONTENT".to_string(), serde_json::json!({ "instructions": [path] }).to_string())]);
        // kimi: a wrapper in the cache that keeps kimi's own prompt.
        let kimi = args("kimi").unwrap().words;
        let wrapper = kimi_wrapper(&dir, "review");
        assert_eq!(kimi, ["--agent-file".to_string(), wrapper.display().to_string()]);
        assert_eq!(std::fs::read_to_string(&wrapper).unwrap(), format!("---\nname: review\ndescription: Skiff task type review\n---\n${{base_prompt}}\n\n{text}"));
        // No way: the row's error, never a write or a message.
        for a in [agent("gemini", "agy", false), agent("hermes", "hermes", false), agent("claude", "claude", true)] {
            let e = instruction_args(&a, &dir, "review", text).unwrap_err().to_string();
            assert_eq!(e, format!("{} cannot take an instruction file. Empty the file or change the agent.", a.id));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_and_grok_refuse_a_file_too_long_for_the_command_line() {
        let dir = folder("long");
        let long = "x".repeat(MAX_INLINE + 1);
        for id in ["codex", "grok"] {
            assert!(instruction_args(&agent(id, id, false), &dir, "review", &long).unwrap_err().to_string().contains("keep it under"), "{id}");
        }
        // claude and opencode read the file themselves.
        assert!(instruction_args(&agent("claude", "claude", false), &dir, "review", &long).is_ok());
        assert!(instruction_args(&agent("opencode", "opencode", false), &dir, "review", &long).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn start_adds_the_file_after_the_model() {
        let dir = folder("start");
        let path = file(&dir, "review").display().to_string();
        let agents = [agent("claude", "claude --dangerously-skip-permissions", false), agent("codex", "codex", false), agent("gemini", "agy", false), agent("opencode", "opencode", false)];
        let r = rows(&[("review", "claude", Some("opus")), ("impl", "codex", None), ("fast", "gemini", None), ("plain", "gemini", None), ("ui", "opencode", None)]);
        for name in ["review", "impl", "fast", "ui"] {
            write(&dir, name, "Review only.");
        }
        let s = start(&r, &agents, "review", None, &dir).unwrap();
        // The path is one word, spaces and all.
        assert_eq!(s.words, ["claude", "--dangerously-skip-permissions", "--model", "opus", "--append-system-prompt-file", path.as_str()]);
        assert!(s.env.is_empty());
        assert_eq!(start(&r, &agents, "review", Some("sonnet"), &dir).unwrap().words[3], "sonnet");
        assert_eq!(start(&r, &agents, "impl", None, &dir).unwrap().words, ["codex", "-c", "developer_instructions=\"Review only.\""]);
        let ui = start(&r, &agents, "ui", None, &dir).unwrap();
        assert_eq!((ui.words.as_slice(), ui.env[0].0.as_str()), (["opencode".to_string()].as_slice(), "OPENCODE_CONFIG_CONTENT"));
        let e = start(&r, &agents, "fast", None, &dir).unwrap_err().to_string();
        assert_eq!(e, "Task type fast: gemini cannot take an instruction file. Empty the file or change the agent.");
        // No file: the plain command, for any agent.
        assert_eq!(start(&r, &agents, "plain", None, &dir).unwrap().words, ["agy"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resolve_shows_the_flag_and_the_file_or_why_not() {
        let dir = folder("resolve");
        let agents = [agent("claude", "claude", false), agent("aider", "aider", true), agent("opencode", "opencode", false), agent("codex", "codex", false)];
        let r = rows(&[("review", "claude", Some("opus")), ("docs", "aider", None), ("ui", "opencode", None), ("impl", "codex", None), ("none", "aider", None)]);
        for name in ["review", "docs", "ui", "impl"] {
            write(&dir, name, "Review only.");
        }
        edit_file(&dir, "none").unwrap();
        let t = resolve(&r, &agents, &dir);
        let f = |name: &str| tilde(&file(&dir, name));
        assert_eq!((t[0].instructions, t[0].file.clone()), (Some(12), file(&dir, "review")));
        assert_eq!(t[0].command.as_deref(), Some(format!("claude --model opus --append-system-prompt-file '{}'", f("review")).as_str()));
        assert!(t[0].error.is_none());
        assert!(t[1].command.is_none());
        assert_eq!(t[1].error.as_deref(), Some("aider cannot take an instruction file. Empty the file or change the agent."));
        assert_eq!(t[2].command.as_deref(), Some(format!("OPENCODE_CONFIG_CONTENT='{{\"instructions\":[\"{}\"]}}' opencode", f("ui")).as_str()));
        // The preview shows the file, never its text.
        assert_eq!(t[3].command.as_deref(), Some(format!("codex -c developer_instructions=<{}>", f("impl")).as_str()));
        // The stub alone: none, and the plain command, even for a custom agent.
        assert_eq!((t[4].instructions, t[4].command.as_deref(), t[4].error.as_deref()), (None, Some("aider"), None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cache_flags_read_the_cache_for_claude_only() {
        let cache = std::path::Path::new("/home/me/skiff-agent/cache");
        let claude = agent("claude", "claude", false);
        assert_eq!(
            cache_flags(&claude, cache).unwrap(),
            ["--settings", r#"{"permissions":{"allow":["Read(//home/me/skiff-agent/cache/**)"]}}"#]
        );
        assert_eq!(
            cache_flags_typed(&claude, cache),
            r#" --settings '{"permissions":{"allow":["Read(//home/me/skiff-agent/cache/**)"]}}'"#
        );
        let set = agent("claude", "claude --settings x.json", false);
        assert_eq!(cache_flags(&set, cache).unwrap(), ["--add-dir=/home/me/skiff-agent/cache"]);
        assert_eq!(cache_flags_typed(&set, cache), " --add-dir=/home/me/skiff-agent/cache");
        assert!(cache_flags(&agent("codex", "codex", false), cache).is_none());
        assert!(cache_flags(&agent("claude", "claude", true), cache).is_none());
        assert_eq!(cache_flags_typed(&agent("grok", "grok", false), cache), "");
    }

    #[test]
    fn names_are_slugs() {
        for ok in ["review", "code-review", "t2", "2fa"] {
            assert_eq!(check_name(ok).unwrap(), ok);
        }
        assert_eq!(check_name(" spec ").unwrap(), "spec");
        for bad in ["", "Review", "-x", "a b", "a_b", "ünï", "a.b"] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn commands_add_the_flag_at_the_end() {
        let claude = agent("claude", "claude --dangerously-skip-permissions", false);
        assert_eq!(command(&claude, Some("opus")).unwrap(), "claude --dangerously-skip-permissions --model opus");
        assert_eq!(command(&claude, Some("  ")).unwrap(), "claude --dangerously-skip-permissions");
        assert_eq!(command(&agent("codex", "codex", false), Some("gpt-5-codex")).unwrap(), "codex -m gpt-5-codex");
        assert_eq!(command(&agent("grok", "grok", false), Some("grok-4")).unwrap(), "grok -m grok-4");
        assert!(command(&agent("aider", "aider --model sonnet", true), Some("x")).is_err());
        assert!(command(&agent("gemini", "agy", false), Some("x")).is_err());
        assert!(command(&claude, Some("two words")).is_err());
    }

    #[test]
    fn resolves_each_row_in_order() {
        let agents = [agent("claude", "claude", false), agent("aider", "aider --model sonnet", true)];
        let t = resolve(&rows(&[("spec", "claude", Some("opus")), ("docs", "aider", Some("x")), ("test", "claude", None), ("old", "gone", None)]), &agents, Path::new("/no/such/skiff-agent"));
        assert_eq!(t.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["spec", "docs", "test", "old"]);
        assert_eq!(t[0].command.as_deref(), Some("claude --model opus"));
        assert_eq!((t[1].model.as_deref(), t[1].command.as_deref()), (None, Some("aider --model sonnet")));
        assert!(t[1].error.is_some());
        assert_eq!((t[2].command.as_deref(), t[2].error.as_deref()), (Some("claude"), None));
        assert!(t[3].command.is_none() && t[3].error.as_deref().unwrap().contains("no agent gone"));
    }

    #[test]
    fn start_takes_the_row_and_a_named_model_wins() {
        let agents = [agent("claude", "claude", false), agent("aider", "aider", true)];
        let r = rows(&[("review", "claude", Some("opus")), ("plain", "claude", None), ("docs", "aider", Some("x"))]);
        let nowhere = Path::new("/no/such/skiff-agent");
        assert_eq!(start(&r, &agents, "review", None, nowhere).unwrap().words.join(" "), "claude --model opus");
        assert_eq!(start(&r, &agents, "review", Some("sonnet"), nowhere).unwrap().words.join(" "), "claude --model sonnet");
        assert_eq!(start(&r, &agents, "plain", None, nowhere).unwrap().words.join(" "), "claude");
        assert_eq!(start(&r, &agents, "docs", None, nowhere).unwrap().words.join(" "), "aider");
        assert!(start(&r, &agents, "docs", Some("y"), nowhere).is_err());
        let e = start(&r, &agents, "nope", None, nowhere).unwrap_err().to_string();
        assert!(e.contains("no task type nope") && e.contains(SETTINGS) && e.contains("review, plain, docs"), "{e}");
    }
}

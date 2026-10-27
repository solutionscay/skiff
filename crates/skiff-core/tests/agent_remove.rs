use skiff_core::{config, project};

// Its own test binary: SKIFF_CONFIG is process-wide.
#[test]
fn an_agent_goes_only_when_nothing_starts_it() {
    let dir = std::env::temp_dir().join(format!("skiff-agent-remove-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("projects.toml");
    std::env::set_var("SKIFF_CONFIG", &file);
    std::fs::write(
        &file,
        "[skiff_agent]\nname = \"Mate\"\nagent = \"claude\"\nproject_agent = \"codex\"\n\n[skiff_agent.tasks]\nweb-research = { agent = \"grok\" }\nreview = { agent = \"claude\" }\n\n[agents.commands]\naider = \"aider --model sonnet\"\n\n[[project]]\nname = \"gesher\"\npath = \"/tmp/gesher\"\nskipper_agent = \"claude\"\n",
    )
    .unwrap();
    let cfg = config::load().unwrap();
    assert_eq!(project::agent_uses(&cfg, "claude"), ["Mate", "project gesher", "task type review"]);
    assert_eq!(project::used_by(&cfg, "claude").as_deref(), Some("Used by Mate and 2 more"));
    assert_eq!(project::used_by(&cfg, "codex").as_deref(), Some("Used by Project and fleet Skippers"));
    assert_eq!(project::used_by(&cfg, "grok").as_deref(), Some("Used by task type web-research"));
    assert_eq!(project::used_by(&cfg, "hermes"), None);

    // The list carries the line, and a used agent stays.
    let used = |id: &str| project::agents().into_iter().find(|a| a.id == id).and_then(|a| a.used_by);
    assert_eq!(used("grok").as_deref(), Some("Used by task type web-research"));
    let e = config::remove_agent("grok").unwrap_err().to_string();
    assert_eq!(e, "Used by task type web-research. grok stays.");
    assert!(project::agents().iter().any(|a| a.id == "grok"));

    // A default and a custom agent with no use both go; the default is recorded.
    config::remove_agent("hermes").unwrap();
    config::remove_agent("aider").unwrap();
    let ids: Vec<String> = project::agents().into_iter().map(|a| a.id).collect();
    assert!(!ids.iter().any(|i| i == "hermes" || i == "aider"), "{ids:?}");
    assert_eq!(config::load().unwrap().agents.removed, ["hermes"]);
    // Restore defaults brings hermes back; the custom agent stays gone.
    config::restore_agents().unwrap();
    let ids: Vec<String> = project::agents().into_iter().map(|a| a.id).collect();
    assert!(ids.iter().any(|i| i == "hermes") && !ids.iter().any(|i| i == "aider"), "{ids:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

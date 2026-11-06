//! `skiff skill path`, `skiff skipper template` and the warnings of `skiff
//! status`, from the binary, against a private config and socket.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

const SKIFF: &str = env!("CARGO_BIN_EXE_skiff");

struct World {
    root: PathBuf,
    home: PathBuf,
    top: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A private install, with a config path that holds a space.
fn world(name: &str) -> World {
    let root = std::env::temp_dir().join(format!("skiff-templates-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(home.join("code/app")).unwrap();
    let home = home.canonicalize().unwrap();
    let top = home.join("agent");
    let config = home.join("Application Support/projects.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, format!("[[project]]\nname = \"app\"\npath = \"{0}/code/app\"\n\n[skiff_agent]\nfolder = \"{1}\"\n", home.display(), top.display())).unwrap();
    World { root, home, top }
}

impl World {
    fn skiff(&self, args: &[&str]) -> Output {
        Command::new(SKIFF)
            .arg("--socket")
            .arg(self.root.join("skiffd.sock"))
            .args(args)
            .env("HOME", &self.home)
            .env("SKIFF_CONFIG", self.home.join("Application Support/projects.toml"))
            .env_remove("SKIFF_SESSION")
            .env_remove("SKIFF_LEDGER")
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let out = self.skiff(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }
}

/// Every path under `dir`, with its bytes and its time.
fn tree(dir: &Path, out: &mut BTreeMap<PathBuf, (Vec<u8>, Option<SystemTime>)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let m = p.symlink_metadata().unwrap();
        let bytes = if m.is_file() { std::fs::read(&p).unwrap() } else { Vec::new() };
        out.insert(p.clone(), (bytes, m.modified().ok()));
        if m.is_dir() {
            tree(&p, out);
        }
    }
}

#[test]
fn skill_path_prints_a_file_that_exists_with_no_daemon() {
    let w = world("skill");
    let path = w.ok(&["skill", "path"]);
    let path = Path::new(path.trim_end());
    assert!(path.is_file() && path.ends_with("skills/skiff-cli/SKILL.md"), "{}", path.display());
    assert!(path.to_string_lossy().contains("Application Support"));
    // A stale copy comes back, and the path stays the same.
    std::fs::write(path, "old").unwrap();
    assert_eq!(w.ok(&["skill", "path"]).trim_end(), path.to_str().unwrap());
    assert!(std::fs::read_to_string(path).unwrap().starts_with("---\nname: skiff-cli"));
}

#[test]
fn status_warns_only_when_the_template_changed_and_writes_nothing() {
    let w = world("status");
    w.ok(&["skipper", "template", "apply", "agents"]);
    w.ok(&["skipper", "template", "apply", "project:app"]);
    // Plain edited: the marker has the current template's hash.
    std::fs::write(w.top.join("projects/app/SKIPPER.md"), "mine\n").unwrap();
    // No hash and other text.
    std::fs::write(w.top.join("AGENTS.md"), "my base\n").unwrap();
    let marker = std::fs::read_to_string(w.top.join(".skiff-agent")).unwrap();
    let kept: String = marker.lines().filter(|l| !l.ends_with(" AGENTS.md")).map(|l| format!("{l}\n")).collect();
    std::fs::write(w.top.join(".skiff-agent"), kept).unwrap();

    let mut before = BTreeMap::new();
    tree(&w.root, &mut before);
    let out = w.skiff(&["status"]);
    let err = String::from_utf8_lossy(&out.stderr);
    let warned: Vec<&str> = err.lines().filter(|l| l.contains("template diff")).collect();
    assert_eq!(warned, ["skiff: warning: AGENTS.md is edited and its template changed. Run: skiff skipper template diff agents"], "{err}");
    let json = String::from_utf8(w.skiff(&["status", "--json"]).stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["templates"], serde_json::json!([{ "file": "AGENTS.md", "scope": "agents", "command": "skiff skipper template diff agents" }]));
    let mut after = BTreeMap::new();
    tree(&w.root, &mut after);
    assert_eq!(before, after, "status wrote a file");
}

#[test]
fn a_file_status_cannot_read_does_not_hide_another_warning() {
    let w = world("unreadable");
    w.ok(&["skipper", "template", "apply", "agents"]);
    std::fs::write(w.top.join("AGENTS.md"), "my base\n").unwrap();
    std::fs::write(w.top.join(".skiff-agent"), "1\n").unwrap();
    // A folder in place of the file: a read fails for every user.
    std::fs::create_dir_all(w.top.join("projects/app/SKIPPER.md")).unwrap();
    let err = String::from_utf8(w.skiff(&["status"]).stderr).unwrap();
    assert!(err.contains("AGENTS.md is edited and its template changed"), "{err}");
    let json: serde_json::Value = serde_json::from_slice(&w.skiff(&["status", "--json"]).stdout).unwrap();
    assert_eq!(json["templates"][0]["file"], "AGENTS.md", "{json}");
}

#[test]
fn apply_writes_the_template_and_keeps_the_old_file_in_bak() {
    let w = world("apply");
    w.ok(&["skipper", "template", "apply", "project:app"]);
    let file = w.top.join("projects/app/SKIPPER.md");
    let template = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, "mine\n").unwrap();

    let diff = w.ok(&["skipper", "template", "diff", "app"]);
    assert!(diff.contains("-mine\n") && diff.contains("+# Project Skipper: app\n"), "{diff}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine\n", "diff changes nothing");
    assert!(w.ok(&["skipper", "template", "ls"]).lines().any(|l| l == "project:app  edited  projects/app/SKIPPER.md"));

    w.ok(&["skipper", "template", "apply", "project:app"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), template);
    let bak = w.top.join("projects/app/SKIPPER.md.bak");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "mine\n");
    // A second apply changes nothing, so the old file stays in .bak.
    assert!(w.ok(&["skipper", "template", "apply", "project:app"]).contains("Nothing changed"));
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "mine\n");
}

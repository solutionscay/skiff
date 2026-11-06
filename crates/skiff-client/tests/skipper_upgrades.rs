//! What a new install and an upgrade do to the Skipper instruction files.
//! HOME and SKIFF_CONFIG are process-wide, so the tests take turns.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use skiff_client::skipper::{self, Scope, Status};
use skiff_core::config::{self, Config};

static TURN: Mutex<()> = Mutex::new(());

/// The `AGENTS.md` template at 7fc4374b, from before Skiff recorded hashes.
const PAST: &str = include_str!("fixtures/AGENTS-7fc4374b.md.tmpl");

struct World {
    _turn: MutexGuard<'static, ()>,
    root: PathBuf,
    top: PathBuf,
    cfg: Config,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A private install: a config path with a space, as on macOS, and a project
/// in a fleet. Nothing is set up yet.
fn world(name: &str) -> World {
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!("skiff-upgrades-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(home.join("code/app")).unwrap();
    std::fs::create_dir_all(home.join("code/api")).unwrap();
    let home = home.canonicalize().unwrap();
    let config = home.join("Library/Application Support/Skiff/projects.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("SKIFF_CONFIG", &config);
    std::env::remove_var("HERMES_HOME");
    std::fs::write(
        &config,
        format!("[[project]]\nname = \"api\"\npath = \"{0}/code/api\"\nfleet = \"Harbor\"\n\n[[project]]\nname = \"app\"\npath = \"{0}/code/app\"\n", home.display()),
    )
    .unwrap();
    let cfg = config::load().unwrap();
    let top = skipper::folder(&cfg).unwrap();
    World { _turn: turn, root, top, cfg }
}

impl World {
    /// Sets the folder up, then gives AGENTS.md this text and this marker.
    fn agents_file(&self, text: &str, marker: Option<&str>) {
        skipper::ensure(&self.cfg).unwrap();
        std::fs::write(self.top.join("AGENTS.md"), text).unwrap();
        if let Some(m) = marker {
            std::fs::write(self.top.join(".skiff-agent"), m).unwrap();
        }
    }
    fn agents(&self) -> String {
        std::fs::read_to_string(self.top.join("AGENTS.md")).unwrap()
    }
    fn marker(&self) -> String {
        std::fs::read_to_string(self.top.join(".skiff-agent")).unwrap()
    }
    fn status(&self) -> Status {
        skipper::file_status(&self.cfg, &Scope::Skipper).unwrap().status
    }
}

/// The path in the first backticks of a first message.
fn named_skill(msg: &str) -> PathBuf {
    PathBuf::from(msg.split('`').nth(1).unwrap())
}

#[test]
fn a_new_project_skippers_first_message_names_a_skill_file_that_exists() {
    let w = world("project");
    let msg = skipper::first_message(&w.cfg, &Scope::Project("app".into())).unwrap();
    let skill = named_skill(&msg);
    assert!(skill.is_file() && skill.ends_with("SKILL.md"), "{msg}");
    assert!(skill.to_string_lossy().contains("Application Support"), "{msg}");
    assert_eq!(skill, skipper::skill_file().unwrap());
    assert!(msg.contains(&format!("`{}/projects/app/SKIPPER.md`", w.top.display())), "{msg}");
    assert!(!msg.contains('\n') && !msg.contains("Tell the user"), "{msg}");
}

#[test]
fn a_new_fleet_skippers_first_message_names_a_skill_file_that_exists() {
    let w = world("fleet");
    let msg = skipper::first_message(&w.cfg, &Scope::Fleet("Harbor".into())).unwrap();
    let skill = named_skill(&msg);
    assert!(skill.is_file() && skill.to_string_lossy().contains("Application Support"), "{msg}");
    assert!(msg.contains(&format!("`{}/fleets/Harbor/SKIPPER.md`", w.top.display())), "{msg}");
}

#[test]
fn an_unedited_file_with_a_hash_is_rewritten() {
    let w = world("hash");
    w.agents_file("the template of an older Skiff\n", None);
    let mut lines: Vec<String> = w.marker().lines().map(str::to_string).collect();
    // The hash of the older text, as that Skiff recorded it. FNV-1a, as in the marker.
    let h = "the template of an older Skiff\n".bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    lines.retain(|l| !l.ends_with(" AGENTS.md"));
    lines.push(format!("{h:016x} AGENTS.md"));
    std::fs::write(w.top.join(".skiff-agent"), lines.join("\n") + "\n").unwrap();
    assert_eq!(w.status(), Status::Template);
    skipper::ensure(&w.cfg).unwrap();
    assert!(w.agents().contains("skiff skill path"));
    assert_eq!(w.status(), Status::Template);
}

#[test]
fn a_past_template_with_no_hash_is_rewritten_and_its_hash_recorded() {
    let w = world("past");
    w.agents_file(&PAST.replace("{{name}}", "Skipper"), Some("1\n"));
    assert_eq!(w.status(), Status::Template);
    skipper::ensure(&w.cfg).unwrap();
    assert!(w.agents().contains("## Names") && w.agents().contains("You are Admiral"));
    assert!(w.marker().lines().any(|l| l.ends_with(" AGENTS.md")));
    // An upgrade adds nothing to the message.
    assert!(!skipper::first_message(&w.cfg, &Scope::Project("app".into())).unwrap().contains("Tell the user"));
}

#[test]
fn other_text_with_no_hash_stays_and_warns() {
    let w = world("other");
    // A custom name counts as other text too.
    let mine = PAST.replace("{{name}}", "Bosun");
    w.agents_file(&mine, Some("1\n"));
    skipper::ensure(&w.cfg).unwrap();
    assert_eq!(w.agents(), mine);
    assert_eq!(w.status(), Status::EditedTemplateChanged);
    let msg = skipper::first_message(&w.cfg, &Scope::Project("app".into())).unwrap();
    assert!(msg.contains("AGENTS.md (`skiff skipper template diff agents`)"), "{msg}");
}

#[test]
fn an_edited_file_with_a_hash_stays_and_warns_only_when_the_template_changed() {
    let w = world("edited");
    w.agents_file("mine\n", None);
    assert_eq!(w.status(), Status::Edited);
    assert!(!skipper::first_message(&w.cfg, &Scope::Project("app".into())).unwrap().contains("Tell the user"));
    // A hash from an older template: the same file now warns, and stays.
    let marker = w.marker().lines().map(|l| if l.ends_with(" AGENTS.md") { "0000000000000000 AGENTS.md".to_string() } else { l.to_string() }).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::write(w.top.join(".skiff-agent"), marker).unwrap();
    skipper::ensure(&w.cfg).unwrap();
    assert_eq!(w.agents(), "mine\n");
    assert_eq!(w.status(), Status::EditedTemplateChanged);
    let msg = skipper::first_message(&w.cfg, &Scope::Project("app".into())).unwrap();
    assert!(msg.contains("Tell the user") && msg.contains("Do not apply"), "{msg}");
}

#[test]
fn a_reset_of_an_unedited_older_agents_file_keeps_its_bytes_in_bak() {
    let w = world("reset");
    let bak = w.top.join("AGENTS.md.bak");
    // A past template with no hash: setup alone would upgrade it.
    let past = PAST.replace("{{name}}", "Skipper");
    w.agents_file(&past, Some("1\n"));
    skipper::reset_file(&w.cfg, &Scope::Skipper).unwrap();
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), past);
    assert!(w.agents().contains("skiff skill path"));
    // An older text with its hash: setup alone would upgrade it too.
    let older = "the template of an older Skiff\n";
    let h = older.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    w.agents_file(older, Some(&format!("1\n{h:016x} AGENTS.md\n")));
    skipper::reset_file(&w.cfg, &Scope::Skipper).unwrap();
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), older);
    assert!(w.agents().contains("skiff skill path"));
}

/// A SKIPPER.md must not need a section of an AGENTS.md of another age.
#[test]
fn a_skipper_template_names_no_section_of_another_file() {
    for (name, text) in [
        ("project", include_str!("../agent-template/SKIPPER-project.md.tmpl")),
        ("fleet", include_str!("../agent-template/SKIPPER-fleet.md.tmpl")),
    ] {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for (at, _) in flat.match_indices("under ") {
            let next = flat[at + "under ".len()..].chars().next().unwrap_or(' ');
            assert!(!next.is_uppercase() && next != '"', "the {name} SKIPPER template names a section: {}", &flat[at..]);
        }
    }
    // Each first message says to do the First steps.
    assert!(include_str!("../agent-template/AGENTS.md.tmpl").contains("## First steps"));
}

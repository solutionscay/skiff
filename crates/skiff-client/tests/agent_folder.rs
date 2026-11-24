//! Settings › Disk use › Folder › Move… (#288): the refusals, then a move.
//! Its own test binary: HOME and SKIFF_CONFIG are process-wide.

use rusqlite::Connection;
use skiff_client::{agent_folder, skipper};
use skiff_core::config::{self, SkiffAgentKey};

#[test]
fn a_move_refuses_a_full_target_and_open_work_then_moves_whole() {
    let root = std::env::temp_dir().join(format!("skiff-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("SKIFF_CONFIG", root.join("config/projects.toml"));
    std::env::remove_var("HERMES_HOME");
    std::env::remove_var("SKIFF_LEDGER");
    let from = home.join("skiff-agent");
    config::set_skiff_agent(SkiffAgentKey::Folder(Some(&from))).unwrap();
    let cfg = config::load().unwrap();
    skipper::make_folder(&cfg).unwrap();
    let db = Connection::open(from.join("ledger.db")).unwrap();
    db.execute_batch("CREATE TABLE tasks (id INTEGER PRIMARY KEY, parent TEXT, closed_at TEXT); INSERT INTO tasks (parent) VALUES (NULL);").unwrap();
    drop(db);

    let full = home.join("disk/full");
    std::fs::create_dir_all(&full).unwrap();
    std::fs::write(full.join("x"), "x").unwrap();
    let e = agent_folder::move_folder(&cfg, &full, &[]).unwrap_err().to_string();
    assert!(e.contains("not empty"), "{e}");

    let to = home.join("disk/skiff-agent");
    let e = agent_folder::move_folder(&cfg, &to, &[]).unwrap_err().to_string();
    assert!(e.contains("open in the ledger"), "{e}");
    assert!(from.join("ledger.db").is_file() && !to.exists(), "a refusal changes nothing");

    Connection::open(from.join("ledger.db")).unwrap().execute("UPDATE tasks SET closed_at = 'x'", []).unwrap();
    let m = agent_folder::move_folder(&cfg, &to, &[]).unwrap();
    assert_eq!(m.to, to);
    assert!(!from.exists(), "nothing stays at the old path");
    assert!(to.join("ledger.db").is_file());
    assert_eq!(config::load().unwrap().skiff_agent.folder.as_deref(), Some(to.as_path()));
    let _ = std::fs::remove_dir_all(&root);
}

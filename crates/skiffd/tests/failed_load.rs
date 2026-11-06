//! A failed workspace load: the daemon says so, for as long as it runs, and it
//! keeps the file. A missing file is a healthy load with no sessions.
#![cfg(target_os = "linux")]

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use skiff_client::Client;
use skiff_core::{protocol::WorkspaceLoad, session::SessionSpec};

mod common;

struct Daemon {
    child: Child,
    dir: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if common::stop(&mut self.child, &self.dir) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("skiff-failed-load-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start(dir: &Path) -> (Daemon, Client) {
    let child = Command::new(env!("CARGO_BIN_EXE_skiffd"))
        .env("SKIFF_SOCKET", dir.join("skiffd.sock"))
        .env("SKIFF_STATE", dir.join("workspace.json"))
        .env("SKIFF_CONFIG", dir.join("projects.toml"))
        .env("SKIFF_LEDGER", dir.join("ledger.db"))
        .env("SKIFF_SHELL_ENV", "0")
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let daemon = Daemon { child, dir: dir.to_path_buf() };
    for _ in 0..200 {
        if let Ok(c) = Client::connect(&dir.join("skiffd.sock")).await {
            return (daemon, c);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("skiffd did not start");
}

#[tokio::test]
async fn a_failed_load_gives_no_full_list_even_with_new_sessions_and_keeps_the_file() {
    let dir = scratch("broken");
    std::fs::write(dir.join("workspace.json"), "{ not json").unwrap();
    let (_daemon, c) = start(&dir).await;
    let list = c.session_list().await.unwrap();
    assert!(list.sessions.is_empty());
    assert!(matches!(&list.workspace_load, Some(WorkspaceLoad::Failed { error }) if error.contains("workspace.json")));
    assert!(list.gap().is_some());
    // A new session changes nothing, and the saver leaves the file alone.
    c.create_session(SessionSpec { cwd: Some(dir.clone()), ..Default::default() }).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let list = c.session_list().await.unwrap();
    assert_eq!(list.sessions.len(), 1);
    assert!(list.gap().is_some());
    assert_eq!(std::fs::read_to_string(dir.join("workspace.json")).unwrap(), "{ not json");
}

#[tokio::test]
async fn a_missing_file_is_a_healthy_load_and_its_empty_list_is_full() {
    let dir = scratch("missing");
    let (_daemon, c) = start(&dir).await;
    let list = c.session_list().await.unwrap();
    assert!(list.sessions.is_empty());
    assert_eq!(list.workspace_load, Some(WorkspaceLoad::Loaded));
    assert!(list.gap().is_none());
}

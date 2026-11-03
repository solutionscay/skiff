//! `skiff ledger add --task -` and `handoff SCOPE -` read the task from
//! stdin, as `skiff send -` does (#246): the task is never the text `-`.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SKIFF: &str = env!("CARGO_BIN_EXE_skiff");

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("skiff-ledger-stdin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Dir(d)
}

/// `skiff ledger ARGS` with `input` on stdin, against a private ledger,
/// config and home, and a socket where no daemon runs.
fn ledger(d: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(SKIFF)
        .args(["--socket", &d.join("none.sock").display().to_string(), "ledger", "--person"])
        .args(args)
        .env("SKIFF_LEDGER", d.join("ledger.db"))
        .env("SKIFF_CONFIG", d.join("projects.toml"))
        .env("HOME", d)
        .env("XDG_CONFIG_HOME", d.join("xdg"))
        .env_remove("SKIFF_SESSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn a_dash_task_is_read_from_stdin() {
    let d = scratch("add");
    let out = ledger(&d.0, &["add", "--plan", "--task", "-", "--title", "From stdin"], "Ship the release notes.\nThen tag it.\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let uid = stdout.lines().find_map(|l| l.strip_prefix("uid ")).unwrap().to_string();
    let show = ledger(&d.0, &["show", &uid, "--json"], "");
    let v: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(v["task"], "Ship the release notes.\nThen tag it.");
    // No send yet: no delivery.
    assert_eq!(v["deliveries"], serde_json::json!([]));
}

#[test]
fn an_empty_stdin_is_refused_and_adds_nothing() {
    let d = scratch("empty");
    let out = ledger(&d.0, &["add", "--plan", "--task", "-"], "  \n");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("stdin is empty"));
    let ls = ledger(&d.0, &["ls", "--all", "--json"], "");
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&ls.stdout).unwrap(), serde_json::json!([]));
}

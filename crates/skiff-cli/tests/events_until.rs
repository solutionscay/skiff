//! `skiff events --until` prints the first matching line and exits, also
//! when the session goes quiet after it (#298). With no match, --timeout
//! ends it with 1.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const SKIFF: &str = env!("CARGO_BIN_EXE_skiff");

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A daemon with one idle session `a`. It answers a ping and the session
/// list. After the list it sends `events`, then stays connected and quiet.
fn daemon(name: &str, events: Vec<Value>) -> (Dir, PathBuf) {
    let d = std::env::temp_dir().join(format!("skiff-events-until-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let sock = d.join("skiffd.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            let events = events.clone();
            std::thread::spawn(move || {
                let mut wr = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { return };
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let id = &request["id"];
                    if request["cmd"] == "ping" {
                        let pong = json!({ "id": id, "type": "pong", "version": skiff_core::VERSION, "protocol": skiff_core::PROTOCOL });
                        writeln!(wr, "{pong}").unwrap();
                        continue;
                    }
                    let session = json!({
                        "id": "a", "label": "a", "role": "agent", "cwd": "/", "command": "claude", "args": [],
                        "state": "idle", "cols": 80, "rows": 24, "pid": null, "exit_code": null,
                    });
                    writeln!(wr, "{}", json!({ "id": id, "type": "sessions", "sessions": [session] })).unwrap();
                    // After the CLI drops the events queued before its list.
                    std::thread::sleep(Duration::from_millis(300));
                    for e in &events {
                        writeln!(wr, "{e}").unwrap();
                    }
                }
            });
        }
    });
    (Dir(d), sock)
}

fn events(sock: &Path, args: &[&str]) -> (Output, Duration) {
    let t = Instant::now();
    let out = Command::new(SKIFF)
        .args(["--socket", &sock.display().to_string(), "events", "--json"])
        .args(args)
        .env_remove("SKIFF_SESSION")
        .output()
        .unwrap();
    (out, t.elapsed())
}

#[test]
fn until_prints_the_first_match_and_exits_or_times_out_with_1() {
    let working = json!({ "event": "state", "session": "a", "state": "working" });
    let turn = json!({ "event": "turn_ended", "session": "a" });

    let (_d, sock) = daemon("match", vec![working.clone(), turn]);
    let (out, took) = events(&sock, &["a", "--until", "exit,turn", "--timeout", "20"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(lines.len(), 2, "{stdout}");
    assert_eq!(lines[1]["event"], "turn");
    assert!(took < Duration::from_secs(10), "took {took:?}");

    let (_d, sock) = daemon("timeout", vec![working]);
    let (out, _) = events(&sock, &["a", "--until", "turn", "--timeout", "1"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
}

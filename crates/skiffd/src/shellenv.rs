//! The login shell's environment.
//!
//! A desktop launcher starts the app with the session's environment, which
//! skips `~/.bashrc` and friends. PATH then misses `~/.local/bin`, nvm and
//! cargo, so agents are not found, and EDITOR is unset, so an agent's
//! "open in editor" key (Ctrl+G in Claude Code) opens the wrong editor.
//! Started from a terminal, the environment is already complete.

use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MARK: &str = "__SKIFF_ENV__";
const TIMEOUT: Duration = Duration::from_secs(3);

/// Variables that describe the probe shell itself, not the user's setup.
const SKIP: &[&str] = &["_", "PWD", "OLDPWD", "SHLVL", "TERM", "COLORTERM", "PS1"];

/// Copies the login shell's variables into this process. Call it before any
/// thread starts: it changes the process environment.
pub fn import() {
    if std::env::var_os("TERM").is_some() || std::env::var("SKIFF_SHELL_ENV").as_deref() == Ok("0") {
        return;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    match probe(&shell) {
        Some(vars) => {
            for (k, v) in vars {
                if !SKIP.contains(&k.as_str()) {
                    std::env::set_var(k, v);
                }
            }
        }
        None => eprintln!("skiffd: could not read the environment of {shell}; using the launcher's"),
    }
}

fn probe(shell: &str) -> Option<Vec<(String, String)>> {
    // -i reads the rc file (bash reads ~/.bashrc only when interactive), -l the profile.
    // The mark skips anything the rc files print.
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("printf '{MARK}'; env -0")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let buf = reader.join().ok()?;
    let text = String::from_utf8_lossy(&buf);
    let body = &text[text.find(MARK)? + MARK.len()..];
    let vars: Vec<_> = body
        .split('\0')
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| !k.is_empty() && !k.contains('\n'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    vars.iter().any(|(k, _)| k == "PATH").then_some(vars)
}

//! Which app opens a file, from `[open]` in the config. Skiff never shows a
//! file itself: it runs the user's command, or hands the file to the OS.
//! The app runs every command in the peek, hidden. A terminal tool shows
//! there; an app with a window of its own never does.

use crate::shell::shell_quote;

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

use crate::config::Open;

/// How a file or a diff opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// This shell script runs: a terminal tool or an app.
    Run(String),
    /// No command is set: the OS default app opens the file.
    Default,
}

/// The `[open]` row for `file`, if one applies. Markdown and HTML go by
/// extension; any other text file uses `text`.
fn key_for(file: &Path) -> Option<&'static str> {
    if file.is_dir() { return None; }
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "md" | "markdown" => Some("markdown"),
        "html" | "htm" => Some("html"),
        _ if is_text(file) => Some("text"),
        _ => None,
    }
}

/// The command set for `key`, trimmed, if any.
fn command(open: &Open, key: &str) -> Option<String> {
    let cmd = match key {
        "diff" => open.diff.as_ref(),
        "text" => open.text.as_ref(),
        "markdown" => open.markdown.as_ref(),
        "html" => open.html.as_ref(),
        _ => None,
    };
    cmd.map(|c| c.trim().to_string()).filter(|c| !c.is_empty())
}

/// The `[open]` command for `file`, or `None` for the OS default app.
pub fn command_for(file: &Path, open: &Open) -> Option<String> {
    command(open, key_for(file)?)
}

/// Selection may start only commands explicitly configured for the peek.
pub fn previews_file(file: &Path, open: &Open) -> bool {
    key_for(file).is_some_and(|key| open.peek.iter().any(|k| k == key) && command(open, key).is_some())
}

/// How `file` opens. `{path}` in the command becomes the file; without it,
/// the file goes at the end.
pub fn plan_file(file: &Path, open: &Open) -> Plan {
    let Some(key) = key_for(file) else { return Plan::Default };
    let Some(cmd) = command(open, key) else { return Plan::Default };
    Plan::Run(file_script(&cmd, file))
}

/// How the diff opens: the `diff` command, else `git diff`, with `{target}`
/// for what to compare. See [`crate::git::diff_script`].
pub fn plan_diff(dir: &Path, file: Option<&str>, open: &Open) -> Plan {
    Plan::Run(crate::git::diff_script(dir, file, open.diff.as_deref()))
}

fn file_script(command: &str, file: &Path) -> String {
    let path = shell_quote(&file.to_string_lossy());
    if command.contains("{path}") {
        command.replace("{path}", &path)
    } else {
        format!("{command} {path}")
    }
}

const OPENER: &str = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };

/// Hands `file` to the OS default app. Fails when the OS has no app for it.
pub fn open_default(file: &Path) -> Result<()> {
    launch(OPENER, file)
}

/// Hands a web address to the default browser. Only http and https go.
pub fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("Not a web address: {url}");
    }
    start(&format!("{OPENER} {}", shell_quote(url)), &std::env::temp_dir())
}

/// No NUL byte in the first 8 KB: the test git uses.
fn is_text(file: &Path) -> bool {
    let Ok(f) = std::fs::File::open(file) else { return false };
    let mut buf = Vec::with_capacity(8192);
    if f.take(8192).read_to_end(&mut buf).is_err() {
        return false;
    }
    !buf.contains(&0)
}

/// Runs `command` on `file` in the file's folder. `{path}` becomes the file;
/// without it, the file goes at the end.
pub fn launch(command: &str, file: &Path) -> Result<()> {
    start(&file_script(command, file), file.parent().unwrap_or(Path::new("/")))
}

/// Runs a shell script in `dir` and lets it go. Does not wait for the app to
/// close, but reports a script that fails at once, such as a misspelled program.
pub fn start(script: &str, dir: &Path) -> Result<()> {
    let mut child = Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("run the open command")?;
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(1) {
        if let Some(status) = child.try_wait()? {
            // Many apps hand the file to a running window and exit 0 at once.
            if status.success() {
                return Ok(());
            }
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            bail!("{script} failed: {}", err.trim());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Drain stderr so a chatty app never blocks on a full pipe, then reap it.
    std::thread::spawn(move || {
        if let Some(mut e) = child.stderr.take() {
            let _ = std::io::copy(&mut e, &mut std::io::sink());
        }
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_row_by_kind() {
        let dir = std::env::temp_dir().join(format!("skiff-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in [("a.md", &b"# hi"[..]), ("b.HTML", b"<p>"), ("c.rs", b"fn x"), ("d.png", b"\x89PNG\0\0")] {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let open = Open {
            peek: vec!["text".into()],
            diff: None,
            text: Some("code".into()),
            markdown: Some("typora".into()),
            html: Some("  ".into()),
        };
        let text = plan_file(&dir.join("c.rs"), &open);
        let markdown = plan_file(&dir.join("a.md"), &open);
        let default = plan_file(&dir.join("d.png"), &open);
        let got = ["a.md", "b.HTML", "c.rs", "d.png"].map(|n| command_for(&dir.join(n), &open));
        let previews = ["a.md", "b.HTML", "c.rs", "d.png"].map(|n| previews_file(&dir.join(n), &open));
        let folder = dir.join("folder.md");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(plan_file(&folder, &open), Plan::Default);
        assert!(!previews_file(&folder, &open));
        let ok = launch("true", &dir.join("a.md")).is_ok() && launch("exit 3", &dir.join("a.md")).is_err();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(got, [Some("typora".into()), None, Some("code".into()), None]);
        assert_eq!(previews, [false, false, true, false]);
        assert!(ok);
        assert!(matches!(text, Plan::Run(s) if s.starts_with("code ")));
        assert!(matches!(markdown, Plan::Run(s) if s.starts_with("typora ")));
        assert_eq!(default, Plan::Default);
    }
}

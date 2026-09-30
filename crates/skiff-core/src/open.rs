//! Which app opens a file, from `[open]` in the config. Skiff never shows a
//! file itself: it runs the user's command, or hands the file to the OS.

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

use crate::config::Open;

/// The `[open]` command for `file`, or `None` for the OS default app.
/// Markdown and HTML go by extension; any other text file uses `text`.
pub fn command_for(file: &Path, open: &Open) -> Option<String> {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let cmd = match ext.as_str() {
        "md" | "markdown" => open.markdown.as_ref(),
        "html" | "htm" => open.html.as_ref(),
        _ if is_text(file) => open.text.as_ref(),
        _ => None,
    };
    cmd.map(|c| c.trim().to_string()).filter(|c| !c.is_empty())
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

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Runs `command` on `file` in the file's folder. `{path}` becomes the file;
/// without it, the file goes at the end. Does not wait for the app to close,
/// but reports a command that fails at once, such as a misspelled program.
pub fn launch(command: &str, file: &Path) -> Result<()> {
    let path = shell_quote(&file.to_string_lossy());
    let script = if command.contains("{path}") {
        command.replace("{path}", &path)
    } else {
        format!("{command} {path}")
    };
    let mut child = Command::new("sh")
        .args(["-c", &script])
        .current_dir(file.parent().unwrap_or(Path::new("/")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("run the open command")?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
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
            diff: None,
            text: Some("code".into()),
            markdown: Some("typora".into()),
            html: Some("  ".into()),
        };
        let got = ["a.md", "b.HTML", "c.rs", "d.png"].map(|n| command_for(&dir.join(n), &open));
        let ok = launch("true", &dir.join("a.md")).is_ok() && launch("exit 3", &dir.join("a.md")).is_err();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(got, [Some("typora".into()), None, Some("code".into()), None]);
        assert!(ok);
    }
}

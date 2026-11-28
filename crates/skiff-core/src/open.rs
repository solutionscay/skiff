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

/// The extensions the `image` row opens, SVG included.
const IMAGE_EXTS: [&str; 11] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "avif", "svg"];

/// The `[open]` row for `file`, if one applies. Markdown and HTML go by
/// extension; any other text file uses `text`.
fn key_for(file: &Path) -> Option<&'static str> {
    if file.is_dir() { return None; }
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "md" | "markdown" => Some("markdown"),
        "html" | "htm" => Some("html"),
        e if IMAGE_EXTS.contains(&e) => Some("image"),
        _ if is_text(file) => Some("text"),
        _ => None,
    }
}

/// The command set for `key`, trimmed, if any. A program in
/// `[open.commands]` that reads the file wins, as a line with `{path}`.
fn command(open: &Open, key: &str) -> Option<String> {
    if let Some(c) = open.commands.get(key).filter(|c| key != "diff" && c.input == crate::config::OpenCommand::PATH) {
        return Some(c.line("{path}"));
    }
    let cmd = match key {
        "diff" => open.diff.as_ref(),
        "text" => open.text.as_ref(),
        "markdown" => open.markdown.as_ref(),
        "html" => open.html.as_ref(),
        "image" => open.image.as_ref(),
        _ => None,
    };
    cmd.map(|c| c.trim().to_string()).filter(|c| !c.is_empty())
}

/// The `[open]` command for `file`, or `None` for the OS default app.
pub fn command_for(file: &Path, open: &Open) -> Option<String> {
    command(open, key_for(file)?)
}

/// Whether `key` opens in the peek on selection: the program's `opens`,
/// else the `peek` list.
pub fn in_peek(open: &Open, key: &str) -> bool {
    match open.commands.get(key).and_then(|c| c.opens.as_deref()) {
        Some(opens) => opens == "peek",
        None => open.peek.iter().any(|k| k == key),
    }
}

/// Selection may start only commands explicitly configured for the peek.
pub fn previews_file(file: &Path, open: &Open) -> bool {
    key_for(file).is_some_and(|key| key != "image" && in_peek(open, key) && command(open, key).is_some())
}

/// How `file` opens. `{path}` in the command becomes the file; without it,
/// the file goes at the end.
pub fn plan_file(file: &Path, open: &Open) -> Plan {
    let Some(key) = key_for(file) else { return Plan::Default };
    let Some(cmd) = command(open, key) else { return Plan::Default };
    Plan::Run(file_script(&cmd, file))
}

/// How `file` opens as Settings › Open with › Text files says, whatever its
/// extension, as an instruction file opens from Settings.
pub fn plan_text(file: &Path, open: &Open) -> Plan {
    match command(open, "text") {
        Some(cmd) => Plan::Run(file_script(&cmd, file)),
        None => Plan::Default,
    }
}

/// Text files open in the peek: the `text` row has a command and the peek.
pub fn previews_text(open: &Open) -> bool {
    in_peek(open, "text") && command(open, "text").is_some()
}

/// How a diff opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffPlan {
    /// Skiff runs Git and hands the patch to a renderer: [`Render`].
    Render(Render),
    /// This shell script runs: a command line from `[open] diff`, or `git diff`.
    Shell(String),
}

/// A diff that Skiff makes itself and hands to a renderer in a terminal.
/// `skiff open-with render` runs it as the peek's program: Git writes the
/// patch to a pipe, the renderer reads it on its standard input and keeps
/// the terminal, so its pager can read keys. No pager setting of Git
/// stands between them. Each list is a program and its words, in order.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Render {
    /// `git diff` and its words, with the target.
    pub producer: Vec<String>,
    /// Git's exit codes that are success: 1 too for `--no-index`, which
    /// exits 1 when the files differ.
    pub producer_ok: Vec<i32>,
    /// The renderer and its words.
    pub renderer: Vec<String>,
}

/// The exit codes of `skiff open-with render` that name a stage. Any other
/// code is the renderer's own.
pub mod render_exit {
    /// The renderer or Git is not on the PATH.
    pub const MISSING: i32 = 127;
    /// The renderer or Git is there but cannot start.
    pub const CANNOT_RUN: i32 = 126;
    /// Git could not make the diff.
    pub const PRODUCER: i32 = 90;
    /// The folder to run in is gone.
    pub const CWD: i32 = 91;
    /// A signal other than the user's stopped the renderer or its pager.
    pub const RENDERER_SIGNAL: i32 = 92;
    /// The user stopped it with Ctrl+C.
    pub const INTERRUPTED: i32 = 130;
}

/// The private OSC the runner prints last, `ESC ] 7750 ; CODE BEL`: all
/// its output is in the terminal once this comes.
pub const RENDER_DONE_OSC: u32 = 7750;

/// How the diff opens. `[open.commands.diff]` is a renderer that reads the
/// patch. A command line that pipes `git diff {target}` into one program,
/// or that sets Git's pager for one `git diff {target}`, runs the same way,
/// with its words. Any other command line runs in the shell, with
/// `{target}` for what to compare: see [`crate::git::diff_script`].
pub fn plan_diff(dir: &Path, file: Option<&str>, open: &Open) -> Result<DiffPlan> {
    let render = |git_opts: Vec<String>, renderer: Vec<String>| {
        let target = crate::git::diff_target(dir, file);
        let no_index = target.first().is_some_and(|w| w == "--no-index");
        let mut producer: Vec<String> = ["git", "--no-pager", "-c", "core.quotePath=false", "diff", "--no-ext-diff"].map(String::from).into();
        producer.extend(git_opts);
        producer.extend(target);
        DiffPlan::Render(Render { producer, producer_ok: if no_index { vec![0, 1] } else { vec![0] }, renderer })
    };
    if let Some(c) = open.commands.get("diff") {
        if c.input == crate::config::OpenCommand::TARGET {
            return Ok(DiffPlan::Shell(crate::git::diff_script(dir, file, Some(&c.line("{target}")))));
        }
        if c.input != crate::config::OpenCommand::PATCH {
            bail!("Open with › Diffs: Skiff cannot give {} the input {:?}", c.program, c.input);
        }
        return Ok(render(Vec::new(), [&[c.program.trim().to_string()][..], &c.args].concat()));
    }
    let line = open.diff.as_deref().map(str::trim).filter(|c| !c.is_empty());
    if let Some((git_opts, renderer)) = line.and_then(patch_pipeline) {
        return Ok(render(git_opts, renderer));
    }
    Ok(DiffPlan::Shell(crate::git::diff_script(dir, file, line)))
}

/// A command line that only feeds `git diff` to one renderer, as its words:
/// `git diff OPTS {target} | PROGRAM ARGS`, or
/// `git -c core.pager=PROGRAM diff OPTS {target}`. OPTS are options of `git
/// diff`. Anything else for the shell (a second pipe, a redirect, a
/// variable, a glob) leaves it to the shell: `None`.
pub fn patch_pipeline(line: &str) -> Option<(Vec<String>, Vec<String>)> {
    let words = shell_words(line)?;
    let pipe = words.iter().position(|w| w.is_none());
    let words: Vec<String> = words.into_iter().map(|w| w.unwrap_or_default()).collect();
    let (git, renderer) = match pipe {
        Some(i) => (&words[..i], Some(words[i + 1..].to_vec())),
        None => (&words[..], None),
    };
    if renderer.as_ref().is_some_and(|r| r.is_empty() || r.contains(&String::new())) {
        return None;
    }
    let (pager, rest) = match git {
        [g, c, kv, rest @ ..] if g == "git" && c == "-c" && kv.starts_with("core.pager=") => (Some(kv["core.pager=".len()..].to_string()), rest),
        [g, rest @ ..] if g == "git" => (None, rest),
        _ => return None,
    };
    let [diff, opts @ .., target] = rest else { return None };
    if diff != "diff" || target != "{target}" || !opts.iter().all(|o| o.starts_with('-') && !o.contains("{target}")) {
        return None;
    }
    let renderer = match (pager, renderer) {
        (None, Some(r)) => r,
        (Some(p), None) => p.split_whitespace().map(String::from).collect(),
        _ => return None,
    };
    if renderer.is_empty() || renderer.iter().any(|w| w.contains("{target}")) {
        return None;
    }
    Some((opts.to_vec(), renderer))
}

/// A command line as words, with `None` for a `|`. Single and double quotes
/// group. `None` for anything else the shell would expand or redirect.
fn shell_words(line: &str) -> Option<Vec<Option<String>>> {
    let mut out = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if let Some(w) = word.take() {
                    out.push(Some(w));
                }
            }
            '|' => {
                if let Some(w) = word.take() {
                    out.push(Some(w));
                }
                // One pipe only, and not `||`.
                if out.iter().any(Option::is_none) || out.is_empty() {
                    return None;
                }
                out.push(None);
            }
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => w.push(c),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '"' => break,
                        '$' | '`' | '\\' => return None,
                        c => w.push(c),
                    }
                }
            }
            c if ";&<>()$`\\*?[#~\n".contains(c) => return None,
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    if let Some(w) = word {
        out.push(Some(w));
    }
    // A `{` or `}` is only allowed as `{target}`.
    if out.iter().flatten().any(|w| (w.contains('{') || w.contains('}')) && w != "{target}") {
        return None;
    }
    Some(out)
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
            image: None,
            commands: Default::default(),
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

    /// #303: a saved command line that only feeds `git diff {target}` to one
    /// renderer runs as Skiff's own Git and renderer, with its words: the
    /// pipeline from #249 and the old Delta preset. Anything more for the
    /// shell stays a shell command. A program in `[open.commands]` wins.
    #[test]
    fn a_patch_pipeline_runs_without_the_shell() {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            patch_pipeline("git diff --color=always {target} | delta --paging=never"),
            Some((words(&["--color=always"]), words(&["delta", "--paging=never"])))
        );
        assert_eq!(patch_pipeline("git -c core.pager=delta diff {target}"), Some((vec![], words(&["delta"]))));
        assert_eq!(patch_pipeline("git -c 'core.pager=delta -s' diff {target}"), Some((vec![], words(&["delta", "-s"]))));
        assert_eq!(patch_pipeline("git diff {target} | '/opt/my tools/delta' --side-by-side"), Some((vec![], words(&["/opt/my tools/delta", "--side-by-side"]))));
        for shell in [
            "git diff {target} | delta | cat",
            "git diff {target} | delta > out",
            "git diff {target} | delta $ARGS",
            "git diff {target} || delta",
            "git diff | delta",
            "git difftool --tool=meld {target}",
            "git -c diff.external=difft diff {target}",
            "git diff HEAD~1 {target} | delta",
            "git diff {target} |",
        ] {
            assert_eq!(patch_pipeline(shell), None, "{shell}");
        }
        let dir = std::env::temp_dir().join(format!("skiff-open-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut open = Open { diff: Some("git diff --color=always {target} | delta --paging=never".into()), ..Open::default() };
        let mapped = plan_diff(&dir, Some("f"), &open).unwrap();
        open.commands.insert("diff".into(), crate::config::OpenCommand { program: "rec".into(), args: vec!["-x".into()], input: "patch".into(), opens: None });
        let program = plan_diff(&dir, Some("f"), &open).unwrap();
        open.commands.clear();
        open.diff = Some("my-diff {target}".into());
        let shell = plan_diff(&dir, Some("f"), &open).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let DiffPlan::Render(mapped) = mapped else { panic!("{mapped:?}") };
        assert_eq!(mapped.renderer, words(&["delta", "--paging=never"]));
        assert!(mapped.producer.starts_with(&words(&["git", "--no-pager"])) && mapped.producer.contains(&"--color=always".to_string()));
        assert!(matches!(program, DiffPlan::Render(r) if r.renderer == words(&["rec", "-x"])));
        assert!(matches!(shell, DiffPlan::Shell(s) if s.starts_with("my-diff ")));
    }
}

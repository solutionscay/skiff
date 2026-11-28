//! `skiff open-with render`, the peek's program for a local diff (#303).
//! Plans come from `skiff_core::open::plan_diff`, as the app makes them. A
//! recording renderer checks what reaches its standard input; real delta
//! and less run in a PTY that is their controlling terminal, as in the peek.
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use skiff_core::config::{Open, OpenCommand};
use skiff_core::open::{plan_diff, render_exit, DiffPlan, Render};

const SKIFF: &str = env!("CARGO_BIN_EXE_skiff");
const DONE: &str = "\x1b]7750;";
/// less's prompt at the foot of a screen.
const LESS_PROMPT: &str = "\r\n:\x1b[K";

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(tag: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("skiff-render-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Dir(d.canonicalize().unwrap())
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_GLOBAL", "/dev/null").status().unwrap().success();
    assert!(ok, "git {args:?}");
}

fn repo(at: &Path) {
    std::fs::create_dir_all(at).unwrap();
    git(at, &["init", "-q", "-b", "main"]);
    git(at, &["config", "user.email", "t@t"]);
    git(at, &["config", "user.name", "t"]);
}

/// Every Git pager setting that could skip a renderer: none may. `PAGER`
/// stays unset: delta reads it to pick its own pager.
fn pager_overrides(c: &mut Command) -> &mut Command {
    c.env("GIT_PAGER", "cat").env_remove("PAGER").env("GIT_CONFIG_PARAMETERS", "'core.pager=cat' 'pager.diff=false'")
}

fn recorder(d: &Path) -> PathBuf {
    let rec = d.join("rec.sh");
    std::fs::write(&rec, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$REC.args\"\ncat > \"$REC\"\n").unwrap();
    std::fs::set_permissions(&rec, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    rec
}

fn plan(wt: &Path, file: Option<&str>, program: &Path, args: &[&str]) -> Render {
    let mut open = Open::default();
    open.commands.insert("diff".into(), OpenCommand { program: program.to_string_lossy().into(), args: args.iter().map(|a| a.to_string()).collect(), input: "patch".into(), opens: None });
    match plan_diff(wt, file, &open).unwrap() {
        DiffPlan::Render(r) => r,
        DiffPlan::Shell(s) => panic!("a renderer runs in the shell: {s}"),
    }
}

/// Runs a plan with pipes, as a recording renderer needs no terminal.
fn render(wt: &Path, plan: &Render, rec: &Path) -> (i32, String, String) {
    let out = pager_overrides(Command::new(SKIFF).args(["open-with", "render", &serde_json::to_string(plan).unwrap()]).current_dir(wt).env("REC", rec))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into(), String::from_utf8_lossy(&out.stderr).into())
}

/// Tracked, untracked, deleted and unborn-branch files, with names the
/// shell and Git's pathspecs would change, each reach the renderer as the
/// patch against the right base. Pager overrides change nothing.
#[test]
fn the_patch_reaches_the_renderer_for_each_kind_of_file() {
    let d = dir("kinds");
    let rec = recorder(&d.0);
    let out = d.0.join("patch");
    let wt = d.0.join("repo");
    repo(&wt);
    let names = ["a b", "it's \"q\"", "-dash", "st*r", "co:lon", "gone"];
    for n in names {
        std::fs::write(wt.join(n), "old\n").unwrap();
    }
    // `st*r` as a glob would also match this file.
    std::fs::write(wt.join("stXr"), "same\n").unwrap();
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "init"]);
    for n in &names[..5] {
        std::fs::write(wt.join(n), "new\n").unwrap();
    }
    std::fs::write(wt.join("stXr"), "changed too\n").unwrap();
    git(&wt, &["rm", "-q", "gone"]);
    std::fs::write(wt.join("new file"), "n\n").unwrap();

    for n in &names[..5] {
        let (code, stdout, err) = render(&wt, &plan(&wt, Some(n), &rec, &[]), &out);
        let patch = std::fs::read_to_string(&out).unwrap();
        assert_eq!(code, 0, "{n}: {err}");
        // Git quotes a name with `"` in the header: one file, by its name.
        assert!(patch.matches("diff --git").count() == 1 && patch.contains(&n.replace('"', "\\\"")) && patch.contains("-old\n+new"), "{n}: {patch}");
        assert!(!patch.contains("stXr"), "{n}: a literal pathspec matches one file: {patch}");
        assert!(stdout.ends_with(&format!("{DONE}0\x07")), "{stdout:?}");
    }
    let (code, _, err) = render(&wt, &plan(&wt, Some("gone"), &rec, &[]), &out);
    let patch = std::fs::read_to_string(&out).unwrap();
    assert!(code == 0 && patch.contains("deleted file mode") && patch.contains("-old"), "{err} {patch}");
    // Untracked: --no-index exits 1 when the files differ, which is success.
    let (code, _, err) = render(&wt, &plan(&wt, Some("new file"), &rec, &["--x"]), &out);
    let patch = std::fs::read_to_string(&out).unwrap();
    assert!(code == 0 && patch.contains("+++ b/new file") && patch.contains("+n"), "{err} {patch}");
    assert_eq!(std::fs::read_to_string(d.0.join("patch.args")).unwrap(), "--x\n", "the arguments reach the renderer as words");
    // The whole worktree leaves untracked files out.
    let (code, _, _) = render(&wt, &plan(&wt, None, &rec, &[]), &out);
    let patch = std::fs::read_to_string(&out).unwrap();
    assert!(code == 0 && patch.contains("a/a b") && patch.contains("a/gone") && !patch.contains("new file"), "{patch}");

    // No commit yet: a staged file compares with the empty tree, an untracked one with an empty file.
    let unborn = d.0.join("unborn");
    repo(&unborn);
    std::fs::write(unborn.join("staged"), "s\n").unwrap();
    git(&unborn, &["add", "staged"]);
    std::fs::write(unborn.join("loose"), "l\n").unwrap();
    for (file, want) in [(Some("staged"), "+++ b/staged"), (Some("loose"), "+++ b/loose"), (None, "+++ b/staged")] {
        let (code, _, err) = render(&unborn, &plan(&unborn, file, &rec, &[]), &out);
        let patch = std::fs::read_to_string(&out).unwrap();
        assert!(code == 0 && patch.contains(want), "{file:?}: {err} {patch}");
    }
}

/// A failed Git run, a missing tool and a folder that went away each end
/// with their own code. Git's error shows; the done mark comes last.
#[test]
fn each_failing_stage_has_its_code() {
    let d = dir("stages");
    let rec = recorder(&d.0);
    let out = d.0.join("patch");
    let wt = d.0.join("repo");
    repo(&wt);
    std::fs::write(wt.join("f"), "1\n").unwrap();
    git(&wt, &["add", "f"]);
    git(&wt, &["commit", "-qm", "i"]);
    let mut bad = plan(&wt, Some("f"), &rec, &[]);
    let at = bad.producer.iter().position(|w| w == "--").unwrap() - 1;
    bad.producer[at] = "no-such-ref".into();
    let (code, stdout, err) = render(&wt, &bad, &out);
    assert_eq!(code, render_exit::PRODUCER, "{err}");
    assert!(err.contains("no-such-ref"), "{err}");
    assert!(stdout.ends_with(&format!("{DONE}90\x07")), "{stdout:?}");

    let (code, _, err) = render(&wt, &plan(&wt, Some("f"), Path::new("no-such-renderer"), &[]), &out);
    assert_eq!((code, err.trim()), (render_exit::MISSING, "no-such-renderer: not found"));

    let gone = d.0.join("gone");
    std::fs::create_dir(&gone).unwrap();
    let json = serde_json::to_string(&plan(&wt, Some("f"), &rec, &[])).unwrap();
    let status = Command::new("sh")
        .args(["-c", "cd \"$1\" && rmdir \"$1\" && exec \"$2\" open-with render \"$3\"", "sh"])
        .arg(&gone)
        .arg(SKIFF)
        .arg(json)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(render_exit::CWD));
}

// ── In a PTY, as skiffd runs the peek ────────────────────────────────────

struct Pty {
    master: std::fs::File,
    child: std::process::Child,
    seen: Vec<u8>,
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

/// The runner in a new PTY session, its controlling terminal, as
/// portable_pty starts the peek's program. COLUMNS and LINES are set, as
/// skiffd sets them.
fn spawn(wt: &Path, plan: &Render, cols: u16, rows: u16, env: &[(&str, &str)]) -> Pty {
    let (mut m, mut s) = (0, 0);
    let ws = winsize(cols, rows);
    // SAFETY: openpty writes two fds.
    assert_eq!(unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) }, 0);
    let slave = unsafe { OwnedFd::from_raw_fd(s) };
    // The runner must not hold its own master, or a closed master never hangs it up.
    unsafe { libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC) };
    let mut c = Command::new(SKIFF);
    pager_overrides(c.args(["open-with", "render", &serde_json::to_string(plan).unwrap()]).current_dir(wt))
        .env("TERM", "xterm-256color")
        .env("COLUMNS", cols.to_string())
        .env("LINES", rows.to_string())
        .env_remove("LESS")
        .env_remove("DELTA_PAGER")
        .env_remove("BAT_PAGER")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave));
    for (k, v) in env {
        c.env(k, v);
    }
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let child = c.spawn().unwrap();
    let master = unsafe { std::fs::File::from_raw_fd(m) };
    unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    Pty { master, child, seen: Vec::new() }
}

/// A failed test still ends the runner, delta and less.
impl Drop for Pty {
    fn drop(&mut self) {
        unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

impl Pty {
    /// Reads until `want` is in the output since `from`, or panics.
    fn until(&mut self, from: usize, want: &str, secs: u64) -> String {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let text = String::from_utf8_lossy(&self.seen[from.min(self.seen.len())..]).into_owned();
            if text.contains(want) {
                return text;
            }
            assert!(Instant::now() < end, "no {want:?} in {text:?}");
            let mut buf = [0u8; 65536];
            match self.master.read(&mut buf) {
                Ok(n) if n > 0 => self.seen.extend_from_slice(&buf[..n]),
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        use std::io::Write;
        self.master.write_all(bytes).unwrap();
    }
    fn resize(&self, cols: u16, rows: u16) {
        let ws = winsize(cols, rows);
        unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
    }
    fn wait(&mut self, secs: u64) -> i32 {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            // Keep reading, so the runner never blocks on a full PTY.
            let mut buf = [0u8; 65536];
            if let Ok(n) = self.master.read(&mut buf) {
                self.seen.extend_from_slice(&buf[..n]);
            }
            if let Some(s) = self.child.try_wait().unwrap() {
                return s.code().unwrap_or(-1);
            }
            assert!(Instant::now() < end, "still running");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn have(tool: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {tool}")]).stdout(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// One repo with a short and a long change.
fn diffs(d: &Path) -> PathBuf {
    let wt = d.join("repo");
    repo(&wt);
    std::fs::write(wt.join("short file"), "a\n").unwrap();
    std::fs::write(wt.join("long"), (0..400).map(|i| format!("line {i}\n")).collect::<String>()).unwrap();
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "i"]);
    std::fs::write(wt.join("short file"), "b\n").unwrap();
    std::fs::write(wt.join("long"), (0..400).map(|i| format!("line {i} changed\n")).collect::<String>()).unwrap();
    wt
}

/// Real delta, with less as its pager: a short diff prints in color and
/// exits; a long one pages in less, which searches, takes a resize and
/// quits on `q`. Git's pager settings say `cat` throughout.
#[test]
fn delta_and_less_run_in_the_terminal() {
    if !have("delta") || !have("less") {
        eprintln!("skipped: delta and less are not both installed");
        return;
    }
    let d = dir("delta");
    let wt = diffs(&d.0);
    let delta = Path::new("delta");

    let mut short = spawn(&wt, &plan(&wt, Some("short file"), delta, &[]), 100, 30, &[]);
    let text = short.until(0, DONE, 10);
    assert_eq!(short.wait(5), 0);
    // Delta's own colors: 24-bit backgrounds on the changed lines.
    assert!(text.contains("\x1b[48;2;") && text.contains("short file"), "{text:?}");

    let mut long = spawn(&wt, &plan(&wt, Some("long"), delta, &[]), 100, 30, &[]);
    // less shows a screen and its prompt, and waits for keys.
    long.until(0, LESS_PROMPT, 10);
    assert!(long.child.try_wait().unwrap().is_none(), "less pages");
    let mark = long.seen.len();
    long.send(b"/line 350 changed\r");
    let found = long.until(mark, LESS_PROMPT, 10);
    assert!(found.contains("line 350 changed") && !found.contains("line 1 changed"), "the search moved the screen: {found:?}");
    let mark = long.seen.len();
    long.resize(100, 12);
    // less redraws for the new size: it reads the size from the terminal,
    // not from LINES. 11 lines and the prompt, where 29 were before.
    let redrawn = long.until(mark, LESS_PROMPT, 10);
    assert!(redrawn.matches(" changed").count() <= 11, "{redrawn:?}");
    long.send(b"q");
    let text = long.until(mark, DONE, 10);
    assert_eq!(long.wait(5), 0, "{text:?}");
}

/// The peek closing hangs up the runner: Git, delta and less all end and
/// are reaped, and nothing of the session is left.
#[test]
fn a_hang_up_ends_the_renderer_and_its_pager() {
    if !have("delta") || !have("less") {
        eprintln!("skipped: delta and less are not both installed");
        return;
    }
    let d = dir("hangup");
    let wt = diffs(&d.0);
    let mut p = spawn(&wt, &plan(&wt, Some("long"), Path::new("delta"), &[]), 100, 30, &[]);
    p.until(0, LESS_PROMPT, 10);
    let group = p.child.id() as libc::pid_t;
    unsafe { libc::kill(group, libc::SIGHUP) };
    p.wait(5);
    let end = Instant::now() + Duration::from_secs(3);
    // No process is left in the runner's group: delta and less are gone.
    while unsafe { libc::kill(-group, 0) } == 0 {
        assert!(Instant::now() < end, "a process of the peek still runs");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A renderer that is quiet for more than 3 s, then prints and exits: its
/// output is all in the terminal before the done mark.
#[test]
fn late_output_comes_before_the_done_mark() {
    let d = dir("late");
    let wt = diffs(&d.0);
    let slow = d.0.join("slow.sh");
    std::fs::write(&slow, "#!/bin/sh\ncat >/dev/null\nsleep 3.5\nprintf 'late output\\n'\n").unwrap();
    std::fs::set_permissions(&slow, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut p = spawn(&wt, &plan(&wt, Some("short file"), &slow, &[]), 80, 24, &[]);
    let text = p.until(0, DONE, 15);
    assert_eq!(p.wait(5), 0);
    let late = text.find("late output").expect("the late output");
    assert!(late < text.find(DONE).unwrap(), "{text:?}");
}

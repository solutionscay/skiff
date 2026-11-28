//! `skiff open-with render PLAN`: the peek's program for a local diff. The
//! app makes the plan ([`Render`]); skiffd runs this in the peek's PTY, in
//! its own environment.
//!
//! Git writes the patch to a pipe. The renderer reads the pipe on its
//! standard input and writes to the PTY, which stays its controlling
//! terminal, so a pager under it (less under delta) reads keys and gets the
//! terminal's size. COLUMNS and LINES go from their environment: less would
//! take them over the terminal's size, and a resize would not reach it.
//!
//! The exit code names the stage that failed ([`render_exit`]); Git's error
//! text shows after the renderer is done. SIGHUP or SIGTERM (the peek
//! closed) ends Git, the renderer and its pager, and waits for them. Last,
//! the private OSC [`RENDER_DONE_OSC`] tells the page that all output came.

use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use skiff_core::open::{render_exit, Render, RENDER_DONE_OSC};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Ctrl+C goes to the whole foreground group. Git and the renderer take
/// it; this process stays to report. A handler, not SIG_IGN, which the
/// children would inherit.
extern "C" fn on_interrupt(_: libc::c_int) {}

fn handle(sig: libc::c_int, f: extern "C" fn(libc::c_int)) {
    // SAFETY: installs an async-signal-safe handler; no memory is shared but an atomic.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = f as usize;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(sig, &action, std::ptr::null_mut());
    }
}

/// Runs the plan and returns the exit code. Prints the done mark last.
pub fn run(plan: &str) -> i32 {
    let code = match serde_json::from_str::<Render>(plan) {
        Ok(plan) => render(&plan),
        Err(e) => {
            eprintln!("skiff: not a render plan: {e}");
            2
        }
    };
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]{RENDER_DONE_OSC};{code}\x07");
    let _ = out.flush();
    code
}

/// `program` as the shell finds it: a path as given, a name on PATH.
fn find(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Path::new(program).exists().then(|| PathBuf::from(program));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

fn command(words: &[String]) -> Result<Command, i32> {
    let Some(name) = words.first().filter(|w| !w.is_empty()) else {
        eprintln!("skiff: the plan names no program");
        return Err(2);
    };
    let Some(path) = find(name) else {
        eprintln!("{name}: not found");
        return Err(render_exit::MISSING);
    };
    let mut c = Command::new(path);
    c.args(&words[1..]).env_remove("COLUMNS").env_remove("LINES");
    Ok(c)
}

fn spawn_failed(name: &str, e: &std::io::Error) -> i32 {
    eprintln!("{name}: {e}");
    if e.kind() == std::io::ErrorKind::NotFound { render_exit::MISSING } else { render_exit::CANNOT_RUN }
}

fn render(plan: &Render) -> i32 {
    if !std::env::current_dir().is_ok_and(|d| d.is_dir()) {
        eprintln!("skiff: the folder is gone");
        return render_exit::CWD;
    }
    handle(libc::SIGHUP, on_stop);
    handle(libc::SIGTERM, on_stop);
    handle(libc::SIGINT, on_interrupt);
    let (mut git, mut view) = match (command(&plan.producer), command(&plan.renderer)) {
        (Ok(g), Ok(v)) => (g, v),
        (Err(c), _) | (_, Err(c)) => return c,
    };
    let mut producer = match git.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => return spawn_failed(&plan.producer[0], &e),
    };
    let patch = producer.stdout.take().expect("piped");
    let mut errors = producer.stderr.take().expect("piped");
    let said = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = errors.read_to_end(&mut text);
        text
    });
    let mut renderer = match view.stdin(Stdio::from(patch)).spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = producer.kill();
            let _ = producer.wait();
            return spawn_failed(&plan.renderer[0], &e);
        }
    };
    let Some((git_status, view_status)) = wait_both(&mut producer, &mut renderer) else {
        return 128 + libc::SIGHUP;
    };
    let said = said.join().unwrap_or_default();
    let quit_early = matches!(git_status.signal(), Some(libc::SIGPIPE | libc::SIGINT));
    let git_ok = git_status.code().is_some_and(|c| plan.producer_ok.contains(&c)) || quit_early;
    if !git_ok {
        let _ = std::io::stderr().write_all(&said);
        if let Some(code) = git_status.code() {
            eprintln!("git: exit {code}");
        }
        return render_exit::PRODUCER;
    }
    match (view_status.code(), view_status.signal()) {
        (Some(c), _) => c,
        (None, Some(libc::SIGINT)) => render_exit::INTERRUPTED,
        (None, Some(s)) => {
            eprintln!("{}: signal {s}", plan.renderer[0]);
            render_exit::RENDERER_SIGNAL
        }
        _ => 0,
    }
}

/// Waits for Git and the renderer. `None` when the peek closed: both, and
/// the pager, were ended and reaped first.
fn wait_both(producer: &mut Child, renderer: &mut Child) -> Option<(ExitStatus, ExitStatus)> {
    let (mut git, mut view) = (None, None);
    // After the renderer exits, Git gets EPIPE at its next write; one that
    // does not write again is ended after this long.
    let mut view_done: Option<Instant> = None;
    loop {
        if STOP.load(Ordering::SeqCst) {
            stop(&mut [producer, renderer]);
            return None;
        }
        if git.is_none() {
            git = producer.try_wait().ok().flatten();
        }
        if view.is_none() {
            view = renderer.try_wait().ok().flatten();
            if view.is_some() {
                view_done = Some(Instant::now());
            }
        }
        match (git, view) {
            (Some(g), Some(v)) => return Some((g, v)),
            (None, Some(_)) if view_done.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) => {
                let _ = producer.kill();
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Ends the children and every process in this group (the pager too),
/// then reaps the children. Group-wide only when this process leads the
/// group, as the PTY's first process does.
fn stop(children: &mut [&mut Child]) {
    // SAFETY: plain signal calls.
    let leader = unsafe { libc::getpgrp() == libc::getpid() };
    for c in children.iter() {
        unsafe { libc::kill(c.id() as libc::pid_t, libc::SIGHUP) };
    }
    if leader {
        // This process gets it too; its handler only sets STOP again.
        unsafe { libc::killpg(0, libc::SIGHUP) };
    }
    let began = Instant::now();
    for c in children.iter_mut() {
        while c.try_wait().ok().flatten().is_none() {
            if began.elapsed() > Duration::from_secs(1) {
                let _ = c.kill();
                let _ = c.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

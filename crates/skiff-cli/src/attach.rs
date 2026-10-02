//! `skiff attach`: run a session in this terminal until Ctrl-\.

use std::{
    io::{Read, Write},
    sync::Mutex,
};

use anyhow::{bail, Context, Result};
use skiff_client::Client;
use skiff_core::protocol::Event;
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::{broadcast::error::RecvError, mpsc},
};

use crate::{print, resolve, sessions};

/// Ctrl-\ detaches.
const DETACH: u8 = 0x1c;
/// Ctrl-\ under the kitty keyboard protocol, if the session turned it on.
const DETACH_KITTY: &[u8] = b"\x1b[92;5u";

/// Undoes the modes a session can leave on, then leaves the alternate screen.
const RESET: &str = "\x1b[?2026l\x1b[0m\x1b[r\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\
\x1b[?1004l\x1b[?2004l\x1b[?1l\x1b>\x1b[?7h\x1b[?25h\x1b[<u\x1b[?1049l";

/// The terminal settings from before raw mode. Taken back on restore.
static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

/// Puts the terminal back as it was. Safe to call more than once.
fn restore() {
    let Some(t) = SAVED.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let mut out = std::io::stdout();
    let _ = out.write_all(RESET.as_bytes());
    let _ = out.flush();
    unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &t) };
}

/// Restores the terminal when dropped, so every return path does it.
struct Raw;

impl Raw {
    fn enter() -> Result<Self> {
        if unsafe { libc::isatty(0) } != 1 || unsafe { libc::isatty(1) } != 1 {
            bail!("attach needs a terminal on stdin and stdout");
        }
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(0, &mut t) } != 0 {
            return Err(std::io::Error::last_os_error()).context("read terminal settings");
        }
        *SAVED.lock().unwrap() = Some(t);
        // A panic aborts in release builds, so Drop alone is not enough.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            prev(info);
        }));
        let mut raw = t;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &raw) } != 0 {
            SAVED.lock().unwrap().take();
            return Err(std::io::Error::last_os_error()).context("set raw mode");
        }
        // Built first, so a failed write below still restores the terminal.
        let r = Raw;
        let mut out = std::io::stdout();
        out.write_all(b"\x1b[?1049h")?;
        out.flush()?;
        Ok(r)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        restore();
    }
}

/// Columns and rows of this terminal.
fn size() -> Option<(u16, u16)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0;
    (ok && ws.ws_col > 0 && ws.ws_row > 0).then_some((ws.ws_col, ws.ws_row))
}

/// A snapshot starts with a full reset (RIS). That would drop this terminal
/// out of the alternate screen and wipe its history, so clear the screen and
/// the modes instead.
fn snapshot_bytes(data: &[u8]) -> Vec<u8> {
    let body = data.strip_prefix(b"\x1bc").unwrap_or(data);
    let mut v = Vec::with_capacity(body.len() + 64);
    v.extend_from_slice(b"\x1b[0m\x1b[r\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1004l\x1b[?2004l\x1b[?1l\x1b>\x1b[?7h\x1b[?25h\x1b[H\x1b[2J");
    v.extend_from_slice(body);
    v
}

/// How the attach ended.
enum End {
    Detached,
    Exited(Option<i32>),
    Removed,
    DaemonGone,
    Signal,
}

pub async fn attach(c: &Client, arg: &str) -> Result<()> {
    let all = sessions::sorted(c).await?;
    let s = resolve::session(&all, arg)?;
    let id = s.id.clone();
    let name = resolve::display_name(s).to_string();
    if s.exit_code.is_some() {
        bail!("{name} has exited");
    }

    let mut events = c.events();
    let raw = Raw::enter()?;
    if let Some((cols, rows)) = size() {
        c.resize(&id, cols, rows).await?;
    }
    let mut out = c.output(&id);
    c.subscribe(&id).await?;

    // A plain thread: a blocking stdin read must not hold up runtime shutdown.
    let (keys_tx, mut keys) = mpsc::unbounded_channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if keys_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut winch = signal(SignalKind::window_change())?;
    // Handled, so the terminal is restored before the process ends.
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    let mut int = signal(SignalKind::interrupt())?;
    let mut quit = signal(SignalKind::quit())?;
    let mut stdout = std::io::stdout();

    let end = loop {
        tokio::select! {
            o = out.recv() => match o {
                Ok(Event::Snapshot { data, .. }) => {
                    stdout.write_all(&snapshot_bytes(&data))?;
                    stdout.flush()?;
                }
                Ok(Event::Output { data, .. }) => {
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    // Dropped chunks: redraw from a fresh snapshot.
                    out.clear();
                    c.subscribe(&id).await?;
                }
                Err(RecvError::Closed) => break End::DaemonGone,
            },
            k = keys.recv() => {
                let Some(k) = k else { break End::Detached };
                let cut = k.iter().position(|&b| b == DETACH).or_else(|| {
                    k.windows(DETACH_KITTY.len()).position(|w| w == DETACH_KITTY)
                });
                let send = &k[..cut.unwrap_or(k.len())];
                if !send.is_empty() {
                    c.write_now(&id, send.to_vec())?;
                }
                if cut.is_some() {
                    break End::Detached;
                }
            }
            _ = winch.recv() => {
                if let Some((cols, rows)) = size() {
                    c.resize(&id, cols, rows).await?;
                }
            }
            _ = term.recv() => break End::Signal,
            _ = hup.recv() => break End::Signal,
            _ = int.recv() => break End::Signal,
            _ = quit.recv() => break End::Signal,
            e = events.recv() => match e {
                Ok(Event::Exit { session, code }) if session == id => break End::Exited(code),
                Ok(Event::SessionRemoved { session }) if session == id => break End::Removed,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break End::DaemonGone,
            },
        }
    };

    if !matches!(end, End::DaemonGone) {
        let _ = c.unsubscribe(&id).await;
    }
    drop(raw);
    match end {
        End::Detached | End::Signal => eprintln!("detached from {name} ({})", print::short_id(&id)),
        End::Exited(Some(code)) => eprintln!("{name} exited with code {code}"),
        End::Exited(None) => eprintln!("{name} exited"),
        End::Removed => eprintln!("{name} was closed"),
        End::DaemonGone => bail!("skiffd closed the connection"),
    }
    Ok(())
}

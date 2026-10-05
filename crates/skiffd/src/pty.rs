//! The master side of a session's PTY, and its child by pid.
//!
//! `portable_pty` opens the PTY and starts the child. After that the daemon
//! keeps only the master fd and the pid, so a session started here and one
//! adopted from an older daemon (reload) run the same code. `portable_pty`
//! has no way to wrap an inherited fd.

use std::{
    fs::File,
    io::{self, Write},
    os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd},
};

use anyhow::{anyhow, Result};

pub struct Pty {
    fd: OwnedFd,
}

impl Pty {
    /// Takes over `master` with an fd of our own; `master` closes its fd on drop.
    pub fn from_master(master: &dyn portable_pty::MasterPty) -> Result<Self> {
        let fd = master.as_raw_fd().ok_or_else(|| anyhow!("PTY has no fd"))?;
        // SAFETY: `master` owns the fd and lives through this call.
        let fd = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()?;
        Ok(Self { fd })
    }

    /// Takes ownership of an fd inherited across exec. Sets close-on-exec
    /// again, so no later child inherits it.
    ///
    /// # Safety
    /// `fd` must be an open PTY master that nothing else in this process owns.
    pub unsafe fn adopt(fd: RawFd) -> Result<Self> {
        if libc::fcntl(fd, libc::F_GETFD) == -1 {
            return Err(io::Error::last_os_error().into());
        }
        let pty = Self { fd: OwnedFd::from_raw_fd(fd) };
        pty.set_inherit(false)?;
        Ok(pty)
    }

    pub fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// A second fd for the reader thread. It closes on exec.
    pub fn reader(&self) -> Result<File> {
        Ok(File::from(self.fd.try_clone()?))
    }

    /// A second fd for the writer thread. It closes on exec.
    pub fn writer(&self) -> Result<PtyWriter> {
        Ok(PtyWriter(File::from(self.fd.try_clone()?)))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: TIOCSWINSZ reads `size` and changes only the PTY.
        if unsafe { libc::ioctl(self.raw_fd(), libc::TIOCSWINSZ as _, &size as *const _) } != 0 {
            return Err(anyhow!("resize: {}", io::Error::last_os_error()));
        }
        Ok(())
    }

    /// The process group in front of the PTY.
    pub fn foreground_group(&self) -> Option<u32> {
        // SAFETY: reads the fd this PTY owns.
        match unsafe { libc::tcgetpgrp(self.raw_fd()) } {
            pid if pid > 0 => Some(pid as u32),
            _ => None,
        }
    }

    /// The program turned canonical input off: raw keys, as editors take them.
    pub fn raw_input(&self) -> bool {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: tcgetattr writes into `t` and only reads the fd.
        unsafe { libc::tcgetattr(self.raw_fd(), &mut t) == 0 && t.c_lflag & libc::ICANON == 0 }
    }

    /// Clears or sets close-on-exec. Clear only right before a reload exec.
    pub fn set_inherit(&self, inherit: bool) -> Result<()> {
        set_inherit(self.raw_fd(), inherit)
    }
}

pub fn set_inherit(fd: RawFd, inherit: bool) -> Result<()> {
    // SAFETY: F_GETFD and F_SETFD change only the fd flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return Err(io::Error::last_os_error().into());
        }
        let flags = if inherit { flags & !libc::FD_CLOEXEC } else { flags | libc::FD_CLOEXEC };
        if libc::fcntl(fd, libc::F_SETFD, flags) == -1 {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}

/// Input to the PTY. Like `portable_pty`'s writer, it ends the input with a
/// newline and EOF when dropped. A reload never drops it: exec ends the
/// process without running destructors, and a failed reload keeps it.
pub struct PtyWriter(File);

impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Drop for PtyWriter {
    fn drop(&mut self) {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: tcgetattr writes into `t` and only reads the fd.
        if unsafe { libc::tcgetattr(self.0.as_raw_fd(), &mut t) } == 0 {
            let eof = t.c_cc[libc::VEOF];
            if eof != 0 {
                let _ = self.0.write_all(&[b'\n', eof]);
            }
        }
    }
}

/// What `try_reap` found.
pub enum Reap {
    /// The child exited and is reaped now. Its code, or 1 after a signal.
    Exited(i32),
    Running,
    /// Not our child, or already reaped.
    Gone,
}

/// Reaps `pid` if it exited. Never waits, and never touches another child.
pub fn try_reap(pid: u32) -> Reap {
    let mut status = 0;
    // SAFETY: waitpid on one pid writes only `status`.
    match unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) } {
        0 => Reap::Running,
        -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => Reap::Running,
        -1 => Reap::Gone,
        _ if libc::WIFEXITED(status) => Reap::Exited(libc::WEXITSTATUS(status)),
        _ => Reap::Exited(1),
    }
}

/// SIGHUP, as `portable_pty` sends to stop a child.
pub fn hang_up(pid: u32) {
    // SAFETY: sends a signal; no memory is touched.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGHUP) };
}

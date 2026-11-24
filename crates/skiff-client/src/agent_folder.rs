//! Settings › Disk use › Folder › Move… (#288): move the agent's folder to
//! another place, such as another disk. The folder moves whole: nothing
//! stays at the old path.
//!
//! The ledger stores the paths of new rows relative to the folder (see
//! [`crate::ledger::stored`]), so they go with it. Rows from before keep
//! their full paths. `[skiff_agent] folder` in projects.toml is the only
//! full path to the folder.
//!
//! A move refuses while ledger work is open or an agent session works. It
//! ends the Skipper sessions first: they work in the folder, and an agent
//! files its conversation by folder path. The app, `skiff-host` and
//! `skiff skipper --move-to` share this code.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use skiff_core::{
    config::{self, Config, SkiffAgentKey},
    session::{Role, SessionInfo, SessionState},
};

use crate::{
    skipper::{self, real, MARKER},
    Client,
};

/// In a staging folder's name: `<target>.skiff-new.<pid>`. A move to
/// another disk copies into it, then renames it into place.
pub const STAGING: &str = ".skiff-new.";

/// The ledger's companion files. The copy to another disk takes the ledger
/// with SQLite, never as a file.
const LEDGER_SIDE: [&str; 3] = ["ledger.db-wal", "ledger.db-shm", "ledger.db-journal"];

/// What the folder at a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Nothing is there yet.
    New,
    /// An empty folder.
    Empty,
    /// A Skiff folder: it has the marker.
    Skiff,
    /// A file, or a folder with other files.
    Refused,
}

/// What a move did.
#[derive(Debug, Clone)]
pub struct Moved {
    pub from: PathBuf,
    pub to: PathBuf,
    /// A step after the move that did not work. The move stands.
    pub warning: Option<String>,
}

impl Moved {
    pub fn to_host(&self) -> skiff_host_api::SkipperFolderMoved {
        skiff_host_api::SkipperFolderMoved {
            from: self.from.display().to_string(),
            to: self.to.display().to_string(),
            warning: self.warning.clone(),
        }
    }
}

/// What is at `p`.
pub fn kind(p: &Path) -> Result<Kind> {
    Ok(match p.symlink_metadata() {
        Err(_) => Kind::New,
        Ok(m) if !m.is_dir() => Kind::Refused,
        Ok(_) if p.join(MARKER).exists() => Kind::Skiff,
        Ok(_) if std::fs::read_dir(p).with_context(|| format!("read {}", p.display()))?.next().is_none() => Kind::Empty,
        Ok(_) => Kind::Refused,
    })
}

/// A chosen folder as an absolute, canonical path. `~` is the home folder.
/// A relative path resolves against `base`: the home folder in the app, the
/// shell's working folder in the CLI. Links resolve now, never later.
pub fn resolve(input: &str, base: &Path, home: &Path) -> Result<PathBuf> {
    let s = input.trim();
    if s.is_empty() {
        bail!("Type a folder.");
    }
    let p = if s == "~" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(s).is_absolute() {
        PathBuf::from(s)
    } else {
        base.join(s)
    };
    if !p.is_absolute() {
        bail!("{} is not a full path.", p.display());
    }
    Ok(real(&p))
}

/// Bytes as the app shows them.
pub fn size(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n if n < 1024 * 1024 * 1024 => format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)),
        n => format!("{:.1} GB", n as f64 / (1024.0 * 1024.0 * 1024.0)),
    }
}

/// Moves the agent's folder to `to` (from [`resolve`]) on this Host: the
/// checks, the end of the Skipper sessions, the move ([`move_folder`]), then
/// the saved pane folders in skiffd. `c` is this Host's skiffd.
pub async fn run(c: &Client, to: &Path) -> Result<Moved> {
    let cfg = config::load()?;
    let all = c.list_sessions().await?;
    let from = real(&skipper::folder(&cfg)?);
    check(&cfg, &from, to, &all)?;
    for s in all.iter().filter(|s| (s.coordinator || s.skipper.is_some()) && s.state != SessionState::Done) {
        c.kill(&s.id).await.with_context(|| format!("end the Skipper session {}", s.id))?;
    }
    let to_path = to.to_path_buf();
    let mut moved = tokio::task::spawn_blocking(move || move_folder(&cfg, &to_path, &[])).await??;
    if let Err(e) = c.refolder(&moved.from, &moved.to).await {
        let line = format!("The panes keep their old folders: {e:#}");
        moved.warning = Some(moved.warning.map_or(line.clone(), |w| format!("{w} {line}")));
    }
    Ok(moved)
}

/// Why the folder cannot move from `from` to `to` now, with the sessions
/// `live` on this Host.
pub fn check(cfg: &Config, from: &Path, to: &Path, live: &[SessionInfo]) -> Result<()> {
    if let Some(why) = refusal(to, from, &skipper::home()?, cfg) {
        bail!("{why}");
    }
    let open = open_work(&from.join("ledger.db"))?;
    if open > 0 {
        bail!("{open} {} open in the ledger. Move the folder when the work is closed.", if open == 1 { "work is" } else { "works are" });
    }
    if let Some(s) = live.iter().find(|s| matches!(s.role, Role::Agent | Role::Task) && s.state == SessionState::Working) {
        bail!("The session {} works. Move the folder when it is idle.", s.name.as_deref().unwrap_or(&s.label));
    }
    Ok(())
}

/// Why `to` cannot take the folder now in `from`: the rules for any agent
/// folder, then the ones for a move.
fn refusal(to: &Path, from: &Path, home: &Path, cfg: &Config) -> Option<String> {
    if to == from {
        return Some("The agent folder is here now.".into());
    }
    if to.to_str().is_none() {
        return Some("Skiff stores folder names in UTF-8 only. Choose another folder.".into());
    }
    if to.starts_with(from) {
        return Some("This folder is inside the agent folder. Choose a folder outside it.".into());
    }
    if from.starts_with(to) {
        return Some("This folder holds the agent folder. Choose a new or empty folder.".into());
    }
    if let Some(why) = skipper::refusal(to, home, cfg) {
        return Some(why);
    }
    match kind(to) {
        Ok(Kind::New | Kind::Empty) => {}
        Ok(_) => return Some("This folder is not empty. Choose a new or empty folder.".into()),
        Err(e) => return Some(format!("{e:#}")),
    }
    let parent = existing(to.parent().unwrap_or(to));
    if !writable(&parent) {
        return Some(format!("You cannot write in {}. Choose another folder.", parent.display()));
    }
    None
}

/// Open work heads in the ledger: a plan, or a task with no plan, that is
/// not closed. A missing ledger has none.
fn open_work(ledger: &Path) -> Result<u64> {
    if !ledger.is_file() {
        return Ok(0);
    }
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("open {}", ledger.display()))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let n: i64 = db.query_row("SELECT count(*) FROM tasks WHERE parent IS NULL AND closed_at IS NULL", [], |r| r.get(0))?;
    Ok(n as u64)
}

/// The nearest folder at or above `p` that exists.
fn existing(p: &Path) -> PathBuf {
    p.ancestors().find(|a| a.is_dir()).unwrap_or(Path::new("/")).to_path_buf()
}

fn writable(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else { return false };
    // SAFETY: a plain syscall on a valid C string.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Moves the folder's files to `to` and writes the new root to
/// projects.toml, then repairs the Git worktrees under it. `live`: the
/// sessions to check (see [`check`]); [`run`] checked them already. A rename
/// on one disk; a copy, a rename into place and a remove of the old folder
/// across disks. A failure before the config write leaves the folder where
/// it was.
pub fn move_folder(cfg: &Config, to: &Path, live: &[SessionInfo]) -> Result<Moved> {
    let _lock = skipper::lock()?;
    let from = real(&skipper::folder(cfg)?);
    let to = real(to);
    // Read again under the lock: the ledger can have changed since the check.
    check(cfg, &from, &to, live)?;
    let parent = to.parent().context("the folder has no parent")?.to_path_buf();
    let was_empty = to.is_dir();
    // Parents this move makes, outermost first, so a failure removes them.
    let made: Vec<PathBuf> = parent.ancestors().take_while(|a| !a.exists()).map(Path::to_path_buf).collect::<Vec<_>>().into_iter().rev().collect();
    std::fs::create_dir_all(&parent).with_context(|| format!("Could not make {}", parent.display()))?;
    let undo_parents = || {
        if was_empty {
            let _ = std::fs::create_dir(&to);
        }
        for d in made.iter().rev() {
            let _ = std::fs::remove_dir(d);
        }
    };
    if was_empty {
        std::fs::remove_dir(&to).with_context(|| format!("Could not replace the empty folder {}", to.display()))?;
    }
    let copied = match std::fs::rename(&from, &to) {
        Ok(()) => false,
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            if let Err(e) = copy_across(&from, &to, &parent) {
                undo_parents();
                return Err(e);
            }
            true
        }
        Err(e) => {
            undo_parents();
            return Err(e).with_context(|| format!("Could not move {} to {}", from.display(), to.display()));
        }
    };
    if let Err(e) = write_folder(&to) {
        // Put the folder back: projects.toml still names the old place.
        if copied {
            let _ = std::fs::remove_dir_all(&to);
        } else {
            let _ = std::fs::rename(&to, &from);
        }
        undo_parents();
        return Err(e).context("Could not write projects.toml");
    }
    let mut warnings = Vec::new();
    if copied {
        if let Err(e) = std::fs::remove_dir_all(&from) {
            warnings.push(format!("Could not remove {}: {e}.", from.display()));
        }
    }
    warnings.extend(repair_worktrees(&to));
    Ok(Moved { from, to, warning: (!warnings.is_empty()).then(|| warnings.join(" ")) })
}

/// A move to another disk: copies `from` into a staging folder beside `to`
/// and renames it into place. The old folder stays until the caller removes
/// it. A failure removes the staging folder.
fn copy_across(from: &Path, to: &Path, parent: &Path) -> Result<()> {
    let staging = parent.join(format!("{}{STAGING}{}", to.file_name().context("the folder has no name")?.to_string_lossy(), std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let done = (|| -> Result<()> {
        std::fs::create_dir(&staging).with_context(|| format!("make {}", staging.display()))?;
        std::fs::set_permissions(&staging, from.metadata()?.permissions())?;
        copy_tree(from, from, &staging)?;
        let ledger = from.join("ledger.db");
        if ledger.is_file() {
            let db = Connection::open(&ledger).with_context(|| format!("open {}", ledger.display()))?;
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            let dst = staging.join("ledger.db");
            db.execute("VACUUM INTO ?1", [dst.to_str().context("the folder name is not UTF-8")?]).with_context(|| format!("copy {}", ledger.display()))?;
        }
        std::fs::rename(&staging, to).with_context(|| format!("rename {} to {}", staging.display(), to.display()))?;
        Ok(())
    })();
    if done.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    done.with_context(|| format!("Could not copy {} to {}", from.display(), to.display()))
}

/// Copies `dir` under `root` into `dst`: folders and files with their modes,
/// links as links. The ledger is left out: SQLite copies it. Sockets and
/// pipes are left out.
fn copy_tree(root: &Path, dir: &Path, dst: &Path) -> Result<()> {
    for e in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let p = e?.path();
        let rel = p.strip_prefix(root).unwrap();
        if rel == Path::new("ledger.db") || LEDGER_SIDE.iter().any(|f| rel == Path::new(f)) {
            continue;
        }
        let out = dst.join(rel);
        let m = p.symlink_metadata().with_context(|| format!("read {}", p.display()))?;
        if m.file_type().is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&p)?, &out).with_context(|| format!("link {}", out.display()))?;
        } else if m.is_dir() {
            std::fs::create_dir(&out).with_context(|| format!("make {}", out.display()))?;
            std::fs::set_permissions(&out, m.permissions())?;
            copy_tree(root, &p, dst)?;
        } else if m.is_file() {
            std::fs::copy(&p, &out).with_context(|| format!("copy {}", p.display()))?;
        }
    }
    Ok(())
}

/// Runs `git worktree repair` in each checkout under `dir`: a worktree's
/// link to its repository, and a repository's links to its worktrees, name
/// the old path. Returns a line for each that failed.
fn repair_worktrees(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    checkouts(dir, &mut found);
    found
        .into_iter()
        .filter_map(|d| {
            let out = std::process::Command::new("git")
                .args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "worktree", "repair"])
                .current_dir(&d)
                .output();
            match out {
                Ok(o) if o.status.success() => None,
                Ok(o) => Some(format!("git worktree repair failed in {}: {}", d.display(), String::from_utf8_lossy(&o.stderr).trim())),
                Err(e) => Some(format!("git worktree repair did not run in {}: {e}", d.display())),
            }
        })
        .collect()
}

/// The folders under `dir` that hold a `.git` file or folder. A checkout's
/// own folders are not searched.
fn checkouts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if p.join(".git").symlink_metadata().is_ok() {
            out.push(p);
        } else {
            checkouts(&p, out);
        }
    }
}

/// `[skiff_agent] folder` = the canonical path. The default folder takes the
/// key out.
fn write_folder(to: &Path) -> Result<()> {
    let default = real(&skipper::default_folder()?);
    config::set_skiff_agent(SkiffAgentKey::Folder((to != default).then_some(to)))
}

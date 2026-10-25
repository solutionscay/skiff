//! Settings › Disk use: what the Skiff agent's folder holds, and the flushes.
//! The closed state comes from the ledger through [`crate::ledger`], as the
//! panel reads it. A result file is one a ledger row names. A file no row
//! names counts as other and is never removed.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::Serialize;
use skiff_core::session::SessionInfo;

use crate::ledger::{self, FINISHED};

/// A count of files and their bytes.
#[derive(Serialize, Default, Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub files: u64,
    pub bytes: u64,
}

impl Line {
    fn add(&mut self, bytes: u64) {
        self.files += 1;
        self.bytes += bytes;
    }
}

/// One Skipper's results folder.
#[derive(Serialize, Debug)]
pub struct Skipper {
    /// "Skipper", or "<project> Skipper".
    pub name: String,
    /// The folder, relative to the agent folder, with a `/` at the end.
    pub folder: String,
    pub line: Line,
}

/// What the folder holds.
#[derive(Serialize, Debug, Default)]
pub struct Usage {
    pub folder: PathBuf,
    /// The folder is the `/tmp` cache: no agent folder is usable.
    pub fallback: bool,
    pub cache: Line,
    /// Files a ledger row names, in the results folders. `None`: the ledger
    /// was refused (see `ledger_error`).
    pub results: Option<Line>,
    pub skippers: Vec<Skipper>,
    /// The result files of closed work that Flush done work removes.
    pub closed: Line,
    /// The closed work items those files belong to.
    pub items: u64,
    /// The plan folders that are empty once those files go.
    pub plans: u64,
    pub logs: Line,
    pub other: Line,
    pub total: Line,
    /// Files the walk could not read.
    pub unreadable: u64,
    pub ledger_error: Option<String>,
    /// The daemon answered, so the closed work was checked against the live panes.
    pub daemon: bool,
    /// What Flush done work removes: the closed work's files and plan
    /// folders, then the cache's files. The flush takes it back.
    pub removable: Vec<String>,
}

/// What a flush removed.
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Removed {
    pub files: u64,
    pub bytes: u64,
}

/// Files a flush never removes, at any depth.
const KEEP: &[&str] = &[
    "ledger.db",
    "ledger.db-wal",
    "ledger.db-shm",
    "ledger.md.bak",
    "SKIPPER.md",
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    ".skiff-agent",
    ".agents",
    ".claude",
];

fn kept(p: &Path) -> bool {
    p.components().any(|c| KEEP.iter().any(|k| c.as_os_str() == *k))
}

/// Every file and link under `dir`, with its size. A link counts as 0 bytes
/// and is never followed. Answers with the count of entries it could not read.
fn walk(dir: &Path) -> (Vec<(PathBuf, u64)>, u64) {
    let mut out = Vec::new();
    let mut bad = 0;
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            bad += 1;
            continue;
        };
        for e in rd {
            let Ok(e) = e else {
                bad += 1;
                continue;
            };
            let p = e.path();
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => todo.push(p),
                Ok(m) if m.file_type().is_symlink() => out.push((p, 0)),
                Ok(m) => out.push((p, m.len())),
                Err(_) => bad += 1,
            }
        }
    }
    (out, bad)
}

/// `*.log`, `*.log.<n>`, or a file under a `logs/` folder.
fn is_log(rel: &Path) -> bool {
    let name = rel.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let numbered = name.rsplit_once('.').is_some_and(|(stem, n)| stem.ends_with(".log") && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    name.ends_with(".log") || numbered || rel.parent().is_some_and(|p| p.components().any(|c| c.as_os_str() == "logs"))
}

/// The results folders: `results/` and each `projects/<p>/results/`, as
/// (name, folder relative, canonical path).
fn roots(folder: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(p) = folder.join("results").canonicalize() {
        out.push(("Skipper".to_string(), "results/".to_string(), p));
    }
    let mut projects: Vec<_> = std::fs::read_dir(folder.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    projects.sort();
    for p in projects {
        if let Ok(dir) = folder.join("projects").join(&p).join("results").canonicalize() {
            out.push((format!("{p} Skipper"), format!("projects/{p}/results/"), dir));
        }
    }
    out
}

/// The result files the ledger names, and what of them closed work can give back.
#[derive(Default)]
struct Named {
    all: HashSet<PathBuf>,
    removable: HashSet<PathBuf>,
    /// The work head of each removable file.
    heads: HashSet<String>,
    /// Plan folders of closed work, with their plan's head.
    plan_folders: Vec<(PathBuf, String)>,
}

/// Reads the ledger's rows and sorts each named file into removable or not.
/// `sessions`: the daemon's list, `None` when it did not answer.
fn named(file: &Path, sessions: Option<&[SessionInfo]>, socket: &Path, folder: &Path, roots: &[PathBuf]) -> anyhow::Result<Named> {
    let mut out = Named::default();
    if !file.exists() {
        return Ok(out);
    }
    let db = ledger::open_read(file)?;
    let mut rows = ledger::rows(&db, "", None)?;
    ledger::roll_up(&mut rows);
    for r in rows.iter_mut() {
        r.elsewhere = r.socket_other_than(socket).is_some();
        // A sent task that cannot be checked counts as worked on.
        r.agent_on = !r.is_plan() && r.state == "sent";
        if let (Some(all), false, false) = (sessions, r.is_plan(), r.elsewhere) {
            let s = r.pane_id.as_deref().and_then(|p| all.iter().find(|s| s.id == p));
            r.agent_on = r.agent_on_in(s);
        }
    }
    ledger::buckets(&mut rows);
    let heads = ledger::heads(&rows);
    // An agent on any task of closed work keeps all of it. Another socket's
    // pane cannot be checked: its closed work stays closed.
    let busy: HashSet<&str> = rows.iter().filter(|r| r.agent_on && !r.elsewhere).map(|r| heads[&r.uid].as_str()).collect();
    let done = |r: &ledger::Row| r.bucket == Some(FINISHED) && !busy.contains(heads[&r.uid].as_str());
    let inside = |p: &Path| roots.iter().any(|root| p.starts_with(root) && p != root);
    // Each file: removable while every row that names it is done.
    let mut keep: HashMap<PathBuf, (bool, String)> = HashMap::new();
    for r in &rows {
        let Some(p) = r.result_path.as_deref().and_then(|p| Path::new(p).canonicalize().ok()) else { continue };
        if !inside(&p) {
            continue;
        }
        let e = keep.entry(p).or_insert((true, heads[&r.uid].clone()));
        e.0 &= done(r);
    }
    for r in rows.iter().filter(|r| r.is_plan() && done(r)) {
        let Some(plan) = r.result_path.as_deref().map(Path::new) else { continue };
        let Some(dir) = plan.parent().and_then(|d| d.canonicalize().ok()) else { continue };
        let direct = dir.parent().is_some_and(|up| roots.iter().any(|root| root == up));
        if direct && dir.file_name().is_some_and(|n| n.to_string_lossy().contains(&r.uid)) {
            out.plan_folders.push((dir, heads[&r.uid].clone()));
        }
    }
    for (p, (ok, head)) in keep {
        if ok && !kept(p.strip_prefix(folder).unwrap_or(&p)) {
            out.removable.insert(p.clone());
            out.heads.insert(head);
        }
        out.all.insert(p);
    }
    Ok(out)
}

/// Counts `folder`. `ledger`: the ledger file, `None` for the `/tmp` cache.
/// `cache`: the cache folder from [`crate::skipper::cache_dir`].
pub fn usage(folder: &Path, cache: &Path, ledger: Option<&Path>, sessions: Option<&[SessionInfo]>, socket: &Path) -> Usage {
    let mut u = Usage {
        folder: folder.to_path_buf(),
        fallback: ledger.is_none(),
        daemon: sessions.is_some(),
        ..Default::default()
    };
    let Ok(root) = folder.canonicalize() else {
        u.results = Some(Line::default());
        return u;
    };
    let cache = cache.canonicalize().unwrap_or_else(|_| cache.to_path_buf());
    let results = roots(&root);
    let dirs: Vec<PathBuf> = results.iter().map(|r| r.2.clone()).collect();
    let n = match ledger.map(|l| named(l, sessions, socket, &root, &dirs)) {
        None => Some(Named::default()),
        Some(Ok(n)) => Some(n),
        Some(Err(e)) => {
            u.ledger_error = Some(format!("{e:#}"));
            None
        }
    };
    let (files, bad) = walk(&root);
    u.unreadable = bad;
    let mut lines = vec![Line::default(); results.len()];
    let mut res = Line::default();
    let mut cached = Vec::new();
    for (p, bytes) in &files {
        u.total.add(*bytes);
        if p.starts_with(&cache) {
            u.cache.add(*bytes);
            cached.push(p);
        } else if let Some(n) = n.as_ref().filter(|n| n.all.contains(p)) {
            res.add(*bytes);
            if let Some(i) = dirs.iter().position(|d| p.starts_with(d)) {
                lines[i].add(*bytes);
            }
            if n.removable.contains(p) {
                u.closed.add(*bytes);
            }
        } else if is_log(p.strip_prefix(&root).unwrap_or(p)) {
            u.logs.add(*bytes);
        } else {
            u.other.add(*bytes);
        }
    }
    u.skippers = results.into_iter().zip(lines).map(|((name, folder, _), line)| Skipper { name, folder, line }).collect();
    if let Some(n) = n {
        u.results = Some(res);
        u.items = n.heads.len() as u64;
        let mut removable: Vec<&PathBuf> = n.removable.iter().filter(|p| files.iter().any(|(f, _)| f == *p)).collect();
        removable.sort();
        // A plan folder goes when every file in it goes.
        let mut plans: Vec<&PathBuf> = n
            .plan_folders
            .iter()
            .map(|(d, _)| d)
            .filter(|d| files.iter().filter(|(f, _)| f.starts_with(d)).all(|(f, _)| n.removable.contains(f)))
            .collect();
        plans.sort();
        plans.dedup();
        u.plans = plans.len() as u64;
        u.removable = removable.into_iter().chain(plans).map(|p| p.to_string_lossy().into_owned()).collect();
    }
    cached.sort();
    u.removable.extend(cached.into_iter().map(|p| p.to_string_lossy().into_owned()));
    u
}

/// Removes every file in the cache. Its folders stay.
pub fn flush_cache(cache: &Path) -> Removed {
    let Ok(root) = cache.canonicalize() else { return Removed::default() };
    let (files, _) = walk(&root);
    remove(files.into_iter().filter(|(p, _)| p.starts_with(&root)))
}

/// Removes cache files whose last change is older than `days` days. `0` does nothing.
pub fn flush_old(cache: &Path, days: u32) -> Removed {
    let Ok(root) = cache.canonicalize() else { return Removed::default() };
    let Some(before) = SystemTime::now().checked_sub(Duration::from_secs(u64::from(days) * 86_400)).filter(|_| days > 0) else {
        return Removed::default();
    };
    let (files, _) = walk(&root);
    let old = |p: &Path| std::fs::symlink_metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t < before);
    remove(files.into_iter().filter(|(p, _)| p.starts_with(&root) && old(p)))
}

/// Flush done work: removes what is on both `expected`, the list the dialog
/// showed, and `fresh.removable`, counted again now. Work that opened again
/// in between stays. Files go first, then the plan folders that are empty.
pub fn flush_done(expected: &[String], fresh: &Usage) -> Removed {
    let Ok(root) = fresh.folder.canonicalize() else { return Removed::default() };
    let asked: HashSet<&str> = expected.iter().map(String::as_str).collect();
    let both: Vec<PathBuf> = fresh.removable.iter().filter(|p| asked.contains(p.as_str())).map(PathBuf::from).collect();
    let safe = |p: &Path| p.starts_with(&root) && p != root && !kept(p.strip_prefix(&root).unwrap_or(p));
    let (dirs, files): (Vec<_>, Vec<_>) = both.into_iter().filter(|p| safe(p)).partition(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()));
    let sized = files.into_iter().filter_map(|p| std::fs::symlink_metadata(&p).ok().map(|m| (p, if m.file_type().is_symlink() { 0 } else { m.len() })));
    let out = remove(sized);
    for d in dirs {
        remove_empty(&d);
    }
    out
}

fn remove(files: impl Iterator<Item = (PathBuf, u64)>) -> Removed {
    let mut out = Removed::default();
    for (p, bytes) in files {
        if std::fs::remove_file(&p).is_ok() {
            out.files += 1;
            out.bytes += bytes;
        }
    }
    out
}

/// Removes `dir` and the folders in it, deepest first, while they are empty.
/// A folder that holds a file stays.
fn remove_empty(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                remove_empty(&e.path());
            }
        }
    }
    let _ = std::fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A folder with: a closed plan with a task, a closed lone task whose
    /// result an open task also names, an open task, an unnamed result, a
    /// project Skipper's closed task, a log and two cache images.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("skiff-disk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = dir.join("results");
        let plan = r.join("2026-10-01-a1b2c3d4-plan");
        write(&plan.join("plan.md"), "plan");
        write(&plan.join("e5f6a7b8-step.md"), "step");
        write(&r.join("2026-10-02-c0ffee01-lone.md"), "lone");
        write(&r.join("2026-10-03-beef0002-open.md"), "open");
        write(&r.join("2026-10-04-dead0003-shared.md"), "shared");
        write(&r.join("notes.md"), "mine");
        write(&r.join("AGENTS.md"), "x");
        write(&dir.join("projects/web/results/2026-10-05-f00d0004-web.md"), "web");
        write(&dir.join("projects/web/SKIPPER.md"), "s");
        write(&dir.join("watch.log.1"), "log");
        write(&dir.join("cache/0123456789abcdef.png"), "png");
        write(&dir.join("cache/fedcba9876543210.jpg"), "jpg");
        let db = Connection::open(dir.join("ledger.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE tasks (id INTEGER PRIMARY KEY, uid TEXT, kind TEXT, parent TEXT, pane_id TEXT, pane_gen INTEGER, project TEXT,
               task TEXT, sent_at TEXT, state TEXT, result_path TEXT, ref TEXT, updated_at TEXT, type TEXT, socket TEXT, reported_at TEXT, title TEXT, closed_at TEXT, gone_at TEXT, owner TEXT NOT NULL DEFAULT 'grand');",
        )
        .unwrap();
        let row = |uid: &str, kind: &str, parent: Option<&str>, state: &str, path: &Path, closed: bool, owner: &str| {
            db.execute(
                "INSERT INTO tasks (uid, kind, parent, project, task, sent_at, state, result_path, updated_at, closed_at, owner) VALUES (?1, ?2, ?3, '', 't', 'x', ?4, ?5, 'x', ?6, ?7)",
                rusqlite::params![uid, kind, parent, state, path.to_string_lossy(), closed.then_some("x"), owner],
            )
            .unwrap();
        };
        row("a1b2c3d4", "plan", None, "done", &plan.join("plan.md"), true, "grand");
        row("e5f6a7b8", "task", Some("a1b2c3d4"), "done", &plan.join("e5f6a7b8-step.md"), false, "grand");
        row("c0ffee01", "task", None, "done", &r.join("2026-10-02-c0ffee01-lone.md"), true, "grand");
        row("beef0002", "task", None, "done", &r.join("2026-10-03-beef0002-open.md"), false, "grand");
        row("dead0003", "task", None, "done", &r.join("2026-10-04-dead0003-shared.md"), true, "grand");
        row("dead0005", "task", None, "done", &r.join("2026-10-04-dead0003-shared.md"), false, "grand");
        row("f00d0004", "task", None, "done", &dir.join("projects/web/results/2026-10-05-f00d0004-web.md"), true, "web");
        // A result outside the results folders is never counted.
        row("0ut51de0", "task", None, "done", &dir.join("SKIPPER.md"), true, "grand");
        dir
    }

    #[test]
    fn closed_work_names_what_goes_and_unnamed_files_stay() {
        let dir = folder("count");
        let u = usage(&dir, &dir.join("cache"), Some(&dir.join("ledger.db")), Some(&[]), Path::new("/nowhere"));
        assert_eq!(u.cache, Line { files: 2, bytes: 6 });
        // plan, step, lone, open, shared, web.
        assert_eq!(u.results.unwrap().files, 6);
        assert_eq!(u.skippers.iter().map(|s| (s.name.as_str(), s.line.files)).collect::<Vec<_>>(), [("Skipper", 5), ("web Skipper", 1)]);
        // plan, step, lone, web: shared is also named by open work.
        assert_eq!(u.closed.files, 4);
        assert_eq!((u.items, u.plans), (3, 1));
        assert_eq!(u.logs.files, 1);
        // notes.md, AGENTS.md, SKIPPER.md, ledger.db.
        assert_eq!(u.other.files, 4);
        assert_eq!(u.total.files, 13);

        // Work that opened again after the dialog stays.
        let shown = u.removable.clone();
        let db = Connection::open(dir.join("ledger.db")).unwrap();
        db.execute("UPDATE tasks SET closed_at = NULL WHERE uid = 'c0ffee01'", []).unwrap();
        let fresh = usage(&dir, &dir.join("cache"), Some(&dir.join("ledger.db")), Some(&[]), Path::new("/nowhere"));
        let out = flush_done(&shown, &fresh);
        assert_eq!(out.files, 5);
        assert!(!dir.join("results/2026-10-01-a1b2c3d4-plan").exists());
        assert!(dir.join("results/2026-10-02-c0ffee01-lone.md").exists());
        assert!(dir.join("results/2026-10-04-dead0003-shared.md").exists());
        assert!(dir.join("results/notes.md").exists());
        assert!(dir.join("projects/web/SKIPPER.md").exists());
        assert!(!dir.join("projects/web/results/2026-10-05-f00d0004-web.md").exists());
        assert_eq!(std::fs::read_dir(dir.join("cache")).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_daemon_sent_work_stays_and_the_cache_flushes() {
        let dir = folder("nodaemon");
        let db = Connection::open(dir.join("ledger.db")).unwrap();
        db.execute("UPDATE tasks SET state = 'sent' WHERE uid = 'e5f6a7b8'", []).unwrap();
        let u = usage(&dir, &dir.join("cache"), Some(&dir.join("ledger.db")), None, Path::new("/nowhere"));
        assert!(!u.daemon);
        // The plan has a sent task that cannot be checked: lone and web only.
        assert_eq!((u.closed.files, u.plans), (2, 0));
        assert_eq!(flush_cache(&dir.join("cache")), Removed { files: 2, bytes: 6 });
        assert_eq!(flush_old(&dir.join("cache"), 30), Removed::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn logs_by_name_and_folder() {
        assert!(is_log(Path::new("a/watch.log")));
        assert!(is_log(Path::new("watch.log.3")));
        assert!(is_log(Path::new("logs/today.txt")));
        assert!(!is_log(Path::new("catalog.md")));
        assert!(!is_log(Path::new("x.log.md")));
    }
}

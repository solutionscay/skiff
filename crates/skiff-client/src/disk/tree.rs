//! The walk of the folder into nodes, the names it keeps, and the fold of a big folder into one row.

use super::{
    checkout::{checkout, mtime, Checkout},
    model::{Kind, Node, Verdict, FOLD_FILES, KEEP},
};

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

/// A walk of `abs`, never through a link. Counts what it cannot read. The
/// git checks of the checkouts in it run at the end, side by side: each
/// one runs git a few times, and they are most of the count's time.
pub(super) fn walk(abs: &Path, rel: &str, bad: &mut u64) -> Node {
    let mut repos = Vec::new();
    let mut n = walk_files(abs, rel, bad, &mut repos);
    let states = checkouts(&repos);
    set_checkouts(&mut n, &states);
    n
}

/// The [`checkout`] state of each folder in `dirs`, a few at a time.
fn checkouts(dirs: &[PathBuf]) -> HashMap<PathBuf, Checkout> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let next = AtomicUsize::new(0);
    let out = Mutex::new(HashMap::new());
    std::thread::scope(|s| {
        for _ in 0..threads.min(dirs.len()) {
            s.spawn(|| {
                while let Some(d) = dirs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let c = checkout(d);
                    out.lock().unwrap_or_else(|e| e.into_inner()).insert(d.clone(), c);
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|e| e.into_inner())
}

fn set_checkouts(n: &mut Node, states: &HashMap<PathBuf, Checkout>) {
    if let Some(c) = states.get(&n.abs) {
        n.checkout = Some(*c);
    }
    for c in &mut n.children {
        if c.dir {
            set_checkouts(c, states);
        }
    }
}

/// [`walk`] without the git checks: the folders that hold a `.git` go to `repos`.
fn walk_files(abs: &Path, rel: &str, bad: &mut u64, repos: &mut Vec<PathBuf>) -> Node {
    let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut n = Node {
        name: name.clone(),
        path: rel.to_string(),
        bytes: 0,
        files: 0,
        mtime: 0,
        state: None,
        children: Vec::new(),
        dir: false,
        frame: false,
        work: None,
        checkout: None,
        abs: abs.to_path_buf(),
    };
    let meta = match std::fs::symlink_metadata(abs) {
        Ok(m) => m,
        Err(_) => {
            *bad += 1;
            n.state = Some(Verdict::new(Kind::Prot, "unreadable"));
            n.files = 1;
            return n;
        }
    };
    n.mtime = mtime(&meta);
    if !meta.is_dir() {
        n.files = 1;
        n.bytes = if meta.file_type().is_symlink() { 0 } else { meta.len() };
        return n;
    }
    n.dir = true;
    n.name.push('/');
    let rd = match std::fs::read_dir(abs) {
        Ok(rd) => rd,
        Err(_) => {
            // A folder Skiff cannot list: one protected row.
            *bad += 1;
            n.state = Some(Verdict::new(Kind::Prot, "unreadable"));
            n.files = 1;
            return n;
        }
    };
    let mut kids: Vec<PathBuf> = Vec::new();
    for e in rd {
        match e {
            Ok(e) => kids.push(e.path()),
            Err(_) => *bad += 1,
        }
    }
    kids.sort();
    if kids.iter().any(|k| k.file_name().is_some_and(|f| f == ".git")) && !rel.is_empty() {
        repos.push(abs.to_path_buf());
    }
    let own = n.mtime;
    for k in kids {
        let kname = k.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let krel = if rel.is_empty() { kname } else { format!("{rel}/{kname}") };
        let c = walk_files(&k, &krel, bad, repos);
        n.bytes += c.bytes;
        n.files += c.files;
        n.children.push(c);
    }
    n.mtime = n.children.iter().filter(|c| c.files > 0).map(|c| c.mtime).max().unwrap_or(own);
    n
}

/// Skiff's own folders: the agent folder, `cache/`, `types/`, the results
/// folders and the Skippers' folders above them.
pub(super) fn is_frame(rel: &str) -> bool {
    let parts: Vec<&str> = if rel.is_empty() { Vec::new() } else { rel.split('/').collect() };
    matches!(
        parts.as_slice(),
        [] | ["cache"] | ["results"] | ["types"] | ["projects"] | ["fleets"] | ["projects" | "fleets", _] | ["projects" | "fleets", _, "results"]
    )
}

/// A Skipper's own folder: its pane's cwd holds every result, so it
/// protects nothing.
pub(super) fn is_home(rel: &str) -> bool {
    let parts: Vec<&str> = if rel.is_empty() { Vec::new() } else { rel.split('/').collect() };
    matches!(parts.as_slice(), [] | ["projects" | "fleets", _])
}

/// Skiff's own files: [`KEEP`] and `types/` at the top of the agent
/// folder, and [`KEEP`] in a Skipper's folder. A file of that name deeper
/// down, such as a repository's `AGENTS.md`, is not Skiff's.
pub(super) fn kept_name(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        [first, ..] if *first == "types" || KEEP.contains(first) => true,
        ["projects" | "fleets", _, name, ..] => KEEP.contains(name),
        _ => false,
    }
}

/// Folds a folder whose files all have one verdict and that has many files.
pub(super) fn fold(n: &mut Node) {
    for c in &mut n.children {
        fold(c);
    }
    if !n.dir || n.frame || n.work.is_some() || n.files <= FOLD_FILES || n.children.is_empty() {
        return;
    }
    let mut vs = Vec::new();
    collect_states(n, &mut vs);
    if vs.windows(2).all(|w| w[0] == w[1]) {
        n.state = vs.into_iter().next();
        n.children.clear();
    }
}

pub(super) fn collect_states(n: &Node, out: &mut Vec<Verdict>) {
    if n.children.is_empty() {
        if let Some(v) = &n.state {
            out.push(v.clone());
        }
    }
    for c in &n.children {
        collect_states(c, out);
    }
}

pub(super) fn find<'a>(n: &'a Node, path: &str) -> Option<&'a Node> {
    if n.path == path {
        return Some(n);
    }
    n.children.iter().filter(|c| path == c.path || path.starts_with(&format!("{}/", c.path))).find_map(|c| find(c, path))
}

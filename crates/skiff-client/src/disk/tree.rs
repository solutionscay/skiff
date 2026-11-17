//! The walk of the folder into nodes, the names it keeps, and the fold of a big folder into one row.

use super::{
    checkout::{checkout, mtime, Checkout},
    model::{Kind, Node, Verdict, FOLD_FILES, KEEP},
    progress::Progress,
};

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Mutex,
    },
};

/// Threads for the walk and for the git checks.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()).min(8)
}

/// A walk of `abs`, never through a link. Counts what it cannot read. The
/// walk lists folders on several threads. Each git checkout it finds is
/// checked at once, on other threads, while the walk goes on: each check
/// runs git a few times. `p` gets the running totals.
pub(super) fn walk(abs: &Path, rel: &str, bad: &mut u64, p: &Progress) -> Node {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let rx = Mutex::new(rx);
    let states = Mutex::new(HashMap::new());
    let unread = AtomicU64::new(0);
    let mut n = std::thread::scope(|s| {
        for _ in 0..threads() {
            s.spawn(|| loop {
                // The lock is held only to take the next folder.
                let next = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                let Ok(d) = next else { return };
                let c = checkout(&d);
                states.lock().unwrap_or_else(|e| e.into_inner()).insert(d, c);
                p.checked();
            });
        }
        let w = Walk { bad: &unread, repos: tx, p, spare: AtomicUsize::new(threads().saturating_sub(1)) };
        let n = walk_files(abs, rel, &w);
        p.walked();
        // The last sender goes: the git threads end when the queue is empty.
        drop(w);
        n
    });
    *bad += unread.into_inner();
    set_checkouts(&mut n, &states.into_inner().unwrap_or_else(|e| e.into_inner()));
    n
}

/// What every folder of one walk shares.
struct Walk<'a> {
    bad: &'a AtomicU64,
    /// The folders that hold a `.git`, for the git threads.
    repos: mpsc::Sender<PathBuf>,
    p: &'a Progress,
    /// Threads the walk may still start. A thread gives its place back when it ends.
    spare: AtomicUsize,
}

impl Walk<'_> {
    /// One place for a thread, when one is free.
    fn take(&self) -> bool {
        self.spare.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1)).is_ok()
    }
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

/// A folder's entry: its path, its path in the tree, and whether it is a folder.
type Kid = (PathBuf, String, bool);

/// The nodes of `kids`, in their order. A folder with two subfolders or
/// more, or with many entries, shares them with the threads that are free.
fn walk_kids(kids: &[Kid], w: &Walk) -> Vec<Node> {
    let dirs = kids.iter().filter(|k| k.2).count();
    if dirs < 2 && kids.len() < 64 {
        return kids.iter().map(|(k, r, _)| walk_files(k, r, w)).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Node>>> = kids.iter().map(|_| Mutex::new(None)).collect();
    let work = || loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        let Some((k, r, _)) = kids.get(i) else { return };
        let n = walk_files(k, r, w);
        *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(n);
    };
    std::thread::scope(|s| {
        let mut helpers = 1;
        while helpers < kids.len() && w.take() {
            helpers += 1;
            s.spawn(|| {
                work();
                w.spare.fetch_add(1, Ordering::AcqRel);
            });
        }
        work();
    });
    slots.into_iter().filter_map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner())).collect()
}

/// [`walk`] of one file or folder: the git checks go to the git threads.
fn walk_files(abs: &Path, rel: &str, w: &Walk) -> Node {
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
    let unreadable = |mut n: Node| {
        w.bad.fetch_add(1, Ordering::Relaxed);
        w.p.file(0);
        n.state = Some(Verdict::new(Kind::Prot, "unreadable"));
        n.files = 1;
        n
    };
    let Ok(meta) = std::fs::symlink_metadata(abs) else { return unreadable(n) };
    n.mtime = mtime(&meta);
    if !meta.is_dir() {
        n.files = 1;
        n.bytes = if meta.file_type().is_symlink() { 0 } else { meta.len() };
        w.p.file(n.bytes);
        return n;
    }
    n.dir = true;
    n.name.push('/');
    // A folder Skiff cannot list: one protected row.
    let Ok(rd) = std::fs::read_dir(abs) else { return unreadable(n) };
    let mut kids: Vec<Kid> = Vec::new();
    for e in rd {
        match e {
            Ok(e) => {
                let kname = e.file_name().to_string_lossy().into_owned();
                let krel = if rel.is_empty() { kname } else { format!("{rel}/{kname}") };
                // The entry's type from the listing: a link is not a folder.
                kids.push((e.path(), krel, e.file_type().is_ok_and(|t| t.is_dir())));
            }
            Err(_) => {
                w.bad.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    kids.sort();
    if kids.iter().any(|k| k.0.file_name().is_some_and(|f| f == ".git")) && !rel.is_empty() {
        w.p.repo();
        let _ = w.repos.send(abs.to_path_buf());
    }
    let own = n.mtime;
    n.children = walk_kids(&kids, w);
    n.bytes = n.children.iter().map(|c| c.bytes).sum();
    n.files = n.children.iter().map(|c| c.files).sum();
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

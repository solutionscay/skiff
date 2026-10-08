//! Which worktrees a session's processes work in. An agent can make a second
//! worktree and run every command there, while its session stays in the
//! first. The scan reads the working folder of each process in the session.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use skiff_core::{
    protocol::Event,
    session::{now_ms, Away, SessionState},
};

use super::{Session, SessionPool};

/// How often working sessions are scanned. A record keeps for a minute, so
/// a slow scan still shows where a session works; a fast one reads the
/// process tree of every working session each time.
const EVERY: Duration = Duration::from_secs(3);

/// A record goes this long after the last process seen in its worktree.
const KEEP_MS: u64 = 60_000;

/// Parent pid to child pids, read as a pass asks for them. Linux keeps a
/// list of children per thread, so a pass reads only the subtrees it looks
/// at. Where there is no such list, the first ask reads every process once.
#[derive(Default)]
pub(super) struct Tree {
    children: HashMap<u32, Vec<u32>>,
    /// `children` holds every process.
    whole: bool,
}

impl Tree {
    pub fn children(&mut self, pid: u32) -> Vec<u32> {
        if !self.whole && !self.children.contains_key(&pid) {
            match proc::children_of(pid) {
                Some(kids) => {
                    self.children.insert(pid, kids);
                }
                None => {
                    self.children = proc::children();
                    self.whole = true;
                }
            }
        }
        self.children.get(&pid).cloned().unwrap_or_default()
    }
}

impl SessionPool {
    /// Scans `working` sessions every few seconds. A thread, not a task: the
    /// scan reads `/proc` and stats folders.
    pub fn spawn_away_watcher(self: &Arc<Self>) {
        let pool = self.clone();
        let _ = std::thread::Builder::new().name("away".into()).spawn(move || loop {
            std::thread::sleep(EVERY);
            pool.scan_away();
        });
    }

    fn scan_away(&self) {
        let Some(_quiet) = self.quiet() else { return };
        let sessions: Vec<Arc<Session>> = self.sessions.read().unwrap().values().cloned().collect();
        let working = |s: &Session| s.info.lock().unwrap().state == SessionState::Working;
        let busy = sessions.iter().any(|s| working(s));
        if !busy && sessions.iter().all(|s| s.info.lock().unwrap().away.is_empty()) {
            return;
        }
        let mut tree = busy.then(Tree::default);
        let now = now_ms();
        let mut roots = HashMap::new();
        for s in &sessions {
            let seen = match (&mut tree, working(s)) {
                (Some(tree), true) => seen_in(s, tree, &mut roots),
                _ => Vec::new(),
            };
            let changed = {
                let mut info = s.info.lock().unwrap();
                let before: Vec<PathBuf> = info.away.iter().map(|a| a.path.clone()).collect();
                for (path, command) in seen {
                    match info.away.iter_mut().find(|a| a.path == path) {
                        Some(a) => {
                            a.command = command;
                            a.at = now;
                        }
                        None => info.away.push(Away { path, command, at: now }),
                    }
                }
                info.away.retain(|a| now.saturating_sub(a.at) < KEEP_MS);
                info.away.sort_by(|a, b| a.path.cmp(&b.path));
                let after: Vec<&PathBuf> = info.away.iter().map(|a| &a.path).collect();
                (after != before.iter().collect::<Vec<_>>()).then(|| (info.id.clone(), info.away.clone()))
            };
            if let Some((session, away)) = changed {
                let _ = self.events.send(Event::Away { session, away });
            }
        }
    }
}

/// Worktrees other than the session's own where its processes are now, with
/// the name of one process in each.
fn seen_in(s: &Session, tree: &mut Tree, roots: &mut HashMap<PathBuf, Option<PathBuf>>) -> Vec<(PathBuf, String)> {
    let (pid, cwd) = {
        let info = s.info.lock().unwrap();
        (info.pid, info.cwd.clone())
    };
    let Some(pid) = pid else { return Vec::new() };
    let mut root_of = |dir: PathBuf| roots.entry(dir).or_insert_with_key(|d| worktree_root(d)).clone();
    let home = root_of(cwd);
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let mut stack = vec![pid];
    while let Some(p) = stack.pop() {
        stack.extend(tree.children(p));
        let Some(root) = proc::cwd(p).and_then(&mut root_of) else { continue };
        if Some(&root) == home.as_ref() || out.iter().any(|(r, _)| *r == root) {
            continue;
        }
        out.push((root, proc::name(p).unwrap_or_default()));
    }
    out
}

/// The deepest folder at or above `dir` with a `.git` entry. A worktree made
/// inside the main checkout (`.claude/worktrees/x`) has its own `.git` file,
/// so it wins over the checkout around it.
fn worktree_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

#[cfg(target_os = "linux")]
pub(super) mod proc {
    use std::{collections::HashMap, path::{Path, PathBuf}, sync::OnceLock};

    /// The children of one process, from the list the kernel keeps per
    /// thread. `None` when this kernel keeps no such list (it needs
    /// CONFIG_PROC_CHILDREN); an empty list for a process that is gone.
    pub fn children_of(pid: u32) -> Option<Vec<u32>> {
        static LISTED: OnceLock<bool> = OnceLock::new();
        let listed = *LISTED.get_or_init(|| {
            let me = std::process::id();
            Path::new(&format!("/proc/{me}/task/{me}/children")).exists()
        });
        if !listed {
            return None;
        }
        let mut kids = Vec::new();
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else { return Some(kids) };
        for t in tasks.flatten() {
            if let Ok(list) = std::fs::read_to_string(t.path().join("children")) {
                kids.extend(list.split_whitespace().filter_map(|p| p.parse::<u32>().ok()));
            }
        }
        Some(kids)
    }

    /// Parent pid to child pids, for every process. One read per process.
    pub fn children() -> HashMap<u32, Vec<u32>> {
        let mut tree: HashMap<u32, Vec<u32>> = HashMap::new();
        let Ok(dir) = std::fs::read_dir("/proc") else { return tree };
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // `pid (comm) state ppid ...`; comm can hold spaces and parens.
            let ppid = stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(1))
                .and_then(|p| p.parse::<u32>().ok());
            if let Some(ppid) = ppid {
                tree.entry(ppid).or_default().push(pid);
            }
        }
        tree
    }

    pub fn cwd(pid: u32) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    pub fn name(pid: u32) -> Option<String> {
        std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string())
    }

    pub fn group(pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?.1.split_whitespace().nth(2)?.parse().ok()
    }

    pub fn argv(pid: u32) -> Vec<String> {
        std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default()
            .split(|b| *b == 0).filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned()).collect()
    }
}

#[cfg(target_os = "macos")]
pub(super) mod proc {
    use std::{collections::HashMap, ffi::CStr, mem, os::raw::c_void, path::PathBuf};

    const PROC_ALL_PIDS: u32 = 1;

    /// No list per process here: `children` lists every process in one call.
    pub fn children_of(_: u32) -> Option<Vec<u32>> {
        None
    }

    /// Parent pid to child pids, for every process.
    pub fn children() -> HashMap<u32, Vec<u32>> {
        let mut tree: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut pids = vec![0 as libc::pid_t; 4096];
        let size = (pids.len() * mem::size_of::<libc::pid_t>()) as libc::c_int;
        // Returns bytes written.
        let n = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr() as *mut c_void, size) };
        let n = (n.max(0) as usize / mem::size_of::<libc::pid_t>()).min(pids.len());
        for &pid in &pids[..n] {
            if pid <= 0 {
                continue;
            }
            let mut info: libc::proc_bsdinfo = unsafe { mem::zeroed() };
            let want = mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut c_void, want) };
            if got == want {
                tree.entry(info.pbi_ppid).or_default().push(pid as u32);
            }
        }
        tree
    }

    pub fn cwd(pid: u32) -> Option<PathBuf> {
        let mut info: libc::proc_vnodepathinfo = unsafe { mem::zeroed() };
        let want = mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
        let got = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDVNODEPATHINFO, 0, &mut info as *mut _ as *mut c_void, want) };
        if got != want {
            return None;
        }
        let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const libc::c_char) };
        let path = path.to_str().ok()?;
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    pub fn name(pid: u32) -> Option<String> {
        let mut buf = [0u8; 256];
        let n = unsafe { libc::proc_name(pid as libc::c_int, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    }

    pub fn group(pid: u32) -> Option<u32> {
        let mut info: libc::proc_bsdinfo = unsafe { mem::zeroed() };
        let want = mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let got = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut c_void, want) };
        (got == want).then_some(info.pbi_pgid)
    }

    pub fn argv(pid: u32) -> Vec<String> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
        let mut size = 0;
        let result = unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) };
        if result != 0 || size < mem::size_of::<i32>() || size > 1024 * 1024 { return Vec::new(); }
        let mut bytes = vec![0u8; size];
        let result = unsafe { libc::sysctl(mib.as_mut_ptr(), 3, bytes.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut(), 0) };
        if result != 0 || size < 4 { return Vec::new(); }
        bytes.truncate(size);
        let argc = i32::from_ne_bytes(bytes[..4].try_into().unwrap()).max(0) as usize;
        // Skip the executable path and its null padding. Stop before the environment.
        let rest = &bytes[4..];
        let Some(end) = rest.iter().position(|b| *b == 0) else { return Vec::new() };
        rest[end..].split(|b| *b == 0).filter(|s| !s.is_empty()).take(argc)
            .map(|s| String::from_utf8_lossy(s).into_owned()).collect()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) mod proc {
    use std::{collections::HashMap, path::PathBuf};

    pub fn children_of(_: u32) -> Option<Vec<u32>> {
        None
    }

    pub fn children() -> HashMap<u32, Vec<u32>> {
        HashMap::new()
    }

    pub fn cwd(_: u32) -> Option<PathBuf> {
        None
    }

    pub fn name(_: u32) -> Option<String> {
        None
    }

    pub fn group(_: u32) -> Option<u32> { None }
    pub fn argv(_: u32) -> Vec<String> { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::worktree_root;

    #[test]
    fn deepest_worktree_wins() {
        let dir = std::env::temp_dir().join(format!("skiff-away-{}", std::process::id()));
        let nested = dir.join(".claude/worktrees/x");
        std::fs::create_dir_all(nested.join("src")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(nested.join(".git"), "gitdir: elsewhere").unwrap();
        assert_eq!(worktree_root(&dir.join(".claude")), Some(dir.clone()));
        assert_eq!(worktree_root(&nested.join("src")), Some(nested.clone()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

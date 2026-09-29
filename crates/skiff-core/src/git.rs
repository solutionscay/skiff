//! Thin wrappers over the `git` binary. Blocking: async callers use
//! `spawn_blocking`.

use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, bail, Context, Result};

use crate::project::Worktree;

/// Runs `git -C <dir> <args>` and returns stdout. A non-zero exit becomes an
/// error that carries git's stderr.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("run git")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("git {} failed: {}", args.join(" "), out.status);
        }
        bail!(stderr);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn list_worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    Ok(parse_worktrees(&git(
        repo,
        &["worktree", "list", "--porcelain", "-z"],
    )?))
}

/// Parses `git worktree list --porcelain -z`: NUL after every field, and an
/// empty field after every record. The first record is the main worktree.
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut cur: Option<Worktree> = None;
    for field in out.split('\0') {
        if field.is_empty() {
            list.extend(cur.take());
            continue;
        }
        let (key, val) = field.split_once(' ').unwrap_or((field, ""));
        if key == "worktree" {
            list.extend(cur.take());
            cur = Some(Worktree {
                path: PathBuf::from(val),
                branch: None,
                head: String::new(),
                is_main: list.is_empty(),
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(wt) = cur.as_mut() else { continue };
        match key {
            "HEAD" => wt.head = val.to_string(),
            "branch" => {
                wt.branch = Some(val.strip_prefix("refs/heads/").unwrap_or(val).to_string())
            }
            "locked" => wt.locked = true,
            "prunable" => wt.prunable = true,
            _ => {}
        }
    }
    list.extend(cur);
    list
}

pub fn add_worktree(repo: &Path, branch: &str, path: &Path, base: Option<&str>) -> Result<()> {
    let path = path.to_str().ok_or_else(|| anyhow!("path is not UTF-8"))?;
    // `--` keeps a base like `--force` from being read as an option.
    let mut args = vec!["worktree", "add", "-b", branch, "--", path];
    args.extend(base);
    git(repo, &args).map(drop)
}

/// Never forces. Git refuses a dirty or locked worktree on its own.
pub fn remove_worktree(repo: &Path, path: &Path) -> Result<()> {
    let path = path.to_str().ok_or_else(|| anyhow!("path is not UTF-8"))?;
    git(repo, &["worktree", "remove", path]).map(drop)
}

/// The top-level directory of the repository that contains `dir`.
pub fn toplevel(dir: &Path) -> Result<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"])
        .map_err(|_| anyhow!("{} is not inside a git repository", dir.display()))?;
    Ok(PathBuf::from(out.trim()))
}

pub fn is_dirty(dir: &Path) -> Result<bool> {
    Ok(!git(dir, &["status", "--porcelain"])?.trim().is_empty())
}

/// Which of `paths`, relative to `dir`, git ignores. A folder needs a
/// trailing `/` to match a pattern such as `target/`.
pub fn ignored(dir: &Path, paths: &[String]) -> Result<HashSet<String>> {
    if paths.is_empty() {
        return Ok(HashSet::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run git")?;
    let mut input = Vec::new();
    for p in paths {
        input.extend_from_slice(p.as_bytes());
        input.push(0);
    }
    // Written from a thread: git may fill its stdout pipe before it reads all of stdin.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().context("run git")?;
    let _ = writer.join();
    // 1 means nothing is ignored. 128 is a real error, such as no repository.
    if !out.status.success() && out.status.code() != Some(1) {
        bail!(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_z() {
        let out = "worktree /code/app\0HEAD aaa\0branch refs/heads/main\0\0\
                   worktree /code/app-worktrees/feat-x\0HEAD bbb\0branch refs/heads/feat/x\0locked\0\0\
                   worktree /tmp/gone\0HEAD ccc\0detached\0prunable gitdir file points to non-existent location\0\0";
        let wts = parse_worktrees(out);
        assert_eq!(wts.len(), 3);
        assert!(wts[0].is_main && !wts[1].is_main && !wts[2].is_main);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[1].path, PathBuf::from("/code/app-worktrees/feat-x"));
        assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
        assert!(wts[1].locked && !wts[1].prunable);
        assert_eq!(wts[2].head, "ccc");
        assert_eq!(wts[2].branch, None);
        assert!(wts[2].prunable);
    }
}

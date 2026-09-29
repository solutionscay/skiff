//! One folder of a worktree, for the sidebar's file tree. The tree only
//! lists and opens: no edits, no previews, no watching. Blocking.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::git;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// The entries of `dir`, folders first, without `.git` and what git ignores.
/// Outside a repository nothing is ignored.
pub fn list_dir(dir: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let e = e?;
        let Ok(name) = e.file_name().into_string() else { continue };
        if name == ".git" {
            continue;
        }
        // Follows symlinks, so a linked folder opens like any other.
        let dir = std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
        entries.push(Entry { name, dir });
    }
    let asked: Vec<String> = entries
        .iter()
        .map(|e| if e.dir { format!("{}/", e.name) } else { e.name.clone() })
        .collect();
    let ignored = git::ignored(dir, &asked).unwrap_or_default();
    entries.retain(|e| !ignored.contains(&e.name) && !ignored.contains(&format!("{}/", e.name)));
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    #[test]
    fn lists_folders_first_without_ignored() {
        let root = std::env::temp_dir().join(format!("skiff-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::write(root.join("README.md"), "").unwrap();
        fs::write(root.join("build.log"), "").unwrap();
        fs::write(root.join("b.txt"), "").unwrap();
        assert!(Command::new("git").arg("init").arg("-q").arg(&root).status().unwrap().success());

        let names: Vec<(String, bool)> = list_dir(&root).unwrap().into_iter().map(|e| (e.name, e.dir)).collect();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            names,
            [("src".into(), true), (".gitignore".into(), false), ("b.txt".into(), false), ("README.md".into(), false)]
        );
    }
}

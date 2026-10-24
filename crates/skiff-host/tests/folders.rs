//! `folders@1`: the picker lists folders only, resolves links, and makes
//! one folder from one name. Private temp folders only.
#![cfg(unix)]

use std::path::PathBuf;

use skiff_host::folders;
use skiff_host_api::{Error, FolderKind};

mod common;
use common::{repo, Guard};

#[test]
fn folders_lists_folders_only_and_create_takes_one_name() {
    let dir = PathBuf::from("/tmp").join(format!("shfo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _guard = Guard(dir.clone());
    let top = dir.to_string_lossy().into_owned();
    repo(&dir.join("api"));
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(dir.join("file.txt"), "x").unwrap();
    std::os::unix::fs::symlink(dir.join("api"), dir.join("link")).unwrap();
    std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("filelink")).unwrap();

    let l = folders::list(&top, false).unwrap();
    let names: Vec<_> = l.entries.iter().map(|e| (e.name.as_str(), e.kind, e.repo.is_some())).collect();
    assert_eq!(names, [("api", FolderKind::Dir, true), ("link", FolderKind::Link, true), ("notes", FolderKind::Dir, false)]);
    assert!(l.entries[0].repo.as_ref().unwrap().branch.is_some());
    assert_eq!(folders::list(&top, true).unwrap().entries.len(), 4, "hidden folders on request");
    // A link opens at its real path.
    let via = folders::list(&format!("{top}/link"), false).unwrap();
    assert_eq!(via.real, folders::list(&format!("{top}/api"), false).unwrap().real);
    assert!(matches!(folders::list("relative", false), Err(Error::Invalid { .. })));
    assert!(matches!(folders::list(&format!("{top}/gone"), false), Err(Error::NotFound { .. })));
    assert!(matches!(folders::list(&format!("{top}/file.txt"), false), Err(Error::NotFound { .. })));

    for bad in ["..", ".", "a/b", "", "a\0b", &"x".repeat(256)] {
        assert!(matches!(folders::create(&top, bad), Err(Error::Invalid { .. })), "{bad:?}");
    }
    assert!(!dir.join("a").exists());
    let made = folders::create(&top, "new one").unwrap();
    assert!(PathBuf::from(&made).is_dir());
    assert!(matches!(folders::create(&top, "new one"), Err(Error::Conflict { .. })));
    assert!(folders::home().unwrap().starts_with('/'));
}

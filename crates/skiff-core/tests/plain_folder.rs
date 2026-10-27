use skiff_core::{config, project::{self, ProjectKind}};

// Its own test binary: SKIFF_CONFIG is process-wide.
#[test]
fn a_folder_in_no_repository_is_a_plain_project_until_it_has_a_git() {
    let dir = std::env::temp_dir().join(format!("skiff-plain-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::env::set_var("SKIFF_CONFIG", dir.join("projects.toml"));

    // A missing folder cannot be added.
    let missing = config::inspect_folder(&dir.join("missing"));
    assert!(missing.root.is_none() && missing.error.is_some());

    // A plain folder is its own root, with no branch and no worktrees.
    let info = config::inspect_folder(&docs);
    assert_eq!(info.error, None);
    assert_eq!(info.root, Some(docs.canonicalize().unwrap()));
    assert_eq!(info.name, "docs");
    assert_eq!((info.branch, info.worktrees), (None, 0));

    let added = config::add_project(&docs, None, None, None, None, vec![]).unwrap();
    assert_eq!(added.name, "docs");
    assert!(config::add_project(&docs, None, None, None, None, vec![]).is_err());

    // It lists as a folder: one worktree, the folder, and no error.
    let p = project::list().unwrap().into_iter().find(|p| p.name == "docs").unwrap();
    assert_eq!(p.kind, ProjectKind::Folder);
    assert_eq!(p.error, None);
    assert_eq!(p.default_branch, None);
    assert_eq!(p.worktrees, vec![project::folder_worktree(&p.path)]);
    assert!(p.worktrees[0].is_main && p.worktrees[0].branch.is_none());
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["kind"], "folder");

    // With a .git when Skiff reads it, the same project is a git project.
    let init = std::process::Command::new("git").arg("-C").arg(&docs).args(["init", "-q"]).status().unwrap();
    assert!(init.success());
    let p = project::list().unwrap().into_iter().find(|p| p.name == "docs").unwrap();
    assert_eq!(p.kind, ProjectKind::Git);
    assert_eq!(p.error, None);
    assert_eq!(p.worktrees.len(), 1);
    assert_eq!(serde_json::to_value(&p).unwrap()["kind"], "git");

    // A project from an older daemon has no kind: it reads as git.
    let mut old = json;
    old.as_object_mut().unwrap().remove("kind");
    assert_eq!(serde_json::from_value::<project::Project>(old).unwrap().kind, ProjectKind::Git);

    // Files: a new project has no key, so its kind sets the default. A choice
    // is written as true or false, so off sticks for a plain folder.
    let files = || config::load().unwrap().projects.into_iter().find(|p| p.name == "docs").unwrap().files;
    assert_eq!(files(), None);
    config::set_project_files("docs", false).unwrap();
    assert_eq!(files(), Some(false));
    let text = std::fs::read_to_string(dir.join("projects.toml")).unwrap();
    assert!(text.contains("files = false"), "{text}");
    config::set_project_files("docs", true).unwrap();
    assert_eq!(files(), Some(true));

    let _ = std::fs::remove_dir_all(&dir);
}

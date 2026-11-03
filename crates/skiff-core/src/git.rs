//! Thin wrappers over the `git` binary. Blocking: async callers use
//! `spawn_blocking`.

mod bounded;
mod diff;
mod repository;
mod root;

use bounded::{bounded, git_bounded, git_in, git_run};
pub(crate) use bounded::Deadline;
use diff::{GIT_TIMEOUT, PREFLIGHT_LIMIT, STATUS_LIMIT};
#[cfg(test)]
use diff::{diff_within, untracked_diff};
pub(crate) use diff::cap;
pub use diff::{diff_in, diff_script, diff_text, is_relative_file, Diff, DEFAULT_DIFF, DIFF_LIMIT};
use repository::{branch_base_with, git};
#[cfg(test)]
use repository::classify_repository;
pub use repository::{add_worktree, branch_base, branches, checkout_worktree, default_branch, head_branch, in_repository, init_repository, is_dirty, list_worktrees, parse_worktrees, remove_worktree, toplevel, Base, Branch, BRANCHES_MAX};
pub use root::{changes, changes_in, Change, Changes, Root};
pub(crate) use root::{ignored_in, open_beneath, Beneath};
#[cfg(test)]
use root::{count_lines, unquote, COUNT_LIMIT};

#[cfg(test)]
use crate::shell::shell_quote;
#[cfg(test)]
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

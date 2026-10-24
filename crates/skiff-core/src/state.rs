//! Whose state a process uses: the person's, or a private copy for its
//! socket. Only the release daemon's default socket serves the person's
//! `projects.toml`, workspace, Skiff agent folder and ledger. Any other
//! socket, a debug build's default included, gets a folder of its own next
//! to the socket, so a test daemon never reads or writes live state.
//! `SKIFF_REAL_STATE=1` opts a process on another socket in. `SKIFF_CONFIG`,
//! `SKIFF_STATE` and `SKIFF_LEDGER` still name a file each.
//!
//! A managed server install (the binaries the app installed under
//! `~/.local/share/skiff/server`) is the one Skiff of its account: its
//! build's own socket is the live one there, a dev build's `skiffd-dev.sock`
//! included. Else its config, workspace and Skipper folder would sit in the
//! runtime folder, which a restart empties.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Set to `1` to use the person's state from another socket.
pub const REAL_ENV: &str = "SKIFF_REAL_STATE";

static SOCKET: OnceLock<PathBuf> = OnceLock::new();

/// The socket this process serves or talks to, when it is not the computed
/// one: the daemon's bound socket, or the CLI's `--socket`. The first call wins.
pub fn set_socket(path: PathBuf) {
    let _ = SOCKET.set(path);
}

/// The socket the state follows.
pub fn socket() -> PathBuf {
    SOCKET.get().cloned().unwrap_or_else(crate::socket::socket_path)
}

/// The socket the installed app and its daemon use: a release build's
/// default, with no `SKIFF_SOCKET`. From a managed server install, its own
/// build's default.
pub fn live_socket() -> PathBuf {
    let name = if managed() { crate::socket::build_socket_name() } else { "skiffd.sock" };
    crate::socket::runtime_dir().join("skiff").join(name)
}

/// This process runs from a managed server install: its binary, links
/// resolved, is under `~/.local/share/skiff/server/`.
pub fn managed() -> bool {
    static MANAGED: OnceLock<bool> = OnceLock::new();
    *MANAGED.get_or_init(|| {
        let (Ok(exe), Some(home)) = (std::env::current_exe(), std::env::var_os("HOME")) else { return false };
        in_managed_folder(&exe, Path::new(&home))
    })
}

fn in_managed_folder(exe: &Path, home: &Path) -> bool {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    real(exe).starts_with(real(&home.join(".local/share/skiff/server")))
}

/// `socket` is the live socket.
pub fn is_live(socket: &Path) -> bool {
    same_socket(socket, &live_socket())
}

/// Two socket paths name the same file. The folders are compared with links
/// resolved, as a socket may not exist yet.
pub fn same_socket(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
        (Some(d), Some(f)) => d.join(f),
        _ => p.to_path_buf(),
    };
    a == b || norm(a) == norm(b)
}

/// The private state folder of `socket`: `x.sock` keeps it in `x.state`.
pub fn dir_for(socket: &Path) -> PathBuf {
    socket.with_extension("state")
}

/// `None`: this process uses the person's state. Else the private folder
/// that holds `config/projects.toml`, `workspace.json` and `skiff-agent/`.
pub fn private_dir() -> Option<PathBuf> {
    if std::env::var(REAL_ENV).is_ok_and(|v| v == "1") {
        return None;
    }
    let s = socket();
    (!is_live(&s)).then(|| dir_for(&s))
}

/// The person's `projects.toml`, whatever this process uses.
pub fn real_config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("skiff").join("projects.toml")
}

/// The Skiff agent folder of the person's state: the `[skiff_agent] folder`
/// in their `projects.toml`, and `~/skiff-agent`. A process on another
/// socket must not run a Skiff agent in either.
pub fn real_agent_folders() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = dirs::home_dir().map(|h| h.join("skiff-agent")).into_iter().collect();
    if let Some(f) = crate::config::load_from(&real_config_path()).ok().and_then(|c| c.skiff_agent.folder) {
        out.push(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_socket_keeps_its_state_beside_it() {
        assert_eq!(dir_for(Path::new("/tmp/x.sock")), Path::new("/tmp/x.state"));
        assert!(same_socket(Path::new("/tmp/./x.sock"), Path::new("/tmp/x.sock")));
        assert!(!same_socket(Path::new("/tmp/x.sock"), Path::new("/tmp/y.sock")));
        assert!(!is_live(Path::new("/tmp/x.sock")));
        assert!(is_live(&live_socket()));
    }

    #[test]
    fn only_a_binary_in_the_managed_folder_is_a_managed_install() {
        let home = Path::new("/home/me");
        assert!(in_managed_folder(Path::new("/home/me/.local/share/skiff/server/2.0.0/skiffd"), home));
        assert!(!in_managed_folder(Path::new("/home/me/code/skiff/target/debug/skiffd"), home));
        assert!(!in_managed_folder(Path::new("/home/me/.local/share/skiff/serverx/skiffd"), home));
        assert!(!managed(), "a test binary is no managed install");
    }
}

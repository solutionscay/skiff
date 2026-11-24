use std::path::PathBuf;

/// The one socket name. Every build, debug or release, of the app, the CLI
/// and skiffd uses it: one computer runs one skiffd, and the newer build
/// updates it in place.
pub const SOCKET_NAME: &str = "skiffd.sock";

/// Where `skiffd` listens. `SKIFF_SOCKET` overrides, for tests and for an
/// agent's throwaway daemon. Otherwise `<runtime dir>/skiff/skiffd.sock`
/// (see [`runtime_dir`]), the same in every build.
pub fn socket_path() -> PathBuf {
    socket_path_from(std::env::var_os("SKIFF_SOCKET").as_deref(), &runtime_dir())
}

/// [`socket_path`] for a given `SKIFF_SOCKET` value and runtime folder.
pub fn socket_path_from(env: Option<&std::ffi::OsStr>, runtime: &std::path::Path) -> PathBuf {
    match env.filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => runtime.join("skiff").join(SOCKET_NAME),
    }
}

/// Where a Host's data-plane service (`skiff-host`) listens, beside the
/// skiffd socket it serves with: `x.sock` gives `x-host.sock`. Pure path
/// arithmetic, so it also names a remote Host's socket without touching
/// this machine.
pub fn data_socket(session: &std::path::Path) -> PathBuf {
    let stem = session.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "skiffd".into());
    session.with_file_name(format!("{stem}-host.sock"))
}

/// The user's runtime folder, the same from the desktop and from an SSH
/// login, where the environment can differ. On Linux, `$XDG_RUNTIME_DIR`,
/// else `/run/user/<uid>` when it exists. On macOS, the per-user temp folder
/// the system names, which is the desktop's `$TMPDIR`. Else `skiff-<uid>` in
/// the temp dir (see [`fallback_runtime_dir`]). `server.sh` picks the same.
pub fn runtime_dir() -> PathBuf {
    system_runtime_dir().unwrap_or_else(fallback_runtime_dir)
}

/// The runtime folder the system names for this account, if any.
fn system_runtime_dir() -> Option<PathBuf> {
    if let Some(d) = dirs::runtime_dir() {
        return Some(d);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::metadata("/proc/self") {
            let d = PathBuf::from(format!("/run/user/{}", m.uid()));
            if d.is_dir() {
                return Some(d);
            }
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(d) = darwin_user_temp() {
        return Some(d);
    }
    None
}

/// With no runtime folder from the system: `<temp dir>/skiff-<uid>`. The
/// temp dir is shared by every account, so the name has the uid and
/// [`prepare_socket_dir`] makes it private or refuses it.
fn fallback_runtime_dir() -> PathBuf {
    #[cfg(unix)]
    // SAFETY: geteuid takes no arguments and cannot fail.
    let uid = unsafe { libc::geteuid() };
    #[cfg(not(unix))]
    let uid = 0;
    std::env::temp_dir().join(format!("skiff-{uid}"))
}

/// Makes the folder of a socket at `socket`, or checks it. skiffd and
/// skiff-host call it before a bind, and the app and the CLI before they
/// write a log or lock beside the socket, so every creator makes it `0700`.
/// - The default folder (`<runtime dir>/skiff`) under a runtime folder the
///   system names: made `0700` if missing; an existing one of this account
///   that is a real folder is kept and loses any group or other bits. A
///   link, a non-folder or another account's folder is refused.
/// - The default folder under the shared temp fallback, and the fallback
///   `skiff-<uid>` above it: made `0700` if missing; an existing one must
///   already be a real folder of this account with no group or other bits.
///   A folder that was ever open to others may hold planted entries, so it
///   is refused, never repaired.
/// - A folder `SKIFF_SOCKET` names: made `0700` if missing, else used as it is.
#[cfg(unix)]
pub fn prepare_socket_dir(socket: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let Some(dir) = socket.parent() else { return Ok(()) };
    let runtime = runtime_dir();
    if dir != runtime.join("skiff") {
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        return Ok(());
    }
    let shared = runtime == fallback_runtime_dir();
    if shared {
        own_dir(&runtime, Loose::Refuse)?;
    } else if !runtime.exists() {
        std::fs::create_dir_all(&runtime)?;
    }
    own_dir(dir, if shared { Loose::Refuse } else { Loose::Tighten })
}

/// [`prepare_socket_dir`] where there are no Unix modes: the folder is made.
#[cfg(not(unix))]
pub fn prepare_socket_dir(socket: &std::path::Path) -> std::io::Result<()> {
    socket.parent().map_or(Ok(()), std::fs::create_dir_all)
}

/// What [`own_dir`] does with a folder of this account that has group or
/// other bits.
#[cfg(unix)]
#[derive(Clone, Copy, PartialEq)]
enum Loose {
    /// Set it to `0700`: its parent is private, so no other account could
    /// reach the folder while it was open.
    Tighten,
    /// Refuse it: its parent is shared.
    Refuse,
}

/// A folder of this account, made `0700` if missing. An existing one that
/// is a link, not a folder, or another account's is refused with an error
/// that names it; one with group or other bits is tightened or refused.
#[cfg(unix)]
fn own_dir(dir: &std::path::Path, loose: Loose) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: geteuid takes no arguments and cannot fail.
    let uid = unsafe { libc::geteuid() };
    let open = meta.permissions().mode() & 0o077 != 0;
    if !meta.is_dir() || meta.uid() != uid || (open && loose == Loose::Refuse) {
        let mode = if loose == Loose::Refuse { ", with mode 0700" } else { "" };
        return Err(std::io::Error::other(format!(
            "{} is not a private folder of this account: it must be a folder, not a link, owned by uid {uid}{mode}",
            dir.display()
        )));
    }
    if open {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// `confstr(_CS_DARWIN_USER_TEMP_DIR)`: what launchd sets `$TMPDIR` to for
/// the desktop. An SSH login can lack `$TMPDIR`.
#[cfg(target_os = "macos")]
fn darwin_user_temp() -> Option<PathBuf> {
    let mut buf = vec![0u8; 1024];
    // SAFETY: confstr writes at most `buf.len()` bytes, NUL included.
    let n = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len()) };
    if n == 0 || n > buf.len() {
        return None;
    }
    buf.truncate(n - 1);
    String::from_utf8(buf).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A debug build resolves the same socket as a release build: there is
    /// one skiffd per computer. Only `SKIFF_SOCKET` names another one.
    #[test]
    fn every_build_resolves_skiffd_sock() {
        let run = Path::new("/run/user/1000");
        assert_eq!(socket_path_from(None, run), Path::new("/run/user/1000/skiff/skiffd.sock"));
        assert_eq!(socket_path_from(Some(std::ffi::OsStr::new("")), run), Path::new("/run/user/1000/skiff/skiffd.sock"));
        assert_eq!(socket_path_from(Some(std::ffi::OsStr::new("/tmp/t/x.sock")), run), Path::new("/tmp/t/x.sock"));
        assert!(socket_path().file_name().is_some_and(|n| n == SOCKET_NAME) || std::env::var_os("SKIFF_SOCKET").is_some());
        assert!(crate::state::is_live(&socket_path_from(None, &runtime_dir())));
    }

    /// A socket folder that is a link or another account's is refused; a
    /// missing one is made 0700. One open to others is refused in the shared
    /// temp fallback, and tightened to 0700 under a private runtime folder.
    #[cfg(unix)]
    #[test]
    fn own_dir_refuses_a_shared_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("skiff-private-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let fresh = root.join("fresh");
        own_dir(&fresh, Loose::Refuse).unwrap();
        assert_eq!(std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777, 0o700);
        let open = root.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(own_dir(&open, Loose::Refuse).is_err());
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o775)).unwrap();
        own_dir(&open, Loose::Tighten).unwrap();
        assert_eq!(std::fs::metadata(&open).unwrap().permissions().mode() & 0o777, 0o700);
        let link = root.join("link");
        std::os::unix::fs::symlink(&fresh, &link).unwrap();
        assert!(own_dir(&link, Loose::Refuse).is_err());
        assert!(own_dir(&link, Loose::Tighten).is_err());
        // Owned by root, unless the test runs as root.
        if unsafe { libc::geteuid() } != 0 {
            assert!(own_dir(Path::new("/"), Loose::Tighten).is_err());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}

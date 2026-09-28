use std::path::PathBuf;

/// Where `skiffd` listens. `SKIFF_SOCKET` overrides. Otherwise
/// `$XDG_RUNTIME_DIR/skiff/skiffd.sock`, falling back to the temp dir.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("SKIFF_SOCKET") {
        return PathBuf::from(p);
    }
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    base.join("skiff").join("skiffd.sock")
}

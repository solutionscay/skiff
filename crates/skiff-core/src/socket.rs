use std::path::PathBuf;

/// Where `skiffd` listens. `SKIFF_SOCKET` overrides. Otherwise
/// `$XDG_RUNTIME_DIR/skiff/skiffd.sock`, falling back to the temp dir.
/// Debug builds use `skiffd-dev.sock`, so a daemon from a checkout never
/// serves the installed app.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("SKIFF_SOCKET") {
        return PathBuf::from(p);
    }
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let name = if cfg!(debug_assertions) { "skiffd-dev.sock" } else { "skiffd.sock" };
    base.join("skiff").join(name)
}

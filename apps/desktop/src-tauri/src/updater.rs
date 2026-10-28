//! Self-update: where the updater plugin may replace the app, and the
//! restart that finishes an update. The new app hands skiffd, the skiff CLI
//! and `skiff watch` over at its launch, as after any install.
use tauri::{AppHandle, Manager, State};

use crate::quit::Guard;

/// A path inside a mounted disk image or a translocated copy. The app there
/// is read-only, and a link into it breaks later.
pub(crate) fn in_disk_image(path: &str) -> bool {
    path.starts_with("/Volumes/") || path.contains("/AppTranslocation/")
}

/// Why this app cannot update itself, in one line; `None` when it can. Apart
/// from the running process, so tests reach it.
fn off(pubkey: &str, debug: bool, kind: Option<&'static str>, exe: &str) -> Option<&'static str> {
    if debug {
        Some("A development build does not update itself.")
    } else if pubkey.trim().is_empty() {
        Some("This build has no update key.")
    } else if kind.is_none() {
        // A binary run from a checkout or a copied file: no package to replace.
        Some("This copy of Skiff is not from a Skiff package.")
    } else if kind == Some("app") && in_disk_image(exe) {
        Some("Move Skiff to Applications to update.")
    } else {
        None
    }
}

fn kind(app: &AppHandle) -> Option<&'static str> {
    use tauri::utils::{config::BundleType, platform::bundle_type};
    match bundle_type() {
        // The updater writes over $APPIMAGE. Without it there is no AppImage file to replace.
        Some(BundleType::AppImage) => app.env().appimage.as_ref().map(|_| "appimage"),
        Some(BundleType::Deb) => Some("deb"),
        Some(BundleType::Rpm) => Some("rpm"),
        Some(BundleType::App) => Some("app"),
        _ => None,
    }
}

#[tauri::command]
pub(crate) fn update_off(app: AppHandle) -> Option<&'static str> {
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .unwrap_or_default();
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    off(pubkey, cfg!(debug_assertions), kind(&app), &exe)
}

/// Called after the update is installed and the page's workspace save guard
/// succeeded. `AppHandle::restart` works on any thread: on the main thread it
/// restarts at once, elsewhere it asks the event loop to exit, which the
/// approval lets through.
#[tauri::command]
pub(crate) fn relaunch_app(app: AppHandle, guard: State<'_, Guard>) {
    guard.approve();
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::off;

    #[test]
    fn updates_only_a_package_with_a_key() {
        assert_eq!(off("KEY", false, Some("appimage"), "/tmp/.mount_x/usr/bin/skiff-desktop"), None);
        assert_eq!(off("KEY", false, Some("deb"), "/usr/bin/skiff-desktop"), None);
        assert!(off("KEY", true, Some("deb"), "/usr/bin/skiff-desktop").is_some());
        assert!(off("  ", false, Some("deb"), "/usr/bin/skiff-desktop").is_some());
        assert!(off("KEY", false, None, "/home/me/skiff/target/release/skiff-desktop").is_some());
    }

    #[test]
    fn a_mac_app_in_a_disk_image_does_not_update() {
        for exe in [
            "/Volumes/Skiff/Skiff.app/Contents/MacOS/skiff-desktop",
            "/private/var/folders/x/AppTranslocation/1234/d/Skiff.app/Contents/MacOS/skiff-desktop",
        ] {
            assert_eq!(off("KEY", false, Some("app"), exe), Some("Move Skiff to Applications to update."));
        }
        assert_eq!(off("KEY", false, Some("app"), "/Applications/Skiff.app/Contents/MacOS/skiff-desktop"), None);
    }
}

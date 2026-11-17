//! The app icon's number on Linux (#275): sessions that wait. Docks such as
//! Ubuntu Dock, Dash to Dock, KDE Plasma's task manager and Plank read the
//! Unity launcher signal on the session bus. Tauri's `setBadgeCount` needs
//! the Unity shell and libunity, so the page uses it only on macOS.

#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use zbus::blocking::Connection;
#[cfg(target_os = "linux")]
use zbus::zvariant::Value;

/// The desktop file the docks know Skiff by: the product name.
#[cfg(target_os = "linux")]
const APP_URI: &str = "application://Skiff.desktop";
#[cfg(target_os = "linux")]
const PATH: &str = "/com/solutionscay/skiff/launcher";

/// One session-bus connection for the app's life. `None` until the first send.
#[cfg(target_os = "linux")]
static BUS: Mutex<Option<Connection>> = Mutex::new(None);

/// Sends the count to the docks. 0 removes the number.
#[cfg(target_os = "linux")]
fn send(count: u32) -> zbus::Result<()> {
    let mut bus = BUS.lock().unwrap_or_else(|e| e.into_inner());
    if bus.is_none() {
        *bus = Some(Connection::session()?);
    }
    let Some(c) = bus.as_ref() else { return Ok(()) };
    let mut props: HashMap<&str, Value> = HashMap::new();
    props.insert("count", Value::I64(count.into()));
    props.insert("count-visible", Value::Bool(count > 0));
    c.emit_signal(None::<&str>, PATH, "com.canonical.Unity.LauncherEntry", "Update", &(APP_URI, props))
}

/// The page's Waiting count changed. No bus (vanilla GNOME has a bus with
/// no listener; that is fine) is not an error to show.
#[tauri::command]
pub(crate) async fn set_waiting_badge(count: u32) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = send(count) {
            eprintln!("badge: the launcher signal did not send: {e}");
        }
    });
    #[cfg(not(target_os = "linux"))]
    let _ = count;
    Ok(())
}

/// The app quits: no number stays on the dock icon.
pub(crate) fn clear() {
    #[cfg(target_os = "linux")]
    if BUS.lock().map(|b| b.is_some()).unwrap_or(false) {
        let _ = send(0);
    }
}


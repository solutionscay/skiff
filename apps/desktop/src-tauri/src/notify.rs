//! This desktop's own notifications, sent through the system notification
//! center on macOS: one per session, with a subtitle, and removed when its
//! pane is seen. A click sends [`CLICK_EVENT`] with the session id.
//!
//! Only a bundled app may use the center. Elsewhere, and on Linux and Windows,
//! `notify_show` returns an error and the page sends through the notification
//! plugin instead.

#[cfg(target_os = "macos")]
mod center;

/// The event a click on a notification sends to the page, with the session id.
pub(crate) const CLICK_EVENT: &str = "skiff:notify-click";

/// The request id of a session's notification. A newer one replaces the older.
fn request_id(session: &str) -> String {
    format!("session-{session}")
}

/// The session id of a request id, for a click.
fn session_of(request: &str) -> Option<&str> {
    request.strip_prefix("session-").filter(|s| !s.is_empty())
}

/// Sets the click handler, when this app may use the center. Call it once, at setup.
pub(crate) fn install(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    center::install(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Shows the session's notification and replaces its older one. The first call
/// asks for permission. An error means the page sends through the plugin.
#[tauri::command]
pub(crate) async fn notify_show(session: String, title: String, subtitle: String, body: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return center::show(&request_id(&session), &session, &title, &subtitle, &body).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (session, title, subtitle, body);
        Err("No notification center on this system.".into())
    }
}

/// Removes the session's notification, shown or not yet shown.
#[tauri::command]
pub(crate) fn notify_remove(session: String) {
    #[cfg(target_os = "macos")]
    center::remove(&request_id(&session));
    #[cfg(not(target_os = "macos"))]
    let _ = session;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_finds_the_session_of_its_request() {
        assert_eq!(session_of(&request_id("ab12")), Some("ab12"));
        assert_eq!(session_of("session-"), None);
        assert_eq!(session_of("other"), None);
    }
}

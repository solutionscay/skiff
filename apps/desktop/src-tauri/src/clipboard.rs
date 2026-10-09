//! The clipboard through GTK on the main thread, Linux only. The webview's own
//! write needs a user gesture, and a key the native menu takes as an
//! accelerator reaches the page as a menu event, with no gesture. The
//! clipboard plugin reaches for X11 and fails on a Wayland session. GTK writes
//! to the display the window is on, Wayland or X11, and serves the data from
//! the main loop for as long as the app runs.
//!
//! Not async, so both run on the main thread, where GTK lives. Elsewhere they
//! fail, and the page falls back to the webview and the plugin.

#[tauri::command]
pub(crate) fn clipboard_write(text: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).set_text(&text);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = text;
        Err("no GTK clipboard here".into())
    }
}

/// The clipboard's text, or an empty string when it holds none.
#[tauri::command]
pub(crate) fn clipboard_read() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        Ok(gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD)
            .wait_for_text()
            .map(|s| s.to_string())
            .unwrap_or_default())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("no GTK clipboard here".into())
    }
}

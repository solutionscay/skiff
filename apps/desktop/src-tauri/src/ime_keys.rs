//! App keys that the Linux input method takes before the page sees them.
//! GTK and IBus start Unicode entry on Ctrl+Shift+U and ignore Alt, so
//! Ctrl+Shift+Alt+U (the Usage key) reaches the page as "Unidentified".
//! The window catches that key first and sends it to the page, which runs
//! it as a keydown through the keymap.

use gtk::prelude::*;
use tauri::{AppHandle, Emitter, WebviewWindow};

/// Catches Ctrl+Shift+Alt+U before the input method and the menu accelerators.
pub(crate) fn install(app: &AppHandle, window: &WebviewWindow) -> tauri::Result<()> {
    let win = window.gtk_window()?;
    let app = app.clone();
    win.connect_key_press_event(move |_, e| {
        use gtk::gdk::{keys::constants as k, ModifierType as M};
        let mods = e.state() & gtk::accelerator_get_default_mod_mask();
        let key = e.keyval().to_lower();
        if mods != M::CONTROL_MASK | M::SHIFT_MASK | M::MOD1_MASK || key != k::u {
            return gtk::glib::Propagation::Proceed;
        }
        let _ = app.emit("skiff:ime-key", "KeyU");
        gtk::glib::Propagation::Stop
    });
    Ok(())
}

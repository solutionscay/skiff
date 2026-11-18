//! Quit: File › Quit and the quit key. Sessions keep running in skiffd.
use tauri::AppHandle;

#[tauri::command]
pub(crate) fn quit_app(app: AppHandle) {
    app.exit(0);
}

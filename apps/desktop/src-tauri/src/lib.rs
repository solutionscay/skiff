//! The desktop app is a client of `skiffd`. It owns no PTY.

mod connection;
mod commands;
mod diagnostics;
mod streaming;
mod native_menu;
#[cfg(target_os = "linux")]
mod header_bar;

use connection::App;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK paints a blank window on NVIDIA under Wayland when it renders
    // through DMA-BUF. Turn that path off there, unless the user chose
    // otherwise. Intel and AMD keep it: it is the faster path.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
        && std::path::Path::new("/proc/driver/nvidia").exists()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(App::default())
        // The window starts hidden: GTK takes a header bar only before it shows.
        .setup(|app| {
            let Some(window) = app.get_webview_window("main") else { return Ok(()) };
            #[cfg(target_os = "linux")]
            if header_bar::wanted() {
                header_bar::install(app.handle(), &window)?;
            }
            window.show()?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            let _ = app.emit("skiff:menu", event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            native_menu::set_menu,
            native_menu::quit_app,
            native_menu::menu_layout,
            native_menu::set_menu_layout,
            native_menu::set_window_dark,
            connection::daemon_status,
            commands::list_sessions,
            commands::create_session,
            commands::pty_write,
            commands::pty_resize,
            commands::kill_session,
            commands::rename_session,
            commands::set_session_theme,
            commands::list_projects,
            commands::add_worktree,
            commands::remove_worktree,
            commands::add_project,
            connection::restart_daemon,
            commands::inspect_folder,
            commands::set_project_icon,
            commands::set_project_color,
            commands::set_project_closed,
            commands::remove_project,
            commands::set_project_background,
            commands::read_image,
            commands::list_dir,
            commands::git_changes,
            commands::git_diff,
            commands::open_settings,
            commands::set_open,
            commands::open_file,
            commands::reveal_file,
            commands::files_projects,
            commands::hidden_changes_projects,
            commands::set_project_changes,
            commands::set_project_files,
            commands::project_themes,
            commands::set_project_theme,
            commands::reorder_projects,
            commands::read_icon,
            commands::list_themes,
            commands::get_appearance,
            diagnostics::memory_usage,
            commands::get_keys,
            commands::config_path,
            diagnostics::trace_info,
            diagnostics::trace_write,
            commands::open_config,
            commands::set_appearance,
            commands::set_font_size,
            commands::list_agents,
            commands::set_agents,
            commands::set_agent_command,
            commands::rename_agent,
            commands::remove_agent,
            commands::restore_agents,
            commands::list_groups,
            commands::save_group,
            commands::delete_group,
            streaming::subscribe_output,
            streaming::ack_output,
            streaming::unsubscribe_output,
            streaming::subscribe_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

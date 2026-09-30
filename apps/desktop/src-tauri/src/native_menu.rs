use serde::Deserialize;
use tauri::{AppHandle, menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder}};

#[derive(Deserialize)]
pub(crate) struct MenuSpec {
    title: String,
    items: Vec<MenuItemSpec>,
}

#[derive(Deserialize)]
struct MenuItemSpec {
    /// None draws a separator.
    id: Option<String>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    accel: String,
    #[serde(default)]
    enabled: bool,
}

/// The menu bar. The page owns the command list and sends it whole on every
/// change; a click comes back as `skiff:menu` with the item's id.
/// Closes the app. Sessions live in skiffd and keep running.
#[tauri::command]
pub(crate) fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub(crate) fn set_menu(app: AppHandle, menus: Vec<MenuSpec>) -> Result<(), String> {
    let mut bar = MenuBuilder::new(&app);
    for m in &menus {
        let mut sub = SubmenuBuilder::new(&app, &m.title);
        for it in &m.items {
            let Some(id) = &it.id else {
                sub = sub.separator();
                continue;
            };
            let item = |accel: &str| {
                let b = MenuItemBuilder::with_id(id, &it.label).enabled(it.enabled);
                if accel.is_empty() { b.build(&app) } else { b.accelerator(accel).build(&app) }
            };
            // A key the menu cannot parse stays with the page, which still handles it.
            let item = item(&it.accel).or_else(|_| item("")).map_err(|e| e.to_string())?;
            sub = sub.item(&item);
        }
        bar = bar.item(&sub.build().map_err(|e| e.to_string())?);
    }
    app.set_menu(bar.build().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(())
}

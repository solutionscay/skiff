use anyhow::Result;
use super::persistence::edit_table;

/// Writes `[agents] enabled = [...]`, keeping the rest of the file.
pub fn set_enabled_agents(enabled: &[String]) -> Result<()> {
    let mut list = toml_edit::Array::new();
    for a in enabled {
        list.push(a.as_str());
    }
    edit_agents(|t| {
        t["enabled"] = toml_edit::value(list);
        Ok(())
    })
}

/// Sets `[agents.commands] <id> = "<command>"`. An empty command removes the
/// override, so the default applies again.
pub fn set_agent_command(id: &str, command: &str) -> Result<()> {
    let command = command.trim();
    edit_agents(|t| {
        let cmds = t
            .entry("commands")
            .or_insert_with(|| {
                let mut c = toml_edit::Table::new();
                c.set_implicit(false);
                toml_edit::Item::Table(c)
            })
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("`agents.commands` is not a table"))?;
        if command.is_empty() {
            cmds.remove(id);
        } else {
            cmds[id] = toml_edit::value(command);
        }
        Ok(())
    })
}

/// Sets or removes `icon` on the named project. `None` removes the key, so
/// Sets `[appearance] theme`, or removes it for the default.
pub fn set_app_theme(theme: Option<&str>) -> Result<()> {
    edit_table("appearance", |t| {
        match theme {
            Some(v) => t["theme"] = toml_edit::value(v),
            None => {
                t.remove("theme");
            }
        }
        Ok(())
    })
}

/// Sets `[appearance] font_size`, or removes it for the default.
pub fn set_font_size(size: Option<u8>) -> Result<()> {
    edit_table("appearance", |t| {
        match size {
            Some(v) => t["font_size"] = toml_edit::value(i64::from(v)),
            None => {
                t.remove("font_size");
            }
        }
        Ok(())
    })
}

/// The values `[appearance] menu_layout` takes.
pub const MENU_LAYOUTS: [&str; 2] = ["header-bar", "menu-bar"];

/// Sets `[appearance] menu_layout`, or removes it for the automatic choice.
pub fn set_menu_layout(layout: Option<&str>) -> Result<()> {
    if let Some(v) = layout.filter(|v| !MENU_LAYOUTS.contains(v)) {
        anyhow::bail!("unknown menu layout: {v}");
    }
    edit_table("appearance", |t| {
        match layout {
            Some(v) => t["menu_layout"] = toml_edit::value(v),
            None => {
                t.remove("menu_layout");
            }
        }
        Ok(())
    })
}

/// The keys `[open]` takes.
pub const OPEN_KEYS: [&str; 4] = ["diff", "text", "markdown", "html"];

/// Sets one `[open]` command, or removes it for the default.
pub fn set_open(key: &str, command: Option<&str>) -> Result<()> {
    if !OPEN_KEYS.contains(&key) {
        anyhow::bail!("unknown [open] key: {key}");
    }
    edit_table("open", |t| {
        match command {
            Some(v) => t[key] = toml_edit::value(v),
            None => {
                t.remove(key);
            }
        }
        Ok(())
    })
}

fn edit_agents(f: impl FnOnce(&mut toml_edit::Table) -> Result<()>) -> Result<()> {
    edit_table("agents", f)
}

use super::{SessionPool, Group, Event};
use anyhow::{anyhow, bail, Result};
use std::path::Path;

impl SessionPool {
    pub fn list_groups(&self) -> Vec<Group> {
        self.groups.lock().unwrap().clone()
    }

    /// Creates the group when `id` is empty, else replaces it.
    pub fn save_group(&self, mut group: Group) -> Result<Group> {
        let mut groups = self.groups.lock().unwrap();
        let ids = group.layout.sessions();
        {
            let sessions = self.sessions.read().unwrap();
            for (i, s) in ids.iter().enumerate() {
                if ids[..i].contains(s) {
                    bail!("session {s} appears twice in the layout");
                }
                if !skiff_core::group::is_slot(s) && !sessions.contains_key(s) {
                    bail!("no such session: {s}");
                }
            }
        }
        if group.focus.as_ref().is_some_and(|f| !ids.contains(f)) {
            group.focus = None;
        }
        group.layout.clamp_ratios();
        if group.id.is_empty() {
            group.id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            groups.push(group.clone());
        } else {
            let slot = groups
                .iter_mut()
                .find(|g| g.id == group.id)
                .ok_or_else(|| anyhow!("no such group: {}", group.id))?;
            *slot = group.clone();
        }
        drop(groups);
        let _ = self.events.send(Event::GroupsChanged {});
        self.dirty.notify_one();
        Ok(group)
    }

    pub fn delete_group(&self, id: &str) -> Result<()> {
        let mut groups = self.groups.lock().unwrap();
        let before = groups.len();
        groups.retain(|g| g.id != id);
        if groups.len() == before {
            bail!("no such group: {id}");
        }
        drop(groups);
        let _ = self.events.send(Event::GroupsChanged {});
        self.dirty.notify_one();
        Ok(())
    }
}

/// Drops `id` from every group. A group left with no session keeps an empty
/// slot, and the session's folder so it stays in that worktree. True when a
/// group changed.
pub(super) fn prune(groups: &mut [Group], id: &str, cwd: &Path) -> bool {
    let mut pruned = false;
    for g in groups.iter_mut() {
        if !g.layout.sessions().iter().any(|s| s == id) {
            continue;
        }
        pruned = true;
        match g.layout.without(id) {
            Some(layout) => {
                g.layout = layout;
                if g.focus.as_deref() == Some(id) {
                    g.focus = g.layout.sessions().into_iter().next();
                }
            }
            None => {
                if !cwd.as_os_str().is_empty() {
                    g.cwd.get_or_insert_with(|| cwd.to_string_lossy().into_owned());
                }
                g.layout = skiff_core::group::Layout::Pane {
                    session: format!("slot:{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                };
                g.focus = None;
            }
        }
    }
    pruned
}

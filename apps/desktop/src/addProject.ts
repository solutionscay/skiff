import { invoke } from "@tauri-apps/api/core";
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { showError } from "./alerts";
import type { FolderInfo, Project } from "./types";

/**
 * Pick a folder, check it with the daemon, and add it with the defaults: the
 * repo's name, letters or its own icon, the next free color. The project's
 * right-click menu changes the icon and color afterwards.
 */
export function createAddProject(onAdded: (p: Project) => void, onClose: () => void) {
  let busy = false;
  return {
    async open() {
      if (busy) return;
      busy = true;
      try {
        const dir = await openFolder({ directory: true, title: "Pick a folder in a git repository" }).catch(() => null);
        if (typeof dir !== "string") return onClose();
        const folder = await invoke<FolderInfo>("inspect_folder", { path: dir });
        if (!folder.root || folder.error) throw new Error(folder.error ?? "Pick a folder in a git repository.");
        const p = await invoke<Project>("add_project", {
          path: folder.root,
          name: folder.name,
          short: folder.short || null,
          color: folder.color,
          icon: null,
        });
        onAdded(p);
      } catch (e) {
        showError(e);
        onClose();
      } finally {
        busy = false;
      }
    },
    get isOpen() {
      return busy;
    },
  };
}

/**
 * Picks an agent's program. On this computer, the system's file dialog. On a
 * remote Host, the folder picker, then a menu of the programs in that folder
 * (`agent_programs@1`). The path is the Host's: it goes only into a Host request.
 */
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { hostInvoke } from "../app/hostCaps";
import { isRemote } from "../app/hosts";
import { openFolderPicker } from "../workspace/folderPicker";
import { ctxMenu } from "../ui/contextMenu";
import { t } from "../i18n";

export async function pickProgram(title: string, done: (path: string) => void) {
  if (!isRemote()) {
    const path = await openFile({ multiple: false, directory: false, title }).catch(() => null);
    if (typeof path === "string") done(path);
    return;
  }
  const back = document.activeElement as HTMLElement | null;
  openFolderPicker({
    action: t("settings.agents.useFolder"),
    pick: async (folder) => {
      const names = await hostInvoke<string[]>("agent_programs", "program_list", { path: folder });
      if (!names.length) throw t("settings.agents.noPrograms");
      // After the picker closes, so the menu is on top and takes the keys.
      setTimeout(() => {
        const r = back?.getBoundingClientRect();
        const entries = names.map((name) => ({ label: name, run: () => done(`${folder.replace(/\/$/, "")}/${name}`) }));
        ctxMenu.open(r?.left ?? 0, r?.bottom ?? 0, title, entries, () => back?.focus());
      });
    },
  });
}

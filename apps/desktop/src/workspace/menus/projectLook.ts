import { promptAction } from "../../ui/confirm";
import { pickColor } from "../../appearance/colorPicker";
import { pickBackgroundOpacity } from "../../appearance/backdropOpacity";
import { t } from "../../i18n";
import { type MenuEntry } from "../../ui/menu";
import type { Project } from "../../platform/types";
import { invoke } from "@tauri-apps/api/core";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { showError } from "../../ui/alerts";
import { refocusTerminal } from "../view";
import { hostInvoke } from "../../app/hostCaps";
import { isRemote, hostName } from "../../app/hosts";

/** A local picker can only name this computer's file. Put it on a remote
 * Host before its project config refers to it. */
async function hostImage(file: string): Promise<string> {
  if (!isRemote()) return file;
  return hostInvoke<string>("drops", "project_image_upload", { path: file, host: hostName() });
}

async function setIcon(p: Project, icon: string | null) {
  try {
    await invoke("set_project_icon", { project: p.name, icon });
  } catch (e) {
    showError(e);
  }
}

const DEFAULT_COLOR = "#b69cff";

/** The color picker for a project's accent. */
export function pickProjectColor(p: Project) {
  pickColor({
    title: t("color.title", { name: p.name }),
    start: p.color ?? DEFAULT_COLOR,
    action: t("color.action"),
    submit: (hex) => hostInvoke("project_config", "set_project_color", { project: p.name, color: hex }),
    onClose: refocusTerminal,
  });
}

async function setBackground(p: Project, background: string | null) {
  try {
    await invoke("set_project_background", { project: p.name, background });
  } catch (e) {
    showError(e);
  }
}

/** One image behind every pane of the project's terminal area. */
export function backgroundEntries(p: Project): MenuEntry[] {
  return [
    {
      icon: "playback-image-plus",
      label: t("menu.chooseImage"),
      run: async () => {
        const file = await openFile({
          title: t("background.title", { name: p.name }),
          defaultPath: p.path,
          filters: [{ name: t("dialog.imageFiles"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "svg"] }],
        }).catch(() => null);
        if (typeof file === "string") await setBackground(p, await hostImage(file));
      },
    },
    // Only an image shows through the panes, so the opacity waits for one.
    ...(p.background ? [{ icon: "playback-image", label: t("menu.opacity"), run: () => pickBackgroundOpacity(p) }] : []),
    { icon: "indicators-minus", label: t("menu.none"), disabled: !p.background, run: () => void setBackground(p, null) },
  ];
}

/** One place for the project icon: pick a file, detect one, or use letters. */
export function iconEntries(p: Project): MenuEntry[] {
  return [
    {
      icon: "documents-folder-open",
      label: t("menu.chooseFile"),
      run: async () => {
        const file = await openFile({
          title: t("icon.title", { name: p.name }),
          defaultPath: p.path,
          filters: [{ name: t("dialog.imageFiles"), extensions: ["png", "svg", "ico", "jpg", "jpeg", "webp", "gif"] }],
        }).catch(() => null);
        if (typeof file !== "string") return;
        const image = await hostImage(file);
        const rel = !isRemote() && image.startsWith(p.path + "/") ? image.slice(p.path.length + 1) : image;
        await setIcon(p, rel);
      },
    },
    { icon: "schedule-refresh-cw", label: t("menu.detect"), run: () => void setIcon(p, null) },
    { icon: "indicators-minus", label: t("menu.lettersOnly"), run: () => void setIcon(p, "") },
  ];
}

/** Ask for the letters on the project's row. An empty field goes back to the first three letters of the name. */
export function setInitials(p: Project) {
  promptAction({
    title: t("initials.title", { name: p.name }),
    body: t("initials.body"),
    placeholder: t("initials.placeholder"),
    value: p.short,
    allowEmpty: true,
    maxLength: 3,
    action: t("initials.action"),
    submit: (short) => hostInvoke("project_config", "set_project_short", { project: p.name, short }),
    onClose: refocusTerminal,
  });
}

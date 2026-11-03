import { ctxMenu } from "../ui/contextMenu";
/** Right-click menus for projects, sessions, panes and groups. */
import { promptAction } from "../ui/confirm";
import { pickColor } from "../appearance/colorPicker";
import { pickBackgroundOpacity } from "../appearance/backdropOpacity";
import { keyLabel } from "../app/keys";
import { t, upper } from "../i18n";
import { type MenuEntry } from "../ui/menu";
import { isRemote } from "../app/hosts";
import { branchName, isFolder } from "./model";
import type { Project } from "../platform/types";
import { projectWorkspaceEntries } from "./profile";
import { invoke } from "@tauri-apps/api/core";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { openPath } from "../platform/os";
import { launchMenu } from "../app/panels";
import { showError } from "../ui/alerts";
import { setShowChanges, showsChanges } from "./changes";
import { setShowFiles, showsFiles } from "./files";
import { closeEntries } from "./projectClose";
import { bareWorktree } from "./tree";

import { projectThemeMenu } from "../appearance/themes";
import { refocusTerminal } from "./view";
import { hostCan, hostInvoke } from "../app/hostCaps";
import { endEntry } from "./menus/end";
import { projectSkipperEntries } from "./menus/skipper";
import { setFleet } from "./menus/fleet";
import { newWorktree, plusOf } from "./menus/worktree";
export { newWorktree, worktreeMenu } from "./menus/worktree";
export { fleetMenu } from "./menus/fleet";
export { deleteKeyMenu, rowMenu } from "./menus/row";
export { paneMenu, skiffAgentMenu, splitMenu } from "./menus/pane";
export { skipperStart } from "./menus/skipper";
export { groupCloseEntries, groupMenu, killGroup, ungroup } from "./menus/group";
export { endEntry };

async function setIcon(p: Project, icon: string | null) {
  try {
    await invoke("set_project_icon", { project: p.name, icon });
  } catch (e) {
    showError(e);
  }
}

const DEFAULT_COLOR = "#b69cff";

/** The color picker for a project's accent. */
function pickProjectColor(p: Project) {
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
function backgroundEntries(p: Project): MenuEntry[] {
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
        if (typeof file === "string") await setBackground(p, file);
      },
    },
    // Only an image shows through the panes, so the opacity waits for one.
    ...(p.background ? [{ icon: "playback-image", label: t("menu.opacity"), run: () => pickBackgroundOpacity(p) }] : []),
    { icon: "indicators-minus", label: t("menu.none"), disabled: !p.background, run: () => void setBackground(p, null) },
  ];
}

/** One place for the project icon: pick a file, detect one, or use letters. */
function iconEntries(p: Project): MenuEntry[] {
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
        const rel = file.startsWith(p.path + "/") ? file.slice(p.path.length + 1) : file;
        await setIcon(p, rel);
      },
    },
    { icon: "schedule-refresh-cw", label: t("menu.detect"), run: () => void setIcon(p, null) },
    { icon: "indicators-minus", label: t("menu.lettersOnly"), run: () => void setIcon(p, "") },
  ];
}

/** Ask for the letters on the project's row. An empty field goes back to the first three letters of the name. */
function setInitials(p: Project) {
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

/** What each project key does to the selected project. The project menu runs the same. */
export function projectRun(p: Project) {
  return {
    "project-folder": () => void openPath(p.path).catch(showError),
    "project-copy-path": () => void navigator.clipboard.writeText(p.path).catch(showError),
    "project-color": () => pickProjectColor(p),
    "project-changes": () => void (isFolder(p) || setShowChanges(p, !showsChanges(p))),
    "project-files": () => void setShowFiles(p, !showsFiles(p)),
  } as const;
}

/** The project row's ⋯, a right-click on it, or the menu key. */
export function projectMenu(p: Project, x: number, y: number) {
  const run = projectRun(p);
  // A remote Host's path is that Host's: nothing here opens it on this Mac,
  // and its images are paths there, which no capability sends yet. Its
  // settings and worktrees change on that Host, when it takes the change.
  const local = !isRemote();
  const edits = hostCan("project_config");
  const starts = hostCan("session_lifecycle");
  // One worktree: its start choices show here, as on a worktree's +. More: pick the worktree first.
  // A Host that starts no sessions shows New session, off.
  const bare = bareWorktree(p);
  const off = !!p.error || !p.worktrees.length || !starts;
  const start: MenuEntry[] = bare && !p.error && starts
    ? launchMenu.entries(p, bare)
    : [{ icon: "indicators-plus", label: t("menu.newSessionIn"), disabled: off, sub: p.worktrees.map((w) => ({
        icon: "code-git-branch",
        label: branchName(w),
        run: () => launchMenu.open(plusOf(w) ?? { x, y }, p, w),
      })) }];
  // New group stands on its own. One worktree: its layouts. More: the
  // worktree, then its layouts at that worktree's +, since a flyout goes one
  // level deep. The start choices of one worktree carry it already.
  const group: MenuEntry | false = !(bare && !p.error && starts) && {
    icon: "code-group",
    label: t("select.newGroup"),
    disabled: off,
    sub: bare
      ? launchMenu.groups(p, bare)
      : p.worktrees.map((w) => ({
          icon: "code-git-branch",
          label: branchName(w),
          run: () => {
            const r = plusOf(w)?.getBoundingClientRect();
            ctxMenu.open(r ? r.right : x, r ? r.top : y, branchName(w), launchMenu.groups(p, w));
          },
        })),
  };
  // How the project looks. Icon and Background are sections: a flyout goes one level deep.
  const appearance = tidy([
    edits && { icon: "tools-sparkles", label: t("menu.color"), hint: keyLabel("project-color"), run: run["project-color"] },
    edits && { icon: "tools-sparkles", label: t("menu.projectTheme"), hint: keyLabel("theme"), run: () => void projectThemeMenu(p) },
    edits && { icon: "tools-pencil", label: t("menu.initials"), run: () => setInitials(p) },
    ...(local ? [{ head: upper(t("menu.headIcon")) }, ...iconEntries(p), { head: upper(t("menu.headBackground")) }, ...backgroundEntries(p)] : []),
  ]);
  // What the project's row shows in the sidebar. A check marks what shows.
  const sidebar = tidy([
    // A plain folder has no git: no Changes.
    edits && !isFolder(p) && { icon: showsChanges(p) ? "indicators-check" : "", label: t("menu.changes"), hint: keyLabel("project-changes"), run: run["project-changes"] },
    edits && hostCan("files") && { icon: showsFiles(p) ? "indicators-check" : "", label: t("menu.files"), hint: keyLabel("project-files"), run: run["project-files"] },
  ]);
  // This Mac starts them itself; a remote Host, through its skiff-host.
  // The start choices carry the Skipper first when they show; else its items lead the menu.
  const carried = !!bare && !p.error && starts;
  const skipper = local || hostCan("skipper_run") ? projectSkipperEntries(p, !carried) : [];
  ctxMenu.open(x, y, p.path, tidy([
    ...(carried ? [] : skipper),
    { sep: true },
    ...start,
    group,
    !!bare && { sep: true },
    // A plain folder has no git: no worktrees.
    hostCan("worktrees") && !isFolder(p) && { icon: "code-git-branch", label: t("cmd.newWorktree"), hint: keyLabel("new-worktree"), disabled: !!p.error, run: () => newWorktree(p) },
    ...(carried ? skipper : []),
    { sep: true },
    local && { icon: "indicators-square-arrow-out-up-right", label: t("menu.showInFileManager"), hint: keyLabel("project-folder"), run: run["project-folder"] },
    { icon: "code-copy", label: t("menu.copyPath"), hint: keyLabel("project-copy-path"), run: run["project-copy-path"] },
    appearance.length > 0 && { icon: "tools-sparkles", label: t("menu.appearance"), sub: appearance },
    sidebar.length > 0 && { icon: "layouts-panel-left", label: t("menu.sidebar"), sub: sidebar },
    { sep: true },
    // Where the project sits: its fleet, then the workspace. The Host keeps its fleets.
    edits && { icon: "code-group", label: t(p.fleet ? "menu.moveToFleet" : "menu.addToFleet"), run: () => setFleet(p) },
    ...(local ? projectWorkspaceEntries(p) : []),
    ...(edits ? closeEntries(p) : []),
  ]));
}

/** Drops the entries a Host gate left out, and any rule that no longer sits
 *  between two items: none first, last or twice in a row. */
function tidy(entries: (MenuEntry | false)[]): MenuEntry[] {
  const out: MenuEntry[] = [];
  for (const e of entries) {
    if (!e) continue;
    if ("sep" in e && (!out.length || "sep" in out[out.length - 1])) continue;
    out.push(e);
  }
  while (out.length && "sep" in out[out.length - 1]) out.pop();
  return out;
}

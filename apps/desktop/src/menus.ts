/** Right-click menus for projects, sessions, panes and groups. */
import { launchIcon } from "./agentIcon";
import { confirmAction, promptAction } from "./confirm";
import { keyLabel } from "./keys";
import { createMenu, type MenuEntry } from "./menu";
import { agentName, basename, branchName, byStart, taskTitle } from "./model";
import type { Group, Project, SessionInfo, SplitDir, Worktree } from "./types";
import { invoke } from "@tauri-apps/api/core";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { loadProjects, newSession } from "./daemon";
import { showError } from "./dom";
import { deleteGroup } from "./groups";
import { sessionsOf } from "./layout";
import { startRename, startSessionRename } from "./rename";
import { render } from "./render";
import { clearSelection, selectionPlan, splitSelection } from "./selection";
import { enabledAgents, FULL_HINT, groupOf, MAX_PANES, place, removeErrors, removing, S, selectedWorktree, sessions, shownIds, splitFull } from "./state";
import { groupThemeMenu, sessionThemeMenu, themeIdFor } from "./themes";
import { closePane, focusPane, refocusTerminal, removeFromGroup, splitWith, unsplit } from "./view";

async function setIcon(p: Project, icon: string | null) {
  try {
    await invoke("set_project_icon", { project: p.name, icon });
  } catch (e) {
    showError(e);
  }
}

async function setColor(p: Project, color: string) {
  try {
    await invoke("set_project_color", { project: p.name, color });
  } catch (e) {
    showError(e);
  }
}

/** The same accents new projects get, from skiff-core's PALETTE. */
const SWATCHES = ["#b69cff", "#f28fd0", "#7ee0cb", "#e0c07e", "#9ec1ff", "#ff9e7a", "#c3e88d", "#d0c2ff"];

function swatch(color: string): HTMLElement {
  const s = document.createElement("span");
  s.className = "lm-key";
  s.style.cssText = `width:12px;height:12px;background:${color};justify-self:center;align-self:center`;
  return s;
}

/** A second menu of accent colors, plus a picker for any other. */
function colorEntries(p: Project): MenuEntry[] {
  // The webview's own color input does not open on Linux, so ask for a hex code.
  const custom = () =>
    promptAction({
      title: `Color for ${p.name}`,
      body: "A hex color, for example #ff9e7a.",
      placeholder: p.color ?? "#rrggbb",
      action: "Set color",
      submit: async (v) => {
        const hex = v.trim().replace(/^#?/, "#").toLowerCase();
        if (!/^#[0-9a-f]{6}$/.test(hex)) throw new Error("Use six hex digits, like #ff9e7a.");
        await invoke("set_project_color", { project: p.name, color: hex });
      },
      onClose: refocusTerminal,
    });
  return [
    ...SWATCHES.map((c) => ({ glyph: swatch(c), label: c, hint: c === p.color ? "current" : "", run: () => void setColor(p, c) })),
    { icon: "◐", label: "Custom…", hint: "hex code", run: custom },
  ];
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
      icon: "▨",
      label: "Choose image…",
      hint: "png, jpg, webp",
      run: async () => {
        const file = await openFile({
          title: `Background for ${p.name}`,
          defaultPath: p.path,
          filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "svg"] }],
        }).catch(() => null);
        if (typeof file === "string") await setBackground(p, file);
      },
    },
    { icon: "∅", label: "None", hint: "plain background", disabled: !p.background, run: () => void setBackground(p, null) },
  ];
}

/** One place for the rail icon: pick a file, detect one, or use letters. */
function iconEntries(p: Project): MenuEntry[] {
  return [
    {
      icon: "◧",
      label: "Choose file…",
      hint: "png, svg, ico",
      run: async () => {
        const file = await openFile({
          title: `Icon for ${p.name}`,
          defaultPath: p.path,
          filters: [{ name: "Images", extensions: ["png", "svg", "ico", "jpg", "jpeg", "webp", "gif"] }],
        }).catch(() => null);
        if (typeof file !== "string") return;
        const rel = file.startsWith(p.path + "/") ? file.slice(p.path.length + 1) : file;
        await setIcon(p, rel);
      },
    },
    { icon: "↺", label: "Detect", hint: "favicon, logo", run: () => void setIcon(p, null) },
    { icon: "∅", label: "Letters only", hint: "no icon", run: () => void setIcon(p, "") },
  ];
}

/** Ask for a branch, then make a worktree for it. Errors stay in the dialog. */
export function newWorktree(p: Project) {
  S.justAdded = null;
  promptAction({
    title: `New worktree in ${p.name}`,
    body: "Git makes a new branch and checks it out in its own folder next to the project.",
    placeholder: "a branch name",
    action: "Create",
    submit: async (branch) => {
      const w = await invoke<Worktree>("add_worktree", { project: p.name, branch, base: null });
      selectedWorktree.set(p.name, w.path);
      await loadProjects();
    },
    onClose: refocusTerminal,
  });
}

/** The project's + in the sidebar, or a right-click on its rail chip. */
export function projectMenu(p: Project, x: number, y: number) {
  ctxMenu.open(x, y, p.path, [
    { icon: "+", label: "New worktree…", disabled: !!p.error, run: () => newWorktree(p) },
    { icon: "↗", label: "Show in file manager", run: () => void openPath(p.path).catch(showError) },
    { icon: "⧉", label: "Copy path", run: () => void navigator.clipboard.writeText(p.path).catch(showError) },
    { icon: "◧", label: "Icon…", hint: "file, detect, none", sub: iconEntries(p) },
    { icon: "◐", label: "Color…", hint: "accent", sub: colorEntries(p) },
    { icon: "▨", label: "Background…", hint: "image behind terminals", sub: backgroundEntries(p) },
  ]);
}

export const ctxMenu = createMenu(() => refocusTerminal());

function sessionLabel(s: SessionInfo) {
  return `${agentName(s)} · ${taskTitle(s)}`;
}

export function rowMenu(s: SessionInfo, x: number, y: number) {
  const n = S.selection.length;
  const picked = n > 1 && S.selection.includes(s.id);
  const plan = picked ? selectionPlan() : null;
  ctxMenu.open(x, y, sessionLabel(s), [
    ...(plan
      ? [{
          icon: "⊞",
          label: plan.label === "New group" ? `Group ${n} selected` : plan.label,
          hint: plan.size > MAX_PANES ? FULL_HINT : "",
          disabled: plan.size > MAX_PANES,
          run: splitSelection,
        }]
      : []),
    ...(groupOf(s.id)
      ? [{ icon: "⊟", label: "Remove from group", hint: "keeps running", run: () => removeFromGroup(s.id) }]
      : []),
    { icon: "✎", label: "Rename", hint: keyLabel("rename"), run: () => startSessionRename(s.id) },
    themeEntry(s),
    picked ? endSelectedEntry(S.selection.filter((id) => sessions.has(id))) : endEntry(s),
  ]);
}

function themeEntry(s: SessionInfo): MenuEntry {
  const name = S.themes.find((t) => t.id === themeIdFor(s))?.name ?? "";
  return { icon: "◐", label: "Terminal theme…", hint: name, run: () => void sessionThemeMenu(s) };
}

/** Right-click, End session: stops the process and drops it from the list. */
export function endEntry(s: SessionInfo): MenuEntry {
  if (s.state === "done") {
    return { icon: "×", label: "Remove from list", hint: "already exited", danger: true, run: () => void endSession(s.id) };
  }
  return {
    icon: "■",
    label: "End session…",
    hint: "stops the process",
    danger: true,
    // A running agent loses its work: ask once more.
    run: async () => {
      const ok = await confirmAction({
        title: `End ${taskTitle(s)}?`,
        body: `Stops ${agentName(s)} and removes the session. Its unsaved work in the terminal is lost.`,
        action: "End session",
      });
      if (ok) void endSession(s.id);
      else refocusTerminal();
    },
  };
}

/** Right-click on a multi-selection: ends every selected session, with one confirm. */
function endSelectedEntry(ids: string[]): MenuEntry {
  return endManyEntry(ids, `End ${ids.length} sessions…`, `End ${ids.length} sessions?`);
}

/** Ends several sessions after one confirm. Sessions that already exited go without asking. */
function endManyEntry(ids: string[], label: string, title: string, hint = "stops the processes"): MenuEntry {
  const running = ids.filter((id) => sessions.get(id)?.state !== "done");
  const end = () => {
    clearSelection();
    render();
    for (const id of ids) void endSession(id);
  };
  if (!running.length) {
    return { icon: "×", label: `Remove ${ids.length} from list`, hint: "already exited", danger: true, run: end };
  }
  return {
    icon: "■",
    label,
    hint,
    danger: true,
    run: async () => {
      const ok = await confirmAction({
        title,
        body: `Stops ${running.length} running ${running.length === 1 ? "process" : "processes"} and removes the sessions. Their unsaved work in the terminal is lost.`,
        action: "End sessions",
      });
      if (ok) end();
      else refocusTerminal();
    },
  };
}

async function endSession(id: string) {
  try {
    await invoke("kill_session", { session: id });
  } catch (e) {
    showError(e);
  }
}

export function paneMenu(id: string, x: number, y: number) {
  const s = sessions.get(id);
  if (!s) return;
  if (id !== S.focused) focusPane(id);
  const full = splitFull();
  const entries: MenuEntry[] = [
    { icon: "▯", label: "Add pane right…", hint: full ? FULL_HINT : keyLabel("split-right"), disabled: full, run: () => splitMenu(id, "row", x, y) },
    { icon: "▭", label: "Add pane below…", hint: full ? FULL_HINT : keyLabel("split-down"), disabled: full, run: () => splitMenu(id, "col", x, y) },
    { icon: "✎", label: "Rename", hint: keyLabel("rename"), run: () => startSessionRename(id) },
  ];
  entries.push(themeEntry(s));
  if (shownIds().length > 1) {
    entries.push({ icon: "×", label: "Remove from group", hint: keyLabel("close-pane"), run: () => closePane(id) });
  }
  entries.push(endEntry(s));
  ctxMenu.open(x, y, sessionLabel(s), entries);
}

/** Second step of Split: a new agent in this pane's worktree, or a session not shown. */
export function splitMenu(id: string, dir: SplitDir, x: number, y: number) {
  const s = sessions.get(id);
  if (!s) return;
  const at = place(s);
  const cwd = at ? at.worktree.path : s.cwd;
  const where = at ? branchName(at.worktree) : basename(s.cwd);
  const entries: MenuEntry[] = [{ head: `NEW IN ${where.toUpperCase()}` }];
  for (const a of [...enabledAgents(), null]) {
    entries.push({
      glyph: launchIcon(a),
      label: a?.id ?? "Shell",
      hint: a?.command ?? "$SHELL",
      run: () => void newSession(cwd, a?.command ?? null, a?.id ?? "", a ? "agent" : "shell", { target: id, dir }).catch(showError),
    });
  }
  const shown = shownIds();
  const others = [...sessions.values()].filter((o) => !shown.includes(o.id)).sort(byStart);
  if (others.length) {
    entries.push({ head: "OPEN SESSION" });
    for (const o of others) {
      const oat = place(o);
      entries.push({ icon: "·", label: sessionLabel(o), hint: oat ? oat.project.name : "other", run: () => splitWith(id, dir, o.id) });
    }
  }
  ctxMenu.open(x, y, `${dir === "row" ? "Add pane right" : "Add pane below"}:`, entries);
}

export function groupMenu(g: Group, x: number, y: number) {
  ctxMenu.open(x, y, `Group: ${g.name}`, [
    { icon: "✎", label: "Rename", hint: "double-click", run: () => startRename(g) },
    { icon: "◐", label: "Terminal theme…", hint: "every pane", run: () => void groupThemeMenu(g) },
    { icon: "⊟", label: "Ungroup", hint: "sessions keep running", run: () => {
      if (g.id === S.activeGroup) unsplit(g);
      else {
        deleteGroup(g);
        render();
      }
    } },
    closeGroupEntry(g),
  ]);
}

/** Close group: ends every session in it. The daemon then drops the group. */
function closeGroupEntry(g: Group): MenuEntry {
  const ids = sessionsOf(g.layout).filter((id) => sessions.has(id));
  return endManyEntry(ids, "Close group…", `Close ${g.name}?`, `ends ${ids.length} sessions`);
}

export async function removeWorktree(p: Project, w: Worktree) {
  if (removing.has(w.path)) return;
  const ok = await confirmAction({
    title: `Remove worktree ${branchName(w)}?`,
    body: `Deletes the folder ${w.path}.\nThe branch stays. Git also deletes ignored files in it, such as .env and build output.`,
    action: "Remove worktree",
  });
  if (!ok) return refocusTerminal();
  removing.add(w.path);
  removeErrors.delete(w.path);
  render();
  try {
    await invoke("remove_worktree", { project: p.name, path: w.path });
    if (selectedWorktree.get(p.name) === w.path) selectedWorktree.delete(p.name);
    await loadProjects();
  } catch (e) {
    removeErrors.set(w.path, String(e));
  } finally {
    removing.delete(w.path);
    render();
  }
}

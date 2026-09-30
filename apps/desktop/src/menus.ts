/** Right-click menus for projects, sessions, panes and groups. */
import { launchIcon } from "./agentIcon";
import { confirmAction, promptAction } from "./confirm";
import { pickColor } from "./colorPicker";
import { keyLabel } from "./keys";
import { createMenu, type MenuEntry } from "./menu";
import { agentName, basename, branchName, byStart, locate, taskTitle } from "./model";
import { cwdOf, filledOf, slotsOf } from "./canvas";
import type { Group, Project, SessionInfo, SplitDir, Worktree } from "./types";
import { invoke } from "@tauri-apps/api/core";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { loadProjects, newSession } from "./daemon";
import { showError } from "./dom";
import { hasChanges, setShowChanges, showsChanges } from "./changes";
import { showDiff } from "./peek";
import { setShowFiles, showsFiles } from "./files";
import { launchMenu } from "./panels";
import { deleteGroup } from "./groups";
import { sessionsOf } from "./layout";
import { startRename, startSessionRename } from "./rename";
import { render } from "./render";
import { clearSelection, selectionPlan, splitSelection } from "./selection";
import { enabledAgents, FULL_HINT, groupOf, MAX_PANES, place, removeErrors, removing, S, selectedWorktree, sessions, shownIds, splitFull } from "./state";
import { groupThemeMenu, projectThemeMenu, sessionThemeMenu, themeIdFor } from "./themes";
import { closePane, focusPane, refocusTerminal, removeFromGroup, showGroup, splitWith, unfocus, unsplit } from "./view";

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
    title: `Color for ${p.name}`,
    start: p.color ?? DEFAULT_COLOR,
    action: "Set color",
    submit: (hex) => invoke("set_project_color", { project: p.name, color: hex }),
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
    { icon: "indicators-minus", label: "None", hint: "plain background", disabled: !p.background, run: () => void setBackground(p, null) },
  ];
}

/** One place for the rail icon: pick a file, detect one, or use letters. */
function iconEntries(p: Project): MenuEntry[] {
  return [
    {
      icon: "documents-folder-open",
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
    { icon: "schedule-refresh-cw", label: "Detect", hint: "favicon, logo", run: () => void setIcon(p, null) },
    { icon: "indicators-minus", label: "Letters only", hint: "no icon", run: () => void setIcon(p, "") },
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
    { icon: "code-git-branch", label: "New worktree…", disabled: !!p.error, run: () => newWorktree(p) },
    { icon: "indicators-square-arrow-out-up-right", label: "Show in file manager", run: () => void openPath(p.path).catch(showError) },
    { icon: "code-copy", label: "Copy path", run: () => void navigator.clipboard.writeText(p.path).catch(showError) },
    { icon: "documents-file-image", label: "Icon…", hint: "file, detect, none", sub: iconEntries(p) },
    { icon: "tools-sparkles", label: "Color…", hint: "accent", run: () => pickProjectColor(p) },
    { icon: "tools-sparkles", label: "Theme…", hint: "app and terminals", run: () => void projectThemeMenu(p) },
    { icon: "playback-image", label: "Background…", hint: "image behind terminals", sub: backgroundEntries(p) },
    showsChanges(p)
      ? { icon: "code-git-branch", label: "Hide changes", run: () => void setShowChanges(p, false) }
      : { icon: "code-git-branch", label: "Show changes", hint: "changed files and +/- counts", run: () => void setShowChanges(p, true) },
    showsFiles(p)
      ? { icon: "documents-folder-open", label: "Hide files", run: () => void setShowFiles(p, false) }
      : { icon: "documents-folder-open", label: "Show files", hint: "tree under each worktree", run: () => void setShowFiles(p, true) },
  ]);
}

export const ctxMenu = createMenu(() => refocusTerminal());

/** A right-click on a worktree's branch row. */
export function worktreeMenu(p: Project, w: Worktree, x: number, y: number) {
  const entries: MenuEntry[] = [
    ...(showsChanges(p) && hasChanges(w) ? [{ icon: "code-git-branch", label: "Review all changes", run: () => showDiff(w.path, branchName(w)) }] : []),
    { icon: "indicators-square-arrow-out-up-right", label: "Show in file manager", run: () => void openPath(w.path).catch(showError) },
    { icon: "code-copy", label: "Copy path", run: () => void navigator.clipboard.writeText(w.path).catch(showError) },
  ];
  if (!w.is_main) {
    entries.push({ icon: "tools-trash-2", label: "Remove worktree…", hint: "keeps the branch", danger: true, disabled: removing.has(w.path), run: () => void removeWorktree(p, w) });
  }
  ctxMenu.open(x, y, branchName(w), entries);
}

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
          icon: "code-group",
          label: plan.label === "New group" ? `Group ${n} selected` : plan.label,
          hint: plan.size > MAX_PANES ? FULL_HINT : "",
          disabled: plan.size > MAX_PANES,
          run: splitSelection,
        }]
      : []),
    ...(groupOf(s.id)
      ? [{ icon: "code-ungroup", label: "Remove from group", hint: "keeps running", run: () => removeFromGroup(s.id) }]
      : []),
    ...newSessionEntry(s, x, y),
    { icon: "tools-pencil", label: "Rename", hint: keyLabel("rename"), run: () => startSessionRename(s.id) },
    themeEntry(s),
    picked ? endSelectedEntry(S.selection.filter((id) => sessions.has(id))) : endEntry(s),
  ]);
}

const pick = (m: MenuEntry) => void ("run" in m && m.run?.());

/**
 * Delete or Backspace on a list row. With one way to remove it, that runs
 * (End or removal asks first). With more, a menu at the row asks which.
 */
export function deleteKeyMenu(el: HTMLElement) {
  // A key opened it: show focus rings, so the first item reads as the one picked.
  document.body.classList.add("kbd");
  const r = el.getBoundingClientRect();
  const [x, y] = [r.left + 24, r.bottom];
  const g = S.groups.find((x) => x.id === el.dataset.group);
  if (g && !el.dataset.session) return ctxMenu.open(x, y, `Group: ${g.name}`, groupCloseEntries(g));
  const s = sessions.get(el.dataset.session ?? "");
  if (!s) return;
  const ids = S.selection.filter((id) => sessions.has(id));
  if (ids.length > 1 && ids.includes(s.id)) return pick(endSelectedEntry(ids));
  if (!groupOf(s.id)) return pick(endEntry(s));
  ctxMenu.open(x, y, sessionLabel(s), [
    { icon: "code-ungroup", label: "Remove from group", hint: "keeps running", run: () => removeFromGroup(s.id) },
    endEntry(s),
  ]);
}

/** Right-click, New session…: the + menu for this session's worktree, at the pointer. */
function newSessionEntry(s: SessionInfo, x: number, y: number): MenuEntry[] {
  const at = place(s);
  return at ? [{ icon: "indicators-plus", label: "New session…", hint: "agent or split", run: () => launchMenu.open({ x, y }, at.project, at.worktree) }] : [];
}

function themeEntry(s: SessionInfo): MenuEntry {
  const name = S.themes.find((t) => t.id === themeIdFor(s))?.name ?? "";
  return { icon: "tools-sparkles", label: "Terminal theme…", hint: name, run: () => void sessionThemeMenu(s) };
}

/** Right-click, End session: stops the process and drops it from the list. */
export function endEntry(s: SessionInfo): MenuEntry {
  if (s.state === "done") {
    return {
      icon: "tools-trash-2",
      label: "Remove from list…",
      hint: "already exited",
      danger: true,
      run: async () => {
        const ok = await confirmAction({
          title: `Remove ${taskTitle(s)}?`,
          body: "This session has already exited. Removing it clears it from the session list.",
          action: "Remove session",
        });
        if (ok) void endSession(s.id);
        else refocusTerminal();
      },
    };
  }
  return {
    icon: "indicators-square-stop",
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
    return { icon: "tools-trash-2", label: `Remove ${ids.length} from list`, hint: "already exited", danger: true, run: end };
  }
  return {
    icon: "indicators-square-stop",
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
    { icon: "layouts-panel-left:flip", label: "Add pane right…", hint: full ? FULL_HINT : keyLabel("split-right"), disabled: full, run: () => splitMenu(id, "row", x, y) },
    { icon: "layouts-panel-bottom", label: "Add pane below…", hint: full ? FULL_HINT : keyLabel("split-down"), disabled: full, run: () => splitMenu(id, "col", x, y) },
    { icon: "tools-pencil", label: "Rename", hint: keyLabel("rename"), run: () => startSessionRename(id) },
  ];
  entries.push(themeEntry(s));
  if (shownIds().length > 1) {
    entries.push({ icon: "code-ungroup", label: "Remove from group", hint: keyLabel("close-pane"), run: () => closePane(id) });
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
    ...groupStartEntries(g),
    { head: "GROUP" },
    { icon: "tools-pencil", label: "Rename", hint: "double-click", run: () => startRename(g) },
    { icon: "tools-sparkles", label: "Terminal theme…", hint: "every pane", run: () => void groupThemeMenu(g) },
    ...groupCloseEntries(g),
  ]);
}

/** Agents, then Shell: a new session in the group's first empty pane, else a new pane on the right. */
function groupStartEntries(g: Group): MenuEntry[] {
  const cwd = cwdOf(g);
  const at = cwd ? locate(S.projects, cwd) : null;
  const where = at ? branchName(at.worktree) : cwd ? basename(cwd) : "home";
  const empty = slotsOf(g.layout)[0];
  const filled = filledOf(g.layout).filter((id) => sessions.has(id));
  const full = !empty && filled.length >= MAX_PANES;
  const entries: MenuEntry[] = [{ head: `NEW IN ${where.toUpperCase()}` }];
  for (const a of [...enabledAgents(), null]) {
    entries.push({
      glyph: launchIcon(a),
      label: a?.id ?? "Shell",
      hint: full ? FULL_HINT : a?.command ?? "$SHELL",
      disabled: full,
      run: () => {
        if (g.id !== S.activeGroup) showGroup(g.id);
        const target = g.focus && filled.includes(g.focus) ? g.focus : filled[0];
        const place = empty ? { slot: empty } : { target, dir: "row" as const };
        void newSession(cwd, a?.command ?? null, a?.id ?? "", a ? "agent" : "shell", place).catch(showError);
      },
    });
  }
  return entries;
}

/** The same group actions serve context menus, the menu bar, and the palette. */
export function groupCloseEntries(g: Group | null): Exclude<MenuEntry, { head: string }>[] {
  const current = () => S.groups.find((x) => x.id === g?.id);
  const idsOf = (group: Group) => sessionsOf(group.layout).filter((id) => sessions.has(id));
  const hasSessions = !!g && idsOf(g).length > 0;
  const drop = () => {
    // Group reloads replace objects while a menu or confirmation is open.
    const group = current();
    if (!group) return;
    if (group.id === S.activeGroup) unsplit(group);
    else {
      deleteGroup(group);
      render();
    }
  };
  const end = async (close: boolean) => {
    const group = current();
    if (!group) return;
    const ids = idsOf(group);
    const running = ids.filter((id) => sessions.get(id)?.state !== "done");
    if (running.length) {
      const ok = await confirmAction({
        title: close ? `Close ${group.name}?` : `Kill all sessions in ${group.name}?`,
        body: `Stops ${running.length} running ${running.length === 1 ? "process" : "processes"} and removes the sessions. Their unsaved work in the terminal is lost.\n${close ? "The group closes." : "The group stays open."}`,
        action: close ? "Close group" : "Kill all sessions",
      });
      if (!ok) return refocusTerminal();
    }
    clearSelection();
    const results = await Promise.allSettled(ids.filter((id) => sessions.has(id)).map((id) => invoke("kill_session", { session: id })));
    let failed = false;
    for (const result of results) {
      if (result.status === "rejected") {
        failed = true;
        showError(result.reason);
      }
    }
    // Keep the group if a kill failed or a new session appeared during confirmation.
    const latest = current();
    if (close && !failed && latest && !idsOf(latest).some((id) => !ids.includes(id))) {
      const active = latest.id === S.activeGroup;
      deleteGroup(latest);
      if (active) unfocus();
    }
    render();
  };
  const entries: Exclude<MenuEntry, { head: string }>[] = [];
  if (hasSessions) entries.push(
    { icon: "code-ungroup", label: "Ungroup", hint: "sessions keep running", run: drop },
    { icon: "indicators-square-stop", label: "Kill all sessions…", hint: "group stays open", danger: true, run: () => void end(false) },
  );
  entries.push(
    { icon: "tools-trash-2", label: hasSessions ? "Close group…" : "Close group", hint: hasSessions ? "ends sessions and removes group" : "no sessions", danger: true, disabled: !g, run: () => void end(true) },
  );
  return entries;
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

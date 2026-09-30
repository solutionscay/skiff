/** One command list for the menu bar and the palette; key actions. */
import { type Action, DESCRIBE, keyLabel, menuAccel } from "./keys";
import { agentName, branchName, bySessionPriority, taskTitle } from "./model";
import { createSwitcher, type SwitchItem } from "./switcher";
import type { DaemonStatus } from "./types";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { $, h, showError } from "./dom";
import { currentRow, cycleRegion, fromProject, stepList, stepRail, toProject } from "./keyboard";
import { copyReport, hasReport, startTrace, stopTrace, tracing } from "./latency";
import { ctxMenu, endEntry, groupCloseEntries, newWorktree, projectMenu, projectRun, splitMenu } from "./menus";
import { addProject, launchMenu, settings } from "./panels";
import { closedProjectsMenu, closeProject, openClosedProject, removeProject } from "./projectClose";
import { renameListItem, startRename, startSessionRename } from "./rename";
import { scheduleRender } from "./render";
import { accent, activeGroupObj, currentProject, currentWorktree, FONT_DEFAULT, place, S, sessions, shownIds, splitFull, worktreeSessions } from "./state";
import { copySelection, openFind, paneCenter, pasteClipboard, setFontSize } from "./terminal";
import { sessionThemeMenu } from "./themes";
import { modeGate, modeKeyBlocked, toggleFocusMode, toggleMaximize } from "./modes";
import { closePane, focusNextWaiting, goBack, moveFocus, refocusTerminal, revealSession, selectWorktree } from "./view";

interface Cmd {
  section: "File" | "Edit" | "View" | "Help";
  label: string;
  key: string;
  /** The keymap action, so the menu bar can show and bind its key. */
  action?: Action;
  run: () => void;
  /** Shown greyed out in the menu, left out of the palette. */
  off?: boolean;
}

function commands(): Cmd[] {
  const s = S.focused ? sessions.get(S.focused) : undefined;
  const shown = !!(S.focused && shownIds().includes(S.focused));
  const g = activeGroupObj();
  const k = (a: Action) => keyLabel(a);
  const act = (a: Action) => () => runAction(a);
  return [
    { section: "File", label: "New session…", key: k("new-session"), action: "new-session", run: act("new-session"), off: !currentWorktree() },
    { section: "File", label: "New worktree…", key: k("new-worktree"), action: "new-worktree", run: act("new-worktree"), off: !currentProject() },
    { section: "File", label: "Add project…", key: k("add-project"), action: "add-project", run: act("add-project") },
    { section: "File", label: "Open closed project…", key: "", run: openClosedMenu, off: !S.closedProjects.length },
    { section: "File", label: "Project menu…", key: "", run: openProjectMenu, off: !currentProject() },
    { section: "File", label: "Close project", key: "", run: () => withProject(closeProject), off: !currentProject() },
    { section: "File", label: "Remove project…", key: "", run: () => withProject(removeProject), off: !currentProject() },
    { section: "File", label: "Settings", key: k("settings"), action: "settings", run: act("settings") },
    { section: "File", label: "Quit", key: k("quit"), action: "quit", run: act("quit") },
    { section: "Edit", label: "Copy", key: k("copy"), action: "copy", run: act("copy"), off: !s },
    { section: "Edit", label: "Paste", key: k("paste"), action: "paste", run: act("paste"), off: !s },
    { section: "Edit", label: "Find…", key: k("find"), action: "find", run: act("find"), off: !shown },
    { section: "Edit", label: "Rename session", key: k("rename"), action: "rename", run: act("rename"), off: !s },
    { section: "Edit", label: "Terminal theme…", key: "", run: () => s && void sessionThemeMenu(s), off: !s },
    {
      section: "Edit", label: "End session…", key: "", off: !s,
      run: () => {
        if (!s) return;
        const e = endEntry(s);
        if ("run" in e) e.run?.();
      },
    },
    { section: "View", label: "Command palette", key: k("palette"), action: "palette", run: act("palette") },
    { section: "View", label: "Next session", key: k("session-next"), action: "session-next", run: act("session-next") },
    { section: "View", label: "Previous session", key: k("session-prev"), action: "session-prev", run: act("session-prev") },
    { section: "View", label: "Add pane right…", key: k("split-right"), action: "split-right", run: act("split-right"), off: !shown || splitFull() },
    { section: "View", label: "Add pane below…", key: k("split-down"), action: "split-down", run: act("split-down"), off: !shown || splitFull() },
    { section: "View", label: shownIds().length > 1 ? "Remove from group" : "Close", key: k("close-pane"), action: "close-pane", run: act("close-pane"), off: !shown },
    ...groupCloseEntries(g).map((entry): Cmd => ({
      section: "View", label: entry.label, key: "", run: () => entry.run?.(), off: entry.disabled,
    })),
    { section: "View", label: "Next waiting", key: k("next-waiting"), action: "next-waiting", run: act("next-waiting") },
    { section: "View", label: "Back to last session", key: k("back"), action: "back", run: act("back"), off: !S.previous },
    { section: "View", label: S.focusMode ? "Leave focus mode" : "Focus mode", key: k("focus-mode"), action: "focus-mode", run: act("focus-mode"), off: (!S.focusMode && !shown) || modeKeyBlocked("focus-mode") },
    { section: "View", label: S.maximized ? "Restore pane" : "Maximize pane", key: k("maximize"), action: "maximize", run: act("maximize"), off: (!S.maximized && !shown) || modeKeyBlocked("maximize") },
    { section: "View", label: "Bigger text", key: k("font-bigger"), action: "font-bigger", run: act("font-bigger") },
    { section: "View", label: "Smaller text", key: k("font-smaller"), action: "font-smaller", run: act("font-smaller") },
    { section: "View", label: "Reset text size", key: k("font-reset"), action: "font-reset", run: act("font-reset") },
    { section: "Help", label: "Keyboard shortcuts", key: k("shortcuts"), action: "shortcuts", run: act("shortcuts") },
    { section: "Help", label: "Open projects.toml", key: "", run: () => void openConfig() },
    {
      section: "Help", label: tracing() ? "Stop latency trace" : "Start latency trace", key: "",
      run: () => {
        tracing() ? stopTrace() : startTrace();
        scheduleRender();
      },
    },
    { section: "Help", label: "Copy latency report", key: "", run: () => void copyReport(), off: !hasReport() },
    { section: "Help", label: "Report an issue", key: "", run: () => void openUrl(ISSUES_URL).catch(showError) },
    { section: "Help", label: "About Skiff", key: "", run: () => void showAbout() },
  ];
}

const ISSUES_URL = "https://github.com/solutionscay/skiff/issues/new";

/** Where a group of commands starts a new block in its menu. */
const BREAK_BEFORE = new Set(["Project menu…", "Settings", "Copy", "Rename session", "Command palette", "Add pane right…", "Next waiting", "Focus mode", "Leave focus mode", "Bigger text", "Open projects.toml", "About Skiff"]);

let menuRuns = new Map<string, () => void>();
let menuSent = "";

/** Sends the command list to the native menu bar when it changes. */
export function syncMenu() {
  const menus: { title: string; items: { id?: string; label?: string; accel?: string; enabled?: boolean }[] }[] = [];
  const runs = new Map<string, () => void>();
  for (const [i, c] of commands().entries()) {
    let m = menus.at(-1);
    if (m?.title !== c.section) menus.push((m = { title: c.section, items: [] }));
    else if (BREAK_BEFORE.has(c.label)) m.items.push({});
    const id = `cmd:${i}`;
    runs.set(id, c.run);
    m.items.push({ id, label: c.label, accel: c.action ? menuAccel(c.action) : "", enabled: !c.off });
  }
  menuRuns = runs;
  const json = JSON.stringify(menus);
  if (json === menuSent) return;
  menuSent = json;
  invoke("set_menu", { menus }).catch(console.error);
}

void listen<string>("skiff:menu", (e) => menuRuns.get(e.payload)?.());

async function openConfig() {
  await invoke("open_config").catch(showError);
}

/** A plain modal for the shortcut sheet and About. Esc or a click outside closes it. */
function infoDialog(title: string, body: HTMLElement, initialFocus?: HTMLElement) {
  const back = document.activeElement as HTMLElement | null;
  const overlay = h("div", "confirm-overlay");
  const panel = h("div", "info-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-modal", "true");
  panel.setAttribute("aria-label", title);
  const head = h("div", "tc-head");
  // Esc in the corner, as in Settings: a click closes too.
  const x = h("button", "tc-x");
  x.type = "button";
  x.title = "Close (Esc)";
  x.setAttribute("aria-label", "Close");
  x.appendChild(h("kbd", "", "Esc"));
  x.addEventListener("click", () => close());
  head.append(h("div", "tc-title", title), x);
  const scroll = h("div", "info-body");
  scroll.appendChild(body);
  panel.append(head, scroll);
  overlay.appendChild(panel);
  document.body.appendChild(overlay);
  const close = () => {
    overlay.remove();
    back?.focus?.();
  };
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "F1") {
      e.preventDefault();
      close();
    }
  });
  if (initialFocus) initialFocus.focus();
  else {
    panel.tabIndex = -1;
    panel.focus();
  }
}

type Shortcut = [section: string, what: string, key: string];

/** Keyboard controls that do not come from the configurable action keymap. */
const FIXED_SHORTCUTS: Shortcut[] = [
  ["Projects", "Select project 1 through 9", `${navigator.userAgent.includes("Macintosh") ? "⌘" : "Ctrl"}+Shift+1…9`],
  ["Rail", "Open the focused project", "Enter / Space"],
  ["Rail", "Close or remove the focused project", "Delete / Backspace"],
  ["Session list", "Open or close all worktrees from the project row", "Enter / Space / ← / →"],
  ["Session list", "Close or remove the project from the project row", "Delete / Backspace"],
  ["Session list", "Open or close the focused worktree", "Enter / Space"],
  ["Session list", "Open the focused session or group", "Enter"],
  ["Session list", "Collapse or expand the focused worktree, Changes, Files or folder", "← / →"],
  ["Session list", "Open or close the focused Changes, Files or folder; open the focused file or diff", "Enter"],
  ["Session list", "Extend the session selection", "Shift+↑ / ↓"],
  ["Session list", "Make a group from the selected sessions", "Enter"],
  ["Session list", "Clear the session selection", "Esc"],
  ["Session list", "Rename the focused session or group", "F2"],
  ["Session list", "End a session or remove a group", "Delete / Backspace"],
  ["Session list", "Open the row menu", "Menu / Shift+F10"],
  ["Command palette", "Move through matches", "↑ / ↓"],
  ["Command palette", "Run the selected match", "Enter"],
  ["Command palette", "Close the palette", "Esc"],
  ["Find in terminal", "Find the next or previous match", "Enter / Shift+Enter"],
  ["Find in terminal", "Close the find bar", "Esc"],
  ["Start-session menu", "Move through choices", "↑ / ↓"],
  ["Start-session menu", "Start the numbered choice", "1…9"],
  ["Start-session menu", "Close the menu", "Esc"],
  ["Empty pane", "Move through session choices", "↑ / ↓"],
  ["Empty pane", "Start the numbered choice", "1…9"],
  ["Empty pane", "Move to the next empty pane", "Tab / Shift+Tab"],
  ["Empty pane", "Remove the pane", "Delete"],
  ["Menus", "Move through menu items", "↑ / ↓"],
  ["Menus", "Open a submenu", "→ / Enter / Space"],
  ["Menus", "Close a menu or submenu", "Esc / ←"],
  ["Dialogs", "Close the dialog", "Esc"],
  ["Dialogs", "Run the selected dialog action", "Enter"],
];

/** F1: all app controls. Action keys come from the live keymap. */
function showShortcuts() {
  const body = h("div", "keys-dialog");
  const search = h("input", "keys-search") as HTMLInputElement;
  search.type = "search";
  search.autocomplete = "off";
  search.spellcheck = false;
  search.placeholder = "Find a shortcut";
  search.setAttribute("aria-label", "Find a shortcut");
  const table = h("div", "keys-table");
  const shortcuts: Shortcut[] = [
    ...DESCRIBE.map(([section, action, what]): Shortcut => [section, what, keyLabel(action)]),
    ...FIXED_SHORTCUTS,
  ];
  const render = () => {
    const query = search.value.trim().toLocaleLowerCase();
    const shown = shortcuts.filter(([section, what, key]) =>
      `${section} ${what} ${key}`.toLocaleLowerCase().includes(query),
    );
    table.replaceChildren();
    let section = "";
    for (const [sec, what, key] of shown) {
      if (sec !== section) {
        table.appendChild(h("div", "keys-head", sec.toUpperCase()));
        section = sec;
      }
      const row = h("div", "keys-row");
      row.append(h("span", "keys-what", what), h("kbd", "", key));
      table.appendChild(row);
    }
    if (!shown.length) table.appendChild(h("div", "keys-empty", "No shortcuts match."));
    if (!query) table.appendChild(h("div", "keys-note", "Change an action key in projects.toml. For example: [keys] palette = \"ctrl+shift+k\"."));
  };
  search.addEventListener("input", render);
  render();
  body.append(search, table);
  infoDialog("Keyboard shortcuts", body, search);
}

async function showAbout() {
  const [app, status, config] = await Promise.all([
    getVersion().catch(() => "?"),
    invoke<DaemonStatus>("daemon_status").catch(() => null),
    invoke<string>("config_path").catch(() => "?"),
  ]);
  const table = h("div", "keys-table");
  const row = (what: string, value: string) => {
    const r = h("div", "keys-row");
    r.append(h("span", "keys-what", what), h("span", "about-value mono", value));
    table.appendChild(r);
  };
  row("App", app);
  row("Daemon", status?.version ?? "not reachable");
  row("Socket", status?.socket ?? "?");
  row("Config", config);
  table.appendChild(h("div", "keys-note", "Skiff runs coding agents side by side. Sessions live in skiffd and keep running when the window closes."));
  infoDialog("About Skiff", table);
}

/** The palette lists every enabled command, then sessions and worktrees. */
function paletteCommands(): SwitchItem[] {
  return commands()
    .filter((c) => !c.off && c.label !== "Command palette")
    .map((c) => ({
      kind: "command" as const,
      color: "var(--text-3)",
      primary: c.label,
      secondary: c.section,
      meta: c.key,
      text: `${c.section} ${c.label}`,
      run: c.run,
    }));
}

export const switcher = createSwitcher(() => {
  const items: SwitchItem[] = paletteCommands();
  const found: SwitchItem[] = [...sessions.values()].sort(bySessionPriority).map((s) => {
    const at = place(s);
    const where = at ? `${at.project.name} / ${branchName(at.worktree)}` : `other / ${s.cwd}`;
    return {
      kind: "session",
      color: accent(at?.project),
      primary: `${agentName(s)} · ${taskTitle(s)}`,
      secondary: where,
      state: s.state,
      meta: s.state === "waiting" || s.state === "idle" ? s.state : "",
      text: `${where} ${agentName(s)} ${taskTitle(s)} ${s.state}`,
      run: () => revealSession(s.id),
    };
  });
  items.push(...found);
  for (const p of S.closedProjects) {
    items.push({
      kind: "command",
      color: accent(p),
      primary: `Open project: ${p.name}`,
      secondary: "closed",
      meta: "",
      text: `open closed project ${p.name}`,
      run: () => void openClosedProject(p),
    });
  }
  for (const p of S.projects) {
    for (const w of p.worktrees) {
      items.push({
        kind: "worktree",
        color: accent(p),
        primary: branchName(w),
        secondary: p.name,
        meta: `${worktreeSessions(w).length} sessions`,
        text: `${p.name} / ${branchName(w)} worktree`,
        run: () => selectWorktree(p, w),
      });
    }
  }
  return items;
}, refocusTerminal);

function openNewSession() {
  // Its key works from inside a menu, such as the worktree menu that shows it: that menu goes.
  ctxMenu.close(false);
  // The highlighted row's worktree, as its + would; else the focused one.
  const row = S.atRail ? undefined : currentRow();
  const rowPlus = row?.closest("#sidebar-scroll .wt")?.querySelector<HTMLElement>(".wt-plus");
  if (rowPlus) return rowPlus.click();
  const at = currentWorktree();
  if (!at) return;
  const plus = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-plus")].find((b) => b.dataset.wt === at.w.path);
  launchMenu.open(plus ?? $("rail"), at.p, at.w);
}

export function runAction(a: Action) {
  const shown = S.focused && shownIds().includes(S.focused);
  // The native menu runs its accelerators through here, not through the key handler.
  if (modeKeyBlocked(a) || modeGate(a)) return;
  switch (a) {
    case "palette": return switcher.toggle();
    case "new-session": return openNewSession();
    case "new-worktree": {
      const p = currentProject();
      return p ? newWorktree(p) : undefined;
    }
    case "add-project": return void addProject.open();
    case "project-menu": {
      // The highlighted row's menu, as a right-click opens it: a worktree, a
      // group, a session, or a Changes or Files row. The
      // highlight, not DOM focus: a picked group can be current while focus
      // rests elsewhere. On the rail, or with no row, the project menu.
      const row = S.atRail || document.activeElement?.closest("#rail") ? undefined : currentRow();
      if (!row) return openProjectMenu();
      const b = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: b.left + 24, clientY: b.bottom }));
      return;
    }
    case "project-folder":
    case "project-copy-path":
    case "project-color":
    case "project-theme":
    case "project-changes":
    case "project-files": {
      const p = currentProject();
      return p ? projectRun(p)[a]() : undefined;
    }
    case "split-right":
    case "split-down": {
      if (!S.focused || !shown || splitFull()) return;
      const [x, y] = paneCenter();
      return splitMenu(S.focused, a === "split-right" ? "row" : "col", x, y);
    }
    case "close-pane": return shown ? closePane(S.focused!) : undefined;
    case "next-waiting": return focusNextWaiting();
    case "back": return goBack();
    case "rename": {
      // In the list, the row with the keys: arrowing to a row does not open it at once.
      if (renameListItem(document.activeElement as HTMLElement | null)) return;
      const pg = S.groups.find((x) => x.id === S.groupPicked);
      return pg ? startRename(pg) : S.focused ? startSessionRename(S.focused) : undefined;
    }
    case "copy": return void copySelection();
    case "paste": return void pasteClipboard();
    case "find": return openFind();
    case "font-bigger": return setFontSize(S.fontSize + 1);
    case "font-smaller": return setFontSize(S.fontSize - 1);
    case "font-reset": return setFontSize(FONT_DEFAULT);
    case "settings": return void settings.open();
    case "quit": return void invoke("quit_app").catch(showError);
    case "focus-left": return moveFocus("left");
    case "focus-right": return moveFocus("right");
    case "focus-up": return moveFocus("up");
    case "focus-down": return moveFocus("down");
    case "region-next": return cycleRegion(1);
    case "region-prev": return cycleRegion(-1);
    case "session-next": return S.atRail ? stepRail(1) : stepList(1);
    case "session-prev": return S.atRail ? stepRail(-1) : stepList(-1);
    case "list-project": return toProject();
    case "list-back": return fromProject();
    case "shortcuts": return showShortcuts();
    case "focus-mode": return toggleFocusMode();
    case "maximize": return toggleMaximize();
  }
}

const withProject = (f: (p: NonNullable<ReturnType<typeof currentProject>>) => Promise<void>) => {
  const p = currentProject();
  if (p) void f(p);
};

/** The closed projects, at the rail's + button. */
function openClosedMenu() {
  const r = document.querySelector<HTMLElement>("#rail .rail-add")?.getBoundingClientRect();
  closedProjectsMenu(r ? r.right : 80, r ? r.top : 80);
}

/** The selected project's menu, at its rail icon. */
function openProjectMenu() {
  const p = currentProject();
  const chip = document.querySelector<HTMLElement>("#rail .rail-chip.active");
  if (!p || !chip) return;
  const r = chip.getBoundingClientRect();
  projectMenu(p, r.right, r.top);
}

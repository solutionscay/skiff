/** One command list for the menu bar and the palette; key actions. */
import { type Action, DESCRIBE, keyLabel, menuAccel } from "./keys";
import { agentName, branchName, bySessionPriority, taskTitle } from "./model";
import { createSwitcher, type SwitchItem } from "./switcher";
import type { DaemonStatus } from "./types";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { $, h, showError } from "./dom";
import { cycleRegion, toggleList } from "./keyboard";
import { endEntry, newWorktree, splitMenu } from "./menus";
import { addProject, launchMenu, settings } from "./panels";
import { startRename, startSessionRename } from "./rename";
import { accent, activeGroupObj, currentProject, currentWorktree, FONT_DEFAULT, place, S, sessions, shownIds, splitFull, worktreeSessions } from "./state";
import { copySelection, openFind, paneCenter, pasteClipboard, setFontSize } from "./terminal";
import { sessionThemeMenu } from "./themes";
import { closePane, focusNextWaiting, goBack, moveFocus, refocusTerminal, revealSession, selectWorktree, stepSession, unsplit } from "./view";

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
    { section: "View", label: "Session list / terminal", key: k("focus-list"), action: "focus-list", run: act("focus-list") },
    { section: "View", label: "Add pane right…", key: k("split-right"), action: "split-right", run: act("split-right"), off: !shown || splitFull() },
    { section: "View", label: "Add pane below…", key: k("split-down"), action: "split-down", run: act("split-down"), off: !shown || splitFull() },
    { section: "View", label: shownIds().length > 1 ? "Remove from group" : "Close", key: k("close-pane"), action: "close-pane", run: act("close-pane"), off: !shown },
    { section: "View", label: "Ungroup", key: "", run: () => g && unsplit(g), off: !g },
    { section: "View", label: "Next waiting", key: k("next-waiting"), action: "next-waiting", run: act("next-waiting") },
    { section: "View", label: "Back to last session", key: k("back"), action: "back", run: act("back"), off: !S.previous },
    { section: "View", label: "Bigger text", key: k("font-bigger"), action: "font-bigger", run: act("font-bigger") },
    { section: "View", label: "Smaller text", key: k("font-smaller"), action: "font-smaller", run: act("font-smaller") },
    { section: "View", label: "Reset text size", key: k("font-reset"), action: "font-reset", run: act("font-reset") },
    { section: "Help", label: "Keyboard shortcuts", key: k("shortcuts"), action: "shortcuts", run: act("shortcuts") },
    { section: "Help", label: "Open projects.toml", key: "", run: () => void openConfig() },
    { section: "Help", label: "Report an issue", key: "", run: () => void openUrl(ISSUES_URL).catch(showError) },
    { section: "Help", label: "About Skiff", key: "", run: () => void showAbout() },
  ];
}

const ISSUES_URL = "https://github.com/solutionscay/skiff/issues/new";

/** Where a group of commands starts a new block in its menu. */
const BREAK_BEFORE = new Set(["Settings", "Copy", "Rename session", "Command palette", "Add pane right…", "Next waiting", "Bigger text", "Open projects.toml", "About Skiff"]);

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
  const path = await invoke<string>("config_path");
  await openPath(path).catch(showError);
}

/** A plain modal for the shortcut sheet and About. Esc or a click outside closes it. */
function infoDialog(title: string, body: HTMLElement) {
  const back = document.activeElement as HTMLElement | null;
  const overlay = h("div", "confirm-overlay");
  const panel = h("div", "info-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-modal", "true");
  panel.setAttribute("aria-label", title);
  const head = h("div", "tc-head");
  head.append(h("div", "tc-title", title), h("kbd", "", "Esc"));
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
  panel.tabIndex = -1;
  panel.focus();
}

/** F1: every key from the live keymap, so [keys] overrides show. */
function showShortcuts() {
  const table = h("div", "keys-table");
  let section = "";
  for (const [sec, action, what] of DESCRIBE) {
    if (sec !== section) {
      table.appendChild(h("div", "keys-head", sec.toUpperCase()));
      section = sec;
    }
    const row = h("div", "keys-row");
    row.append(h("span", "keys-what", what), h("kbd", "", keyLabel(action)));
    table.appendChild(row);
  }
  const extra = h("div", "keys-row");
  extra.append(h("span", "keys-what", "Select project 1–9"), h("kbd", "", "Ctrl+1…9"));
  table.appendChild(extra);
  const note = h("div", "keys-note", "Change any key in projects.toml, for example [keys] palette = \"ctrl+shift+k\".");
  table.appendChild(note);
  infoDialog("Keyboard shortcuts", table);
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
  const at = currentWorktree();
  if (!at) return;
  const plus = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-plus")].find((b) => b.dataset.wt === at.w.path);
  launchMenu.open(plus ?? $("rail"), at.p, at.w);
}

export function runAction(a: Action) {
  const shown = S.focused && shownIds().includes(S.focused);
  switch (a) {
    case "palette": return switcher.toggle();
    case "new-session": return openNewSession();
    case "new-worktree": {
      const p = currentProject();
      return p ? newWorktree(p) : undefined;
    }
    case "add-project": return void addProject.open();
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
    case "focus-list": return toggleList();
    case "session-next": return stepSession(1);
    case "session-prev": return stepSession(-1);
    case "shortcuts": return showShortcuts();
  }
}

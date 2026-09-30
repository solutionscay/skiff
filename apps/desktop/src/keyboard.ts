/** App keys, Tab order, F6 regions, arrows in the lists. */
import { type Action, actionFor } from "./keys";
import { runAction } from "./commands";
import { $, host } from "./dom";
import { ctxMenu, deleteKeyMenu } from "./menus";
import { render } from "./render";
import { renameListItem } from "./rename";
import { clearSelection, extendSelection, keepRow, splitSelection } from "./selection";
import { collapsed, panes, S } from "./state";
import { revealSession, selectProject, showGroup } from "./view";

/** Ctrl+Shift+1..9 selects a project (Command+Shift+1..9 on macOS). Not in the keymap: it is a range, not one key. */
function projectKey(e: KeyboardEvent): number | null {
  const primary = navigator.userAgent.includes("Macintosh") ? e.metaKey : e.ctrlKey;
  if (!primary || !e.shiftKey || e.altKey || (navigator.userAgent.includes("Macintosh") ? e.ctrlKey : e.metaKey)) return null;
  const m = /^Digit([1-9])$/.exec(e.code);
  return m ? Number(m[1]) : null;
}

/** Keys the app owns even while a terminal has focus. Everything else goes to the PTY. */
export function appKey(e: KeyboardEvent): Action | number | null {
  return actionFor(e) ?? projectKey(e);
}

window.addEventListener("keydown", (e) => {
  const key = appKey(e);
  if (key === null) return;
  e.preventDefault();
  e.stopPropagation();
  if (typeof key === "number") {
    const p = S.projects[key - 1];
    if (p) selectProject(p.name);
  } else {
    runAction(key);
  }
}, true);

// Focus rings show only while the keyboard is in use (see body.kbd in styles.css).
const NAV_KEYS = new Set(["Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "F6", "Home", "End"]);

window.addEventListener("keydown", (e) => {
  if (NAV_KEYS.has(e.key)) document.body.classList.add("kbd");
}, true);
window.addEventListener("mousedown", () => document.body.classList.remove("kbd"), true);

export const itemKey = (el: HTMLElement) => el.dataset.session ?? el.dataset.wt ?? el.dataset.group ?? el.dataset.key ?? "";

export function listItems(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-pick, #sidebar-scroll .group-pick, #sidebar-scroll button.session-row, #sidebar-scroll .files-head, #sidebar-scroll .file-row")];
}

/** Where the roving tab stop lands: the row for the pane on screen ("where I am"),
 *  else the last-roved row, else the first row. */
function roveTarget(): HTMLElement | undefined {
  const items = listItems();
  return (
    items.find((x) => x.classList.contains("focused")) ??
    items.find((x) => S.roveKey && itemKey(x) === S.roveKey) ??
    items[0]
  );
}

/**
 * One Tab stop per region: the rail, the new-worktree button, the session
 * list, the terminal. Everything else is reached by mouse, key, or palette.
 */
export function applyTabOrder() {
  for (const el of document.querySelectorAll<HTMLElement>("#app button, #app [tabindex]")) {
    // Settings is its own screen with its own Tab order; xterm keeps its textarea.
    if (el.closest("#settings, .xterm")) continue;
    el.tabIndex = -1;
  }
  const rail = document.querySelector<HTMLElement>("#rail .rail-chip.active") ?? document.querySelector<HTMLElement>("#rail .rail-chip");
  if (rail) rail.tabIndex = 0;
  const addWt = document.querySelector<HTMLElement>("#sidebar-scroll .project-row .wt-plus");
  if (addWt) addWt.tabIndex = 0;
  const item = roveTarget();
  if (item) item.tabIndex = 0;
}

function regionOf(el: Element | null): number {
  if (!el) return -1;
  if (el.closest("#rail")) return 0;
  if (el.closest(".project-row")) return 1;
  if (el.closest("#sidebar-scroll")) return 2;
  if (host.contains(el)) return 3;
  return -1;
}

/** F6 and Shift+F6: rail, new worktree, session list, terminal. */
export function cycleRegion(dir: 1 | -1) {
  document.body.classList.add("kbd");
  const stops: (() => boolean)[] = [
    () => focusEl(document.querySelector<HTMLElement>("#rail .rail-chip.active") ?? document.querySelector<HTMLElement>("#rail .rail-chip")),
    () => focusEl(document.querySelector<HTMLElement>("#sidebar-scroll .project-row .wt-plus")),
    () => focusEl(roveTarget() ?? null),
    () => {
      const t = S.focused ? panes.get(S.focused)?.term : undefined;
      if (!t) return false;
      t.focus();
      return true;
    },
  ];
  let i = regionOf(document.activeElement);
  for (let n = 0; n < stops.length; n++) {
    i = (i + dir + stops.length) % stops.length;
    if (stops[i]()) return;
  }
}

function focusEl(el: HTMLElement | null): boolean {
  if (!el) return false;
  el.focus();
  return document.activeElement === el;
}

/**
 * Up/Down browses the list without switching the terminal view. Pause on a
 * row and it opens on its own; move again inside the pause and nothing
 * happens, so scanning past several rows does not flicker the view.
 */
const REVEAL_DELAY_MS = 150;
let revealTimer: ReturnType<typeof setTimeout> | undefined;

function cancelReveal() {
  clearTimeout(revealTimer);
  revealTimer = undefined;
}

function scheduleReveal(el: HTMLElement) {
  cancelReveal();
  revealTimer = setTimeout(() => openRow(el), REVEAL_DELAY_MS);
}

/** Shows a session or picks a group. The row keeps the keys, as a click does. */
function openRow(el: HTMLElement) {
  const sid = el.dataset.session;
  if (sid && (S.focused !== sid || S.groupPicked)) {
    clearSelection();
    revealSession(sid);
    requestAnimationFrame(() => keepRow(sid));
  } else if (el.classList.contains("group-pick") && el.dataset.group && (el.dataset.group !== S.activeGroup || el.dataset.group !== S.groupPicked)) {
    showGroup(el.dataset.group, true);
  }
}

/** The row Ctrl+Shift+Up/Down last landed on. A diff or terminal takes the keys, so the row remembers the place. */
let stepKey = "";

/**
 * Ctrl+Shift+Down/Up: the next or previous session, Changes or Files header, or file row in
 * the list. A session opens. A changed file loads its diff. Headers and files take focus.
 */
export function stepList(dir: 1 | -1) {
  const rows = listItems().filter((r) => !r.dataset.wt && !r.dataset.group);
  if (!rows.length) return;
  const active = document.activeElement as HTMLElement | null;
  let i = active && rows.includes(active) ? rows.indexOf(active) : rows.findIndex((r) => itemKey(r) === stepKey);
  if (i < 0) i = rows.findIndex((r) => r.dataset.session && r.dataset.session === S.focused);
  const next = rows[i < 0 ? (dir > 0 ? 0 : rows.length - 1) : (i + dir + rows.length) % rows.length];
  stepKey = itemKey(next);
  S.roveKey = stepKey;
  if (next.dataset.session) revealSession(next.dataset.session);
  else if (next.classList.contains("change-row")) {
    // The row's own Enter handler loads the diff.
    next.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  } else {
    document.body.classList.add("kbd");
    next.focus();
  }
}

/** A Changes or Files header, or a row under one. Not a session, group or worktree. */
const isSection = (el: HTMLElement) => !!el.dataset.key && !el.dataset.session && !el.dataset.wt && !el.dataset.group;

/** Arrows inside the rail and the session list; Enter, Left/Right, Menu key. */
$("rail").addEventListener("keydown", (e) => {
  if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
  const chips = [...document.querySelectorAll<HTMLElement>("#rail .rail-chip")];
  const i = chips.indexOf(document.activeElement as HTMLElement);
  const next = chips[(i + (e.key === "ArrowDown" ? 1 : -1) + chips.length) % chips.length];
  if (next) {
    e.preventDefault();
    chips.forEach((c) => (c.tabIndex = -1));
    next.tabIndex = 0;
    next.focus();
  }
});

$("sidebar-scroll").addEventListener("keydown", (e) => {
  const el = document.activeElement as HTMLElement | null;
  if (!el || el.tagName === "INPUT") return;
  const items = listItems();
  const i = items.indexOf(el);
  if (i < 0) return;
  const go = (j: number) => {
    const t = items[(j + items.length) % items.length];
    items.forEach((x) => (x.tabIndex = -1));
    t.tabIndex = 0;
    S.roveKey = itemKey(t);
    t.focus();
    scheduleReveal(t);
  };
  const wt = el.dataset.wt;
  cancelReveal();
  if (e.shiftKey && (e.key === "ArrowDown" || e.key === "ArrowUp") && el.dataset.session) extendSelection(e.key === "ArrowDown" ? 1 : -1, el);
  else if (e.key === "Enter" && S.selection.length >= 2) splitSelection();
  else if (e.key === "Escape" && S.selection.length) {
    const key = itemKey(el);
    clearSelection();
    render();
    keepRow(key);
  } else if (isSection(el) && (e.key === "Enter" || e.key === " ")) {
    // A Changes or Files header or a folder: Enter opens or closes it.
    el.click();
  } else if (isSection(el) && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
    // Right opens, Left closes. On a file row they do nothing.
    if (el.getAttribute("aria-expanded") === String(e.key === "ArrowLeft")) el.click();
  } else if (e.key === "ArrowDown") go(i + 1);
  else if (e.key === "ArrowUp") go(i - 1);
  else if (e.key === "Home") go(0);
  else if (e.key === "End") go(items.length - 1);
  else if (e.key === "Enter" && wt && el.classList.contains("wt-pick")) {
    // Enter on a worktree starts a terminal there.
    const plus = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-plus")].find((b) => b.dataset.wt === wt);
    plus?.click();
  } else if (e.key === "Enter") {
    // Enter opens the row right away, instead of waiting out the pause.
    openRow(el);
  } else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && wt) {
    if (e.key === "ArrowLeft") collapsed.add(wt);
    else collapsed.delete(wt);
    S.roveKey = wt;
    render();
    roveTarget()?.focus();
  } else if (e.key === "F2" && !e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && renameListItem(el)) {
    // F2 renames the row in place, as in a file manager.
  } else if ((e.key === "Delete" || e.key === "Backspace") && !e.ctrlKey && !e.altKey && !e.metaKey && (el.dataset.session || el.dataset.group)) {
    deleteKeyMenu(el);
  } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: r.left + 24, clientY: r.bottom }));
  } else if (!(el.dataset.session && passToTerminal(e, el.dataset.session))) return;
  e.preventDefault();
  e.stopPropagation();
});

/**
 * A clicked session keeps the keys on its row. A key the list does not use
 * moves them to the open terminal, and the terminal gets that key too.
 */
function passToTerminal(e: KeyboardEvent, id: string): boolean {
  const term = S.focused === id ? panes.get(id)?.term : undefined;
  if (!term || e.altKey || e.metaKey) return false;
  let data = "";
  if (e.key === "Enter") data = "\r";
  else if (e.key.length === 1 && !e.ctrlKey) data = e.key;
  else if (e.ctrlKey && /^[a-z]$/i.test(e.key)) data = String.fromCharCode(e.key.toUpperCase().charCodeAt(0) - 64);
  else if (e.key !== "Escape") return false;
  term.focus();
  if (data) term.input(data);
  return true;
}

// A render can drop the element that had the keys. Focus then falls to the
// body and typed keys go nowhere, while a pane still shows as focused.
window.addEventListener("keydown", (e) => {
  const a = document.activeElement;
  if (e.defaultPrevented || (a && a !== document.body) || ctxMenu.isOpen || !S.focused) return;
  if (!passToTerminal(e, S.focused)) return;
  e.preventDefault();
  e.stopPropagation();
});

$("sidebar-scroll").addEventListener("focusin", (e) => {
  const el = e.target as HTMLElement;
  if (listItems().includes(el)) S.roveKey = itemKey(el);
});

// Leaving the list (F6, a click elsewhere) drops any pending reveal.
$("sidebar-scroll").addEventListener("focusout", () => cancelReveal());

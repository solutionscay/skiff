/** App keys, Tab order, F6 regions, arrows in the lists. */
import { type Action, actionFor } from "./keys";
import { runAction } from "./commands";
import { $, host } from "./dom";
import { render } from "./render";
import { clearSelection, extendSelection, keepRow, splitSelection } from "./selection";
import { collapsed, panes, S } from "./state";
import { refocusTerminal, selectProject } from "./view";

/** Ctrl+1..9 selects a project. Not in the keymap: it is a range, not one key. */
function projectKey(e: KeyboardEvent): number | null {
  if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return null;
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

export const itemKey = (el: HTMLElement) => el.dataset.session ?? el.dataset.wt ?? el.dataset.group ?? "";

export function listItems(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-pick, #sidebar-scroll .group-pick, #sidebar-scroll button.session-row")];
}

function roveTarget(): HTMLElement | undefined {
  const items = listItems();
  return (
    items.find((x) => S.roveKey && itemKey(x) === S.roveKey) ??
    items.find((x) => x.classList.contains("focused")) ??
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

/** Ctrl+Shift+L: into the session list, or back to the terminal. For keyboards with no F6. */
export function toggleList() {
  document.body.classList.add("kbd");
  if (regionOf(document.activeElement) === 2) refocusTerminal();
  else focusEl(roveTarget() ?? null);
}

function focusEl(el: HTMLElement | null): boolean {
  if (!el) return false;
  el.focus();
  return document.activeElement === el;
}

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
  };
  const wt = el.dataset.wt;
  if (e.shiftKey && (e.key === "ArrowDown" || e.key === "ArrowUp") && el.dataset.session) extendSelection(e.key === "ArrowDown" ? 1 : -1, el);
  else if (e.key === "Enter" && S.selection.length >= 2) splitSelection();
  else if (e.key === "Escape" && S.selection.length) {
    const key = itemKey(el);
    clearSelection();
    render();
    keepRow(key);
  } else if (e.key === "ArrowDown") go(i + 1);
  else if (e.key === "ArrowUp") go(i - 1);
  else if (e.key === "Home") go(0);
  else if (e.key === "End") go(items.length - 1);
  else if (e.key === "Enter" && wt && el.classList.contains("wt-pick")) {
    // Enter on a worktree starts a terminal there.
    const plus = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-plus")].find((b) => b.dataset.wt === wt);
    plus?.click();
  } else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && wt) {
    if (e.key === "ArrowLeft") collapsed.add(wt);
    else collapsed.delete(wt);
    S.roveKey = wt;
    render();
    roveTarget()?.focus();
  } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: r.left + 24, clientY: r.bottom }));
  } else return;
  e.preventDefault();
  e.stopPropagation();
});

$("sidebar-scroll").addEventListener("focusin", (e) => {
  const el = e.target as HTMLElement;
  if (listItems().includes(el)) S.roveKey = itemKey(el);
});

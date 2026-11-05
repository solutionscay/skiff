/**
 * Focus mode and maximize. Focus mode hides the tree, the top bar and
 * the status bar: the panes of the current view fill the window. The Ledger
 * beside Skipper stays, folded or open. Maximize shows only the focused pane,
 * with the same chrome and the Ledger hidden. Ctrl+Shift+Up/Down
 * step through the panes of the view, as they step through its rows in the
 * tree. While maximized, the next pane is maximized in its place.
 * Ctrl+Shift+Left/Right do nothing, but for the Ledger beside Skipper. Keys that go to another view (Next
 * waiting, the palette, Back, a project key) work, and going there ends the
 * mode.
 */
import { isSlot } from "../workspace/layoutSlots";
import { runAction } from "./actions";
import { currentRow } from "./keyboard";
import type { Action } from "./keys";
import { render } from "./render";
import { isSkiffAgent } from "./skiffAgent";
import { S } from "./state";
import { shownIds } from "./stateQueries";

import { fitWithLayout } from "../terminal/terminal";

import { focusPane } from "../workspace/view";

/** Keys that move in the tree, which focus mode hides. */
const TREE_KEYS = new Set<Action>(["session-next", "session-prev", "list-project", "list-back", "list-main", "region-next", "region-prev", "project-menu"]);
/** Keys that move between panes or add one: maximize shows one pane only. */
const PANE_KEYS = new Set<Action>(["focus-left", "focus-right", "focus-up", "focus-down", "split-right", "split-down"]);
/** Keys whose menus open at a tree row. The chrome comes back first. */
const CHROME_KEYS = new Set<Action>(["new-session", "new-worktree", "add-project", "project-color", "project-changes", "project-files"]);

/** The sessions shown at the last render. Focus mode ends when none of them stays. */
let seen: string[] = [];

const hidden = () => S.focusMode || !!S.maximized;

/**
 * Entering a mode needs a session to act on. Focus mode takes a session row or
 * a group row. Maximize takes a session row only: a group has no one pane. A
 * worktree, Changes, Files, or a project row is not a session. Leaving
 * a mode always works, since the tree is hidden then.
 */
export function modeKeyBlocked(a: Action): boolean {
  if (a !== "focus-mode" && a !== "maximize") return false;
  if (hidden()) return false;
  const row = currentRow();
  if (!row) return false;
  if (row.dataset.session) return false;
  return !(a === "focus-mode" && row.dataset.group);
}

/** True when the mode takes the key: it does nothing, or already ran. */
export function modeGate(a: Action): boolean {
  if (!hidden()) return false;
  if (S.maximized && PANE_KEYS.has(a)) return true;
  // Ctrl+Shift+Up/Down step through the group's panes, as they step through its rows in the tree.
  if (a === "session-next" || a === "session-prev") {
    stepPane(a === "session-next" ? 1 : -1);
    return true;
  }
  if (TREE_KEYS.has(a)) return true;
  if (CHROME_KEYS.has(a)) {
    leaveModes();
    // The menu measures the tree, so it must be laid out first.
    requestAnimationFrame(() => runAction(a));
    return true;
  }
  return false;
}

/** The next or previous pane of the view, in tree order, wrapping at the ends. Maximize moves with it. */
function stepPane(dir: 1 | -1) {
  const ids = shownIds().filter((id) => !isSlot(id));
  const i = S.focused ? ids.indexOf(S.focused) : -1;
  if (ids.length < 2 || i < 0) return;
  const next = ids[(i + dir + ids.length) % ids.length];
  if (S.maximized) S.maximized = next;
  focusPane(next);
}

/** The view has one pane: focus mode and maximize look the same, so they are one state. */
const onePane = () => shownIds().filter((id) => !isSlot(id)).length === 1;

/**
 * Skipper's pane alone. Focus mode keeps the Ledger and maximize hides it, so
 * the two look different and stay two states, as with many panes.
 */
const twoStates = () => !!S.focused && isSkiffAgent(S.focused) && shownIds().includes(S.focused);
/** One pane where focus mode and maximize are one state. */
const oneState = () => onePane() && !twoStates();

export function toggleFocusMode() {
  // One pane: focus mode is maximize. A second press leaves it, never a step back to the other.
  if (oneState() && !maxReturn.to) return hidden() ? leaveModes() : toggleMaximize();
  // From maximize, focus mode shows the view's panes again with the chrome still hidden.
  if (S.maximized) {
    S.focusMode = true;
    return restore();
  }
  if (hidden()) return leaveModes();
  if (!S.focused || !shownIds().includes(S.focused)) return;
  S.focusMode = true;
  enter();
}

/**
 * Where a restore goes: set when mission control maximized the pane, so a
 * restore reopens it. Mission control opens over the maximized pane first and
 * calls `restore` when it covers it. Another way out forgets it.
 */
export const maxReturn = { to: null as ((restore: () => void) => void) | null };

export function toggleMaximize() {
  if (S.maximized) {
    const back = maxReturn.to;
    maxReturn.to = null;
    // One pane: out of maximize is out of focus mode too. Else back to focus mode when it came from there.
    if (!back) return oneState() ? leaveModes() : restore();
    back(restore);
    return;
  }
  if (!S.focused || !shownIds().includes(S.focused)) return;
  // One pane in focus mode already fills the window: the key leaves it.
  if (oneState() && S.focusMode) return leaveModes();
  S.maximized = S.focused;
  enter();
}

/** Back from maximize: the view's panes show again. */
function restore() {
  S.maximized = null;
  focusPane(S.focused!);
}

const chrome = () => [...document.querySelectorAll<HTMLElement>("#topbar, #sidebar, #statusbar, #ledger")];
/** The chrome a mode hides: all of it, but focus mode keeps the Ledger. */
const hides = (el: HTMLElement) => el.id !== "ledger" || !!S.maximized;

let wasHidden = false;
let wasMaximized: string | null = null;

function enter() {
  seen = shownIds();
  // The tree goes away, so the keys go to the pane.
  focusPane(S.focused!);
}

function leaveModes() {
  S.focusMode = false;
  S.maximized = null;
  maxReturn.to = null;
  if (S.focused) focusPane(S.focused);
  else render();
}

/** Runs before each render: a move away from the view ends the mode. */
export function settleModes() {
  const shown = shownIds();
  if (S.maximized && (S.maximized !== S.focused || !shown.includes(S.maximized))) {
    S.maximized = null;
    maxReturn.to = null;
  }
  // A split or a closed pane keeps some of the sessions. Another view keeps none.
  if (S.focusMode && !shown.some((id) => seen.includes(id))) S.focusMode = false;
  seen = shown;
}

/**
 * Runs after the layout: hides the chrome while a mode is on. The panes hold
 * terminals, so a mode change has no motion: all of it lands in one frame, and
 * the terminals take their new size in that frame.
 */
export function renderModes() {
  const on = hidden();
  const chromeChanged = on !== wasHidden;
  if (!chromeChanged && S.maximized === wasMaximized) return;
  wasHidden = on;
  wasMaximized = S.maximized;
  // Read before any write. The chrome keeps its box while it hides: the tree's scroll position stays.
  const boxes = chrome().map((el) => ({ el, off: on && hides(el), rect: el.getBoundingClientRect() }));
  for (const { el, off, rect } of boxes) {
    // The Ledger can go while the chrome stays hidden: from focus mode into maximize.
    if (off && (chromeChanged || !el.inert)) {
      Object.assign(el.style, { left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
    } else if (!off) {
      for (const prop of ["left", "top", "width", "height"]) el.style.removeProperty(prop);
    }
    el.inert = off;
  }
  document.body.classList.toggle("focus-mode", on);
  document.body.classList.toggle("maximize-mode", !!S.maximized);
  fitWithLayout();
}

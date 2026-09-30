/**
 * Focus mode and maximize. Focus mode hides the rail, the tree, the top bar
 * and the status bar: the panes of the current view fill the window. Maximize
 * shows only the focused pane, with the same chrome hidden. Ctrl+Shift+Up/Down
 * step through the panes of the view, as they step through its rows in the
 * tree. Ctrl+Shift+Left/Right do nothing, and while maximized neither does
 * Up/Down. Keys that go to another view (Next waiting, the
 * palette, Back, a project key) work, and going there ends the mode.
 */
import { isSlot } from "./canvas";
import { runAction } from "./commands";
import { host } from "./dom";
import { currentRow } from "./keyboard";
import type { Action } from "./keys";
import { render } from "./render";
import { S, shownIds } from "./state";
import { holdFits } from "./terminal";
import { focusPane } from "./view";

/** Keys that move in the rail or the tree, which focus mode hides. */
const TREE_KEYS = new Set<Action>(["session-next", "session-prev", "list-project", "list-back", "region-next", "region-prev", "project-menu"]);
/** Keys that move between panes or add one: maximize shows one pane only. */
const PANE_KEYS = new Set<Action>(["focus-left", "focus-right", "focus-up", "focus-down", "split-right", "split-down"]);
/** Keys whose menus open at the rail or a tree row. The chrome comes back first. */
const CHROME_KEYS = new Set<Action>(["new-session", "new-worktree", "add-project", "project-color", "project-theme", "project-changes", "project-files"]);

/** The sessions shown at the last render. Focus mode ends when none of them stays. */
let seen: string[] = [];

const hidden = () => S.focusMode || !!S.maximized;

/**
 * Entering a mode needs a session to act on. Focus mode takes a session row or
 * a group row. Maximize takes a session row only: a group has no one pane. A
 * worktree, Changes, Files, a project row or the rail is not a session. Leaving
 * a mode always works, since the tree is hidden then.
 */
export function modeKeyBlocked(a: Action): boolean {
  if (a !== "focus-mode" && a !== "maximize") return false;
  if (hidden()) return false;
  if (S.atRail) return true;
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
  if (!S.maximized && (a === "session-next" || a === "session-prev")) {
    stepPane(a === "session-next" ? 1 : -1);
    return true;
  }
  if (TREE_KEYS.has(a)) return true;
  if (CHROME_KEYS.has(a)) {
    leaveModes();
    // The menu measures the rail and the tree, so they must be laid out first.
    requestAnimationFrame(() => runAction(a));
    return true;
  }
  return false;
}

/** The next or previous pane of the view, in tree order, wrapping at the ends. */
function stepPane(dir: 1 | -1) {
  const ids = shownIds().filter((id) => !isSlot(id));
  const i = S.focused ? ids.indexOf(S.focused) : -1;
  if (ids.length < 2 || i < 0) return;
  focusPane(ids[(i + dir + ids.length) % ids.length]);
}

export function toggleFocusMode() {
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

export function toggleMaximize() {
  if (S.maximized) return restore();
  if (!S.focused || !shownIds().includes(S.focused)) return;
  const id = S.focused;
  const cell = cellOf(id);
  const others = [...host.querySelectorAll<HTMLElement>(".cell")].filter((c) => c !== cell);
  S.maximized = id;
  if (!cell || !others.length || reduced()) return enter();
  // The view keeps its panes while the pane grows over them and they fade out.
  // Then the layout drops them.
  const from = slotOf(cell);
  S.growing = true;
  holdFits(SLIDE_MS + 20);
  enter();
  lift(cell, from, FULL, others.map((c) => fade(c, 1, 0)), () => {
    S.growing = false;
    render();
  });
}

/** Back from maximize: the pane shrinks into its slot and the other panes fade in. */
function restore() {
  const id = S.maximized!;
  drop();
  S.maximized = null;
  holdFits(SLIDE_MS + 20);
  focusPane(S.focused!);
  const cell = cellOf(id);
  if (!cell || reduced()) return;
  const others = [...host.querySelectorAll<HTMLElement>(".cell")].filter((c) => c !== cell);
  if (!others.length) return;
  lift(cell, FULL, slotOf(cell), others.map((c) => fade(c, 0, 1)));
}

/** The chrome's slide in styles.css takes this long. */
const SLIDE_MS = 200;
const EASE = "cubic-bezier(.2, .7, .2, 1)";
const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
const cellOf = (id: string) => host.querySelector<HTMLElement>(`.cell[data-session="${CSS.escape(id)}"]`);

/** A place in the terminal area, in percent of it: it holds while the chrome slides. */
type Box = { left: string; top: string; width: string; height: string };
const FULL: Box = { left: "0%", top: "0%", width: "100%", height: "100%" };

function slotOf(cell: HTMLElement): Box {
  const area = host.querySelector<HTMLElement>(".layout")!.getBoundingClientRect();
  const r = cell.getBoundingClientRect();
  const pc = (n: number, of: number) => `${(n / of) * 100}%`;
  return { left: pc(r.left - area.left, area.width), top: pc(r.top - area.top, area.height), width: pc(r.width, area.width), height: pc(r.height, area.height) };
}

function fade(el: HTMLElement, from: number, to: number): Animation {
  return el.animate([{ opacity: from }, { opacity: to }], { duration: SLIDE_MS, easing: EASE, fill: "forwards" });
}

/** The pane out of its slot, over the others, and what it leaves behind. */
let lifted: { cell: HTMLElement; slot: HTMLElement; anims: Animation[] } | null = null;

/**
 * Moves a pane from one place to another over the other panes. It moves, it does
 * not scale: the header and the text keep their size. An empty box keeps its
 * slot, so the other panes stay where they are.
 */
function lift(cell: HTMLElement, from: Box, to: Box, fades: Animation[], done?: () => void) {
  const slot = document.createElement("div");
  slot.style.flex = cell.style.flex;
  cell.before(slot);
  cell.classList.add("lifted");
  const move = cell.animate([from, to], { duration: SLIDE_MS, easing: EASE, fill: "forwards" });
  const mine = (lifted = { cell, slot, anims: [move, ...fades] });
  move.onfinish = () => {
    if (lifted !== mine) return;
    // Render first: the pane takes its new place before it lets go of the old one.
    done?.();
    drop();
  };
}

/** Ends a lift at once. */
function drop() {
  if (!lifted) return;
  const { cell, slot, anims } = lifted;
  lifted = null;
  S.growing = false;
  for (const a of anims) a.cancel();
  slot.remove();
  cell.classList.remove("lifted");
}

function enter() {
  seen = shownIds();
  // The rail and the tree go away, so the keys go to the pane.
  S.atRail = false;
  focusPane(S.focused!);
}

export function leaveModes() {
  drop();
  S.focusMode = false;
  S.maximized = null;
  if (S.focused) focusPane(S.focused);
  else render();
}

/** Runs before each render: a move away from the view ends the mode. */
export function settleModes() {
  const shown = shownIds();
  if (S.maximized && (S.maximized !== S.focused || !shown.includes(S.maximized))) {
    S.maximized = null;
    drop();
  }
  // A split or a closed pane keeps some of the sessions. Another view keeps none.
  if (S.focusMode && !shown.some((id) => seen.includes(id))) S.focusMode = false;
  seen = shown;
}

let wasHidden = false;

/** Runs after the layout: hides the chrome while a mode is on. */
export function renderModes() {
  const on = hidden();
  if (on !== wasHidden && !reduced()) holdFits(SLIDE_MS + 20);
  wasHidden = on;
  document.body.classList.toggle("focus-mode", on);
}

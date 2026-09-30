/**
 * Focus mode and maximize. Focus mode hides the rail, the tree, the top bar
 * and the status bar: the panes of the current view fill the window. Maximize
 * shows only the focused pane, with the same chrome hidden. Keys that move in
 * the hidden tree do nothing. Keys that go to another view (Next waiting, the
 * palette, Back, a project key) work, and going there ends the mode.
 */
import { runAction } from "./commands";
import { type Action, keyLabel } from "./keys";
import { sessionsOf } from "./layout";
import { agentName, bySessionPriority, taskTitle } from "./model";
import { $, host } from "./dom";
import { render } from "./render";
import { S, sessions, shownIds, viewLayout } from "./state";
import { focusPane, revealSession } from "./view";

/** Keys that move in the rail or the tree, which focus mode hides. */
const TREE_KEYS = new Set<Action>(["session-next", "session-prev", "list-project", "list-back", "region-next", "region-prev", "project-menu"]);
/** Keys that move between panes or add one: maximize shows one pane only. */
const PANE_KEYS = new Set<Action>(["focus-left", "focus-right", "focus-up", "focus-down", "split-right", "split-down"]);
/** Keys whose menus open at the rail or a tree row. The chrome comes back first. */
const CHROME_KEYS = new Set<Action>(["new-session", "new-worktree", "add-project", "project-color", "project-theme", "project-changes", "project-files"]);

/** The sessions shown at the last render. Focus mode ends when none of them stays. */
let seen: string[] = [];

const hidden = () => S.focusMode || !!S.maximized;

/** True when the mode takes the key: it does nothing, or already ran. */
export function modeGate(a: Action): boolean {
  if (!hidden()) return false;
  if (TREE_KEYS.has(a)) return true;
  if (S.maximized && PANE_KEYS.has(a)) return true;
  if (CHROME_KEYS.has(a)) {
    leaveModes();
    // The menu measures the rail and the tree, so they must be laid out first.
    requestAnimationFrame(() => runAction(a));
    return true;
  }
  return false;
}

export function toggleFocusMode() {
  if (hidden()) return leaveModes();
  if (!S.focused || !shownIds().includes(S.focused)) return;
  S.focusMode = true;
  enter();
}

export function toggleMaximize() {
  if (S.maximized) {
    S.maximized = null;
    return focusPane(S.focused!);
  }
  if (!S.focused || !shownIds().includes(S.focused)) return;
  S.maximized = S.focused;
  enter();
}

function enter() {
  seen = shownIds();
  // The rail and the tree go away, so the keys go to the pane.
  S.atRail = false;
  focusPane(S.focused!);
}

export function leaveModes() {
  S.focusMode = false;
  S.maximized = null;
  if (S.focused) focusPane(S.focused);
  else render();
}

/** Runs before each render: a move away from the view ends the mode. */
export function settleModes() {
  const shown = shownIds();
  if (S.maximized && (S.maximized !== S.focused || !shown.includes(S.maximized))) S.maximized = null;
  // A split or a closed pane keeps some of the sessions. Another view keeps none.
  if (S.focusMode && !shown.some((id) => seen.includes(id))) S.focusMode = false;
  seen = shown;
}

/** Runs after the layout: the chrome, and the corner block for waiting sessions out of view. */
export function renderModes() {
  document.body.classList.toggle("focus-mode", hidden());
  const corner = $<HTMLButtonElement>("waiting-corner");
  const inView = new Set(sessionsOf(viewLayout()));
  const waiting = hidden() ? [...sessions.values()].filter((s) => s.state === "waiting" && !inView.has(s.id)).sort(bySessionPriority) : [];
  corner.hidden = !waiting.length;
  for (const c of host.querySelectorAll(".cell.under-corner")) c.classList.remove("under-corner");
  if (!waiting.length) return;
  const first = waiting[0];
  $("waiting-corner-label").textContent = `${agentName(first)} · ${taskTitle(first)} waiting` + (waiting.length > 1 ? ` +${waiting.length - 1}` : "");
  corner.querySelector("kbd")!.textContent = keyLabel("next-waiting").replace(/\+/g, " ");
  corner.onclick = () => revealSession(first.id);
  // The pane head under the block makes room, so its buttons stay in reach.
  const box = host.getBoundingClientRect();
  const under = [...host.querySelectorAll<HTMLElement>(".cell")].find((c) => {
    const r = c.getBoundingClientRect();
    return Math.abs(r.top - box.top) < 2 && Math.abs(r.right - box.right) < 2;
  });
  under?.classList.add("under-corner");
  host.style.setProperty("--corner-w", `${corner.offsetWidth}px`);
}

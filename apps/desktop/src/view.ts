/** What the terminal area shows and which pane has the keys. */
import { beginDrag, type DropTarget } from "./drag";
import { build, type Direction, neighbor, removePane, replacePane, sessionsOf, splitPane } from "./layout";
import { bySessionPriority, taskTitle } from "./model";
import type { Group, Layout, Project, SplitDir, Worktree } from "./types";
import { focusFirstSlot, isSlot } from "./canvas";
import { host } from "./dom";
import { autoName, deleteGroup, saveGroup } from "./groups";
import { ctxMenu } from "./menus";
import { render } from "./render";
import { clearSelection } from "./selection";
import { activeGroupObj, currentLayout, FULL_HINT, groupOf, MAX_PANES, panes, place, S, selectedWorktree, sessions, shownIds, splitFull, worktreeSessions } from "./state";
import { view } from "./terminal";

/** Give the keys to a shown pane. */
export function focusPane(id: string, grab = true) {
  S.justAdded = null;
  if (grab) S.groupPicked = null;
  if (S.focused && S.focused !== id) S.previous = S.focused;
  S.focused = id;
  const g = activeGroupObj();
  if (g) g.focus = id;
  const s = sessions.get(id);
  const at = s ? place(s) : null;
  if (at) {
    S.selectedProject = at.project.name;
    selectedWorktree.set(at.project.name, at.worktree.path);
  }
  render();
  requestAnimationFrame(() => {
    // A right-click focuses the pane, then opens a menu that keeps the keys.
    // A text field (a group name being typed) keeps them too.
    const a = document.activeElement;
    const typing = a instanceof HTMLInputElement && !host.contains(a);
    if (!ctxMenu.isOpen && !typing && S.focused === id && grab) panes.get(id)?.term.focus();
  });
}

export function refocusTerminal() {
  if (S.focused) panes.get(S.focused)?.term.focus();
}

/** One session fills the area. Any group stays in the sidebar. */
export function showSingle(id: string) {
  S.activeGroup = null;
  S.single = id;
  focusPane(id);
}

/** Show a group. `pick` selects the group itself: the keys stay on its row, not in a terminal. */
export function showGroup(id: string, pick = false, focus?: string) {
  const g = S.groups.find((x) => x.id === id);
  if (!g) return;
  S.activeGroup = id;
  S.single = null;
  const ids = sessionsOf(g.layout).filter((x) => !isSlot(x));
  if (pick) S.groupPicked = id;
  const f = focus && ids.includes(focus) ? focus : g.focus && ids.includes(g.focus) ? g.focus : ids[0];
  if (f) focusPane(f, !pick);
  else {
    // A canvas with every pane empty: the keys go to its first pane's list.
    S.focused = null;
    render();
    if (!pick) focusFirstSlot();
  }
  if (pick) {
    S.roveKey = id;
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`#sidebar-scroll .group-pick[data-group="${id}"]`)?.focus());
  }
}

/**
 * Show a new tree. Two or more panes are a group: the active one is updated,
 * else a new one is made. One pane is a single view and ends the group.
 */
export function applyLayout(next: Layout | null, focus: string | null) {
  const ids = sessionsOf(next);
  const g = activeGroupObj();
  const f = focus && ids.includes(focus) ? focus : ids[0] ?? null;
  if (next && ids.length >= 2) {
    S.single = null;
    let target = g;
    if (target) {
      target.layout = next;
      target.focus = f;
      saveGroup(target);
    } else {
      target = { id: `tmp-${++S.tmpSeq}`, name: autoName(ids), layout: next, focus: f };
      S.groups.push(target);
      S.activeGroup = target.id;
      saveGroup(target);
    }
    releaseFromOtherGroups(target, ids);
  } else {
    if (g) deleteGroup(g);
    S.activeGroup = null;
    S.single = f;
  }
  if (f) focusPane(f);
  else unfocus();
}

/** A session belongs to one group: take `ids` out of every other group. */
function releaseFromOtherGroups(keep: Group, ids: string[]) {
  for (const other of [...S.groups]) {
    if (other === keep) continue;
    let layout: Layout | null = other.layout;
    for (const id of ids) layout = removePane(layout, id);
    if (layout === other.layout || sessionsOf(layout).join() === sessionsOf(other.layout).join()) continue;
    if (!layout || sessionsOf(layout).length < 2) {
      deleteGroup(other);
    } else {
      other.layout = layout;
      if (other.focus && !sessionsOf(layout).includes(other.focus)) other.focus = sessionsOf(layout)[0];
      saveGroup(other);
    }
  }
}

/** Take a session out of its group. One session left ends the group. */
export function removeFromGroup(id: string) {
  const g = groupOf(id);
  if (!g) return;
  if (g.id === S.activeGroup) return closePane(id);
  const rest = removePane(g.layout, id);
  if (!rest || sessionsOf(rest).length < 2) deleteGroup(g);
  else {
    g.layout = rest;
    if (g.focus === id) g.focus = sessionsOf(rest)[0];
    saveGroup(g);
  }
  render();
}

/** Put `id` next to `target`. A session shown elsewhere in the layout moves. */
export function splitWith(target: string | null, dir: SplitDir, id: string) {
  if (!shownIds().includes(id) && splitFull()) return;
  let base = currentLayout();
  if (sessionsOf(base).includes(id)) base = removePane(base, id);
  if (!base) return showSingle(id);
  const ids = sessionsOf(base);
  const t = target && ids.includes(target) ? target : ids[0];
  applyLayout(splitPane(base, t, dir, id), id);
}

/** Click on a session: focus it where it is shown, else show its group, else show it alone. A click never changes a group. */
export function revealSession(id: string) {
  if (shownIds().includes(id)) return focusPane(id);
  const g = S.groups.find((x) => sessionsOf(x.layout).includes(id));
  // One render: the group shows with this pane focused.
  if (g) showGroup(g.id, false, id);
  else showSingle(id);
}

export function closePane(id: string) {
  const next = removePane(currentLayout(), id);
  applyLayout(next, S.focused === id ? null : S.focused);
}

/** Keep the focused pane as a single view and drop the group. Sessions keep running. */
export function unsplit(g: Group) {
  const ids = sessionsOf(g.layout).filter((x) => !isSlot(x));
  const f = g.id === S.activeGroup && S.focused ? S.focused : g.focus && ids.includes(g.focus) ? g.focus : ids[0];
  const active = g.id === S.activeGroup;
  deleteGroup(g);
  if (f) showSingle(f);
  else if (active) unfocus();
  else render();
}

export function moveFocus(dir: Direction) {
  if (!S.focused) return;
  const next = neighbor(view.cellRects(), S.focused, dir);
  if (next) focusPane(next);
}

/**
 * Drop dragged sessions on a pane. An edge splits that pane there; the
 * center replaces it (its session keeps running). Several sessions land
 * as one block, tiled like Open in split.
 */
function dropOn(t: DropTarget, dragged: string[]) {
  const ids = dragged.filter((id) => sessions.has(id));
  if (!ids.length) return;
  if (ids.length === 1 && t.session === ids[0]) return;
  let base = currentLayout();
  for (const id of ids) base = removePane(base, id);
  const block = build(ids);
  const target = t.session && sessionsOf(base).includes(t.session) ? t.session : null;
  if (sessionsOf(base).length + ids.length > MAX_PANES) return;
  clearSelection();
  // Every shown pane was dragged, or nothing was shown: the block is the view.
  if (!base) return applyLayout(block, ids[0]);
  // The target pane was itself dragged away: put the block beside the rest.
  if (!target) return applyLayout({ type: "split", dir: "row", ratio: 0.5, a: base, b: block }, ids[0]);
  let next: Layout;
  if (t.zone === "center") {
    next = replacePane(base, target, () => block);
  } else {
    const dir: SplitDir = t.zone === "left" || t.zone === "right" ? "row" : "col";
    const before = t.zone === "left" || t.zone === "top";
    next = replacePane(base, target, (p) => ({ type: "split", dir, ratio: 0.5, a: before ? block : p, b: before ? p : block }));
  }
  applyLayout(next, ids[0]);
}

/** Start a drag of these sessions; a press that does not move stays a click. */
export function dragSessions(e: MouseEvent, ids: string[]) {
  const first = sessions.get(ids[0]);
  const label = ids.length > 1 ? `${ids.length} sessions` : first ? taskTitle(first) : "session";
  // A full split takes no more panes; moving a pane inside it still works.
  const blocked = splitFull() && ids.some((id) => !shownIds().includes(id));
  beginDrag(e, {
    label: blocked ? `${label}: ${FULL_HINT}` : label,
    cells: () => (blocked ? new Map() : view.cellRects()),
    area: blocked ? null : host,
    drop: (t) => dropOn(t, ids),
  });
}

/**
 * The terminal area follows the selection. A shown split that already holds
 * a session of this worktree stays. Else the worktree's best session shows,
 * in its group if it has one. No session: nothing has the keys.
 */
export function selectWorktree(project: Project, w: Worktree) {
  S.selectedProject = project.name;
  selectedWorktree.set(project.name, w.path);
  const here = worktreeSessions(w);
  if (here.some((x) => shownIds().includes(x.id))) return render();
  const best = here.sort(bySessionPriority)[0];
  if (best) revealSession(best.id);
  else unfocus();
}

/** No session has the keys: hide every pane and offer to start one here. */
export function unfocus() {
  S.focused = null;
  S.single = null;
  S.activeGroup = null;
  (document.activeElement as HTMLElement | null)?.blur();
  render();
  host.querySelector<HTMLButtonElement>(".welcome button")?.focus();
}

export function selectProject(name: string) {
  const p = S.projects.find((x) => x.name === name);
  if (!p) return;
  S.selectedProject = name;
  const path = selectedWorktree.get(name);
  const w = p.worktrees.find((x) => x.path === path) ?? p.worktrees.find((x) => x.is_main) ?? p.worktrees[0];
  if (w) selectWorktree(p, w);
  else unfocus();
}

export function focusNextWaiting() {
  const waiting = [...sessions.values()].filter((s) => s.state === "waiting").sort(bySessionPriority);
  const next = waiting.find((s) => s.id !== S.focused) ?? waiting[0];
  if (next) revealSession(next.id);
}

export function goBack() {
  if (S.previous && sessions.has(S.previous)) revealSession(S.previous);
}

/** Open the next or previous session in the order the list shows. */
export function stepSession(dir: 1 | -1) {
  const rows = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll button.session-row")].filter((r) => r.dataset.session);
  if (!rows.length) return;
  const i = rows.findIndex((r) => r.dataset.session === S.focused);
  const next = rows[i < 0 ? (dir > 0 ? 0 : rows.length - 1) : (i + dir + rows.length) % rows.length];
  revealSession(next.dataset.session!);
}

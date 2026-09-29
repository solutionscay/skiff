/** What the terminal area shows and which pane has the keys. */
import { beginDrag, type DropTarget } from "./drag";
import { build, type Direction, leaf, neighbor, removePane, replacePane, sessionsOf, splitPane } from "./layout";
import { bySessionPriority, taskTitle } from "./model";
import type { Group, Layout, Project, SplitDir, Worktree } from "./types";
import { filledOf, focusFirstSlot, isSlot, slot, slotsOf } from "./canvas";
import { host, showError } from "./dom";
import { autoName, deleteGroup, saveGroup } from "./groups";
import { ctxMenu } from "./menus";
import { render } from "./render";
import { clearSelection } from "./selection";
import { activeGroupObj, currentLayout, FULL_HINT, groupOf, MAX_PANES, OTHER, panes, place, S, selectedWorktree, sessions, shownIds, splitFull, worktreeSessions } from "./state";
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
  } else if (s) {
    S.selectedProject = OTHER;
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
 * else a new one is made. One pane stays in the active group; without one it is a single view.
 */
export function applyLayout(next: Layout | null, focus: string | null) {
  const ids = sessionsOf(next);
  const g = activeGroupObj();
  const f = focus && ids.includes(focus) ? focus : ids[0] ?? null;
  if (next && (ids.length >= 2 || g)) {
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
    if (g) {
      // The group keeps its slot when its last session leaves.
      g.layout = slot();
      g.focus = null;
      saveGroup(g);
      S.single = null;
      render();
      return focusFirstSlot();
    }
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
    // Its last session left: the group stays, with an empty pane.
    other.layout = layout ?? slot();
    if (other.focus && !sessionsOf(other.layout).includes(other.focus)) other.focus = sessionsOf(other.layout).find((x) => !isSlot(x)) ?? null;
    saveGroup(other);
  }
}

/** Take a session out of its group. */
export function removeFromGroup(id: string) {
  const g = groupOf(id);
  if (!g) return;
  if (g.id === S.activeGroup) return closePane(id);
  const kept = emptyLast(g, id);
  if (kept) {
    g.layout = kept;
    g.focus = null;
    saveGroup(g);
    return render();
  }
  const rest = removePane(g.layout, id);
  if (!rest) deleteGroup(g);
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

/** The group's layout with `id` swapped for an empty pane, when it was the last session: the group stays open. */
function emptyLast(g: Group, id: string): Layout | null {
  const rest = removePane(g.layout, id);
  return filledOf(rest).length === 0 ? replacePane(g.layout, id, slot) : null;
}

export function closePane(id: string) {
  const g = activeGroupObj();
  const kept = g && sessionsOf(g.layout).includes(id) ? emptyLast(g, id) : null;
  if (g && kept) {
    g.layout = kept;
    g.focus = null;
    S.focused = null;
    saveGroup(g);
    render();
    return focusFirstSlot();
  }
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
  if (t.group) return addToGroup(t.group, ids);
  if (ids.length === 1 && t.session === ids[0]) return;
  let base = currentLayout();
  for (const id of ids) base = removePane(base, id);
  const block = build(ids);
  const target = t.session && sessionsOf(base).includes(t.session) ? t.session : null;
  if (sessionsOf(base).length + ids.length > MAX_PANES) return;
  clearSelection();
  // The rim of the area: wrap every pane, so one pane spans the full width or height (a T shape).
  if (t.outer && base) {
    const dir: SplitDir = t.zone === "left" || t.zone === "right" ? "row" : "col";
    const before = t.zone === "left" || t.zone === "top";
    return applyLayout({ type: "split", dir, ratio: 0.5, a: before ? block : base, b: before ? base : block }, ids[0]);
  }
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

/** Drop sessions on a group row: they fill its empty panes, then split in. A full group takes no more. */
function addToGroup(groupId: string, dragged: string[]) {
  const g = S.groups.find((x) => x.id === groupId);
  if (!g) return;
  const ids = dragged.filter((id) => !sessionsOf(g.layout).includes(id));
  if (!ids.length) return;
  const room = MAX_PANES - sessionsOf(g.layout).length + slotsOf(g.layout).length;
  if (ids.length > room) return showError(`The group is full (${MAX_PANES} panes).`);
  releaseFromOtherGroups(g, ids);
  let layout = g.layout;
  for (const id of ids) {
    const slot = slotsOf(layout)[0];
    if (slot) layout = replacePane(layout, slot, () => leaf(id));
    else if (sessionsOf(layout).length === 2) layout = splitPane(layout, sessionsOf(layout)[1], "col", id);
    else layout = build([...sessionsOf(layout), id]);
  }
  g.layout = layout;
  if (S.single && ids.includes(S.single)) S.single = null;
  clearSelection();
  saveGroup(g);
  showGroup(g.id, false, ids[0]);
}

/** Each group's drop area: its header row and its nested session rows, as one box. */
function groupRects() {
  const boxes = new Map<string, DOMRect>();
  for (const el of document.querySelectorAll<HTMLElement>("#sidebar-scroll .group-row, #sidebar-scroll .session-row[data-in-group]")) {
    const id = el.dataset.group ?? el.dataset.inGroup;
    if (!id) continue;
    const r = el.getBoundingClientRect();
    const b = boxes.get(id);
    boxes.set(id, b ? new DOMRect(Math.min(b.left, r.left), Math.min(b.top, r.top), Math.max(b.right, r.right) - Math.min(b.left, r.left), Math.max(b.bottom, r.bottom) - Math.min(b.top, r.top)) : r);
  }
  return [...boxes].map(([id, rect]) => ({ id, rect }));
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
    groupRows: groupRects,
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

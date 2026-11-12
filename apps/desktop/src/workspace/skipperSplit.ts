/**
 * The Skipper split: a Skipper's pane at the left, one child pane at the
 * right, the Ledger beside them. A child is a live session of the
 * Skipper's scope (`splitChildren`). The split is a view of the single view: no group is
 * made, and the child session keeps running when the split closes.
 *
 * It opens from the split button on the Skipper's head, the next-child key,
 * and a Ledger row with a live pane. It closes from × on the child's head,
 * the close key in the child's pane, the split button, and at once when the
 * last child pane goes. A pane drag or Split with in the split moves its
 * panes and keeps the split (`keepSplit`).
 */
import { coordinatorOf } from "../app/skiffAgent";
import { render } from "../app/renderRequest";
import { S, sessions } from "../app/state";
import { skipperSplit } from "../app/stateQueries";
import type { Layout } from "../platform/types";
import { leaf, replacePane, sessionsOf } from "./layout";
import { isSlot } from "./layoutSlots";
import { ledgerRows, ledgerTitle } from "./ledgerStore";
import { ownPanes } from "./ledgerModel";
import { scopeOrder, worktreeLines } from "./sidebarLines";
import { focusPane } from "./view";

/** The Skipper whose view can hold a split: its pane is the single view. */
export function splitSkipper(): string | null {
  const sp = skipperSplit();
  if (sp) return sp.skipper;
  if (S.activeGroup || !S.single || !sessions.has(S.single)) return null;
  return coordinatorOf(S.single) ? S.single : null;
}

/** The child pane on screen, else null. */
export const splitChild = () => skipperSplit()?.child ?? null;

/** The newest Ledger task of the pane, else "". */
function paneTask(id: string): string {
  const r = [...ledgerRows()].reverse().find((x) => x.pane_id === id);
  return r ? ledgerTitle(r) : "";
}

/**
 * The children of the Skipper on screen. A project Skipper: every live
 * session of its project, shells too, in tree order. A fleet Skipper: the
 * same over its fleet's projects. The Skiff agent: its Ledger panes
 * (`ownPanes`), then every live shell in tree order. Never the Skipper's own pane, the Skiff agent, or a
 * Skipper of another scope.
 */
export function splitChildren(sk: string | null = splitSkipper()): { id: string; title: string }[] {
  const scope = sk && coordinatorOf(sk);
  if (!sk || !scope) return [];
  if (scope === "grand") {
    const own = ownPanes(sk);
    // Shells have no Ledger row: each live shell of every project, then those in no project.
    const ids = [...S.projects.flatMap((p) => scopeOrder(p.name)), ...worktreeLines(null).flatMap((l) => ("session" in l ? [l.session.id] : []))];
    const shells = ids.filter((id, i) => ids.indexOf(id) === i && sessions.get(id)?.role === "shell" && !own.some((c) => c.id === id));
    return [...own, ...shells.map((id) => ({ id, title: paneTask(id) }))];
  }
  const inScope = (id: string) => {
    const c = coordinatorOf(id);
    return !c || c === scope || S.projects.some((p) => p.name === c && scope === `fleet:${p.fleet}`);
  };
  return scopeOrder(scope).filter((id) => id !== sk && inScope(id)).map((id) => ({ id, title: paneTask(id) }));
}

/** True when `id` can show in the split: a Skipper view is on screen and `id` is another live pane. */
export function canSplitWith(id: string): boolean {
  const sk = splitSkipper();
  return !!sk && id !== sk && sessions.has(id);
}

/**
 * Shows `id` as the child. `focus`: the child takes the keys and its row; else
 * the keys stay where they are, unless the child that leaves had them.
 */
export function showChild(id: string, focus = false) {
  const sk = splitSkipper();
  if (!sk || id === sk || !sessions.has(id)) return;
  const was = skipperSplit();
  S.skipperSplits.set(sk, { child: id, layout: childIn(sk, id) });
  // The keys were in the child that leaves: they go to the new one.
  if (focus || (was && S.focused === was.child)) return focusPane(id);
  render();
}

/** Closes the split. The child session keeps running; the Skipper's pane takes the keys if the child had them. */
export function closeSplit() {
  const sp = skipperSplit();
  // Closed, it keeps its tree: the divider and the child's place come back on the next open.
  if (sp) S.skipperSplits.set(sp.skipper, { child: null, layout: sp.layout });
  if (sp && S.focused === sp.child) return focusPane(sp.skipper);
  render();
}

/** The split button: opens the split on the first child, or closes it. */
export function toggleSplit() {
  if (skipperSplit()) return closeSplit();
  const first = splitChildren()[0];
  if (first) showChild(first.id);
}

/**
 * The next or previous child, wrapping. With the split closed, the next key
 * opens it on the first child and the previous key on the last. The child
 * takes the keys, so the next key goes to the pane on screen. False when no
 * Skipper view is on screen, so the key does nothing.
 */
export function stepChild(dir: 1 | -1): boolean {
  if (!splitSkipper()) return false;
  const list = splitChildren().map((c) => c.id);
  if (!list.length) return true;
  const now = splitChild();
  const i = now ? list.indexOf(now) : -1;
  const next = i < 0 ? list[dir === 1 ? 0 : list.length - 1] : list[(i + dir + list.length) % list.length];
  if (next !== now) showChild(next, true);
  return true;
}

/** The Skipper's saved tree with `child` in the last child's place: a moved or resized split keeps its shape. */
function childIn(sk: string, child: string): Layout {
  const saved = S.skipperSplits.get(sk)?.layout;
  const old = saved && sessionsOf(saved).find((x) => x !== sk);
  return saved && old ? replacePane(saved, old, () => leaf(child)) : { type: "split", dir: "row", ratio: 0.5, a: leaf(sk), b: leaf(child) };
}

/**
 * `sk` comes on screen. Its split comes back with its child while that one
 * is live and a child; else the first child takes its place; with none, it
 * closes. A closed split stays closed.
 */
export function restoreSplit(sk: string) {
  const sp = S.skipperSplits.get(sk);
  if (!sp?.child) return;
  const list = splitChildren(sk).map((c) => c.id);
  if (list.includes(sp.child) && sessions.has(sp.child)) return;
  const next = list.find((x) => sessions.has(x));
  S.skipperSplits.set(sk, next ? { child: next, layout: childIn(sk, next) } : { ...sp, child: null });
}

/**
 * A session is about to go. Call before it leaves `sessions`; run the answer
 * after. In each Skipper's split, the child that goes gives its place to the
 * next child; with no other child the split closes. A Skipper that goes
 * takes its split along. The answer returns the child that took `id`'s place
 * in the split on screen, else null.
 */
export function splitLoses(id: string): () => string | null {
  const after: (() => void)[] = [];
  const shown = skipperSplit();
  let heir: string | null = null;
  if (S.skipperSplits.has(id)) after.push(() => void S.skipperSplits.delete(id));
  for (const [sk, sp] of S.skipperSplits) {
    if (sp.child !== id) continue;
    const list = splitChildren(sk).map((c) => c.id);
    const i = list.indexOf(id);
    const rest = list.filter((x) => x !== id);
    const next = rest.length ? rest[(i < 0 ? 0 : i) % rest.length] : null;
    after.push(() => {
      if (S.skipperSplits.get(sk) !== sp) return;
      const live = next && sessions.has(next) ? next : null;
      S.skipperSplits.set(sk, live ? { child: live, layout: childIn(sk, live) } : { ...sp, child: null });
      if (live && shown?.skipper === sk) heir = live;
    });
  }
  return () => {
    after.forEach((f) => f());
    return heir;
  };
}

/**
 * A pane drop or Split with made `next` while the split shows. The split
 * keeps the new shape when the Skipper stays: two panes keep both; with a
 * third session, that one becomes the child in the old child's place. The
 * Skipper alone closes the split. False when the Skipper left `next`: the
 * caller makes its group.
 */
export function keepSplit(next: Layout | null, focus: string | null): boolean {
  const sp = skipperSplit();
  if (!sp || !next) return false;
  const ids = sessionsOf(next).filter((x) => !isSlot(x) && sessions.has(x));
  if (!ids.includes(sp.skipper)) return false;
  const others = ids.filter((x) => x !== sp.skipper);
  if (!others.length) {
    closeSplit();
    return true;
  }
  if (others.length > 1) {
    const add = others.find((x) => x !== sp.child) ?? others[0];
    showChild(add, focus === add);
    return true;
  }
  S.skipperSplits.set(sp.skipper, { child: others[0], layout: next });
  focusPane(focus && ids.includes(focus) ? focus : others[0]);
  return true;
}

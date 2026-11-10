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
import { scopeOrder } from "./sidebarLines";
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
 * (`ownPanes`). Never the Skipper's own pane, the Skiff agent, or a
 * Skipper of another scope.
 */
export function splitChildren(): { id: string; title: string }[] {
  const sk = splitSkipper();
  const scope = sk && coordinatorOf(sk);
  if (!sk || !scope) return [];
  if (scope === "grand") return ownPanes(sk);
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

/** Shows `id` as the child. `focus`: the child takes the keys; else the keys stay where they are. */
export function showChild(id: string, focus = false) {
  const sk = splitSkipper();
  if (!sk || id === sk || !sessions.has(id)) return;
  const was = skipperSplit();
  // The new child takes the old one's place: a moved or resized split keeps its shape.
  const layout: Layout = was ? replacePane(was.layout, was.child, () => leaf(id)) : { type: "split", dir: "row", ratio: 0.5, a: leaf(sk), b: leaf(id) };
  S.skipperSplit = { skipper: sk, child: id, layout };
  // The keys were in the child that leaves: they go to the new one.
  if (focus || (was && S.focused === was.child)) return focusPane(id);
  render();
}

/** Closes the split. The child session keeps running; the Skipper's pane takes the keys if the child had them. */
export function closeSplit() {
  const sp = skipperSplit();
  S.skipperSplit = null;
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
 * opens it on the first child and the previous key on the last. False when
 * no Skipper view is on screen, so the key does nothing.
 */
export function stepChild(dir: 1 | -1): boolean {
  if (!splitSkipper()) return false;
  const list = splitChildren().map((c) => c.id);
  if (!list.length) return true;
  const now = splitChild();
  const i = now ? list.indexOf(now) : -1;
  const next = i < 0 ? list[dir === 1 ? 0 : list.length - 1] : list[(i + dir + list.length) % list.length];
  if (next !== now) showChild(next);
  return true;
}

/**
 * A session is about to go. Call before it leaves `sessions`; run the answer
 * after. The child that goes gives its place to the next child; with no
 * other child the split closes. The Skipper that goes takes its split along.
 */
export function splitLoses(id: string): () => void {
  const sp = S.skipperSplit;
  if (!sp) return () => {};
  if (sp.skipper === id) return () => void (S.skipperSplit = null);
  if (sp.child !== id) return () => {};
  const list = skipperSplit() ? splitChildren().map((c) => c.id) : [];
  const i = list.indexOf(id);
  const rest = list.filter((x) => x !== id);
  const next = rest.length ? rest[(i < 0 ? 0 : i) % rest.length] : null;
  return () => {
    if (S.skipperSplit !== sp) return;
    S.skipperSplit = next && sessions.has(next) ? { ...sp, child: next, layout: replacePane(sp.layout, sp.child, () => leaf(next)) } : null;
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
  S.skipperSplit = { ...sp, child: others[0], layout: next };
  focusPane(focus && ids.includes(focus) ? focus : others[0]);
  return true;
}

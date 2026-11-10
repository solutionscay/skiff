/**
 * The Skipper split: a Skipper's pane at the left, one child pane at the
 * right, the Ledger beside them. A child is a live pane of an open row the
 * Ledger shows (`childPanes`). The split is a view of the single view: no
 * group is made, and the child session keeps running when the split closes.
 *
 * It opens from the split button on the Skipper's head, the next-child key,
 * and a Ledger row with a live pane. It closes from × on the child's head,
 * the close key in the child's pane, the split button, and at once when the
 * last child pane goes.
 */
import { coordinatorOf } from "../app/skiffAgent";
import { render } from "../app/renderRequest";
import { S, sessions } from "../app/state";
import { skipperSplit } from "../app/stateQueries";
import { leaf } from "./layout";
import { childPanes } from "./ledgerModel";
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

/** The children of the Skipper on screen, in Ledger order. */
export function splitChildren(): { id: string; title: string }[] {
  const sk = splitSkipper();
  return sk ? childPanes(sk) : [];
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
  const ratio = was?.layout.type === "split" ? was.layout.ratio : 0.5;
  S.skipperSplit = { skipper: sk, child: id, layout: { type: "split", dir: "row", ratio, a: leaf(sk), b: leaf(id) } };
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
    S.skipperSplit = next && sessions.has(next) ? { ...sp, child: next, layout: { ...sp.layout, b: leaf(next) } as typeof sp.layout } : null;
  };
}

/** Multi-select in the session list, and the bar that splits it. */
import { build, sessionsOf } from "./layout";
import type { Group } from "./types";
import { $, button, h } from "./dom";
import { itemKey, listItems } from "./keyboard";
import { render } from "./render";
import { groupOf, MAX_PANES, S, sessions } from "./state";
import { applyLayout, refocusTerminal, showGroup } from "./view";

/** Where a Shift+click range starts: the last row toggled, else the open session. */
let selectAnchor: string | null = null;

export function clearSelection() {
  S.selection = [];
  selectAnchor = null;
}

/** The first Ctrl+click starts from what is open: a picked group's members, else the open session. */
function seedSelection(except: string[]) {
  if (S.selection.length) return;
  const g = S.groups.find((x) => x.id === S.groupPicked);
  const seed = g ? members(g) : S.focused && sessions.has(S.focused) ? [S.focused] : [];
  S.selection = seed.filter((x) => !except.includes(x));
}

const members = (g: Group) => sessionsOf(g.layout).filter((x) => sessions.has(x));

export function toggleSelect(id: string) {
  // The open session counts as picked, as the last-clicked file does in a file manager.
  seedSelection([id]);
  S.selection = S.selection.includes(id) ? S.selection.filter((x) => x !== id) : [...S.selection, id];
  selectAnchor = id;
  render();
  keepRow(id);
}

/** Ctrl+click on a group's header: all its sessions in or out of the selection. */
export function toggleSelectGroup(g: Group) {
  const ids = members(g);
  seedSelection(ids);
  const all = ids.every((x) => S.selection.includes(x));
  S.selection = all ? S.selection.filter((x) => !ids.includes(x)) : [...S.selection, ...ids.filter((x) => !S.selection.includes(x))];
  render();
}

/** A selection gesture leaves the keys on the row, so Shift+Up/Down and Esc carry on from it. */
export function keepRow(id: string) {
  S.roveKey = id;
  // The row claims the keys: a terminal still attaching must not take them when it opens.
  S.grab = null;
  listItems().find((x) => itemKey(x) === id)?.focus();
}

/** Shift+Up/Down: the selection is the rows from the anchor to the row the cursor moves to. */
export function extendSelection(dir: 1 | -1, el: HTMLElement) {
  const order = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll button.session-row")].filter((r) => r.dataset.session);
  const to = order.indexOf(el) + dir;
  if (to < 0 || to >= order.length) return;
  const anchor = selectAnchor && order.some((r) => r.dataset.session === selectAnchor) ? selectAnchor : el.dataset.session!;
  const from = order.findIndex((r) => r.dataset.session === anchor);
  const [lo, hi] = [from, to].sort((a, b) => a - b);
  S.selection = order.slice(lo, hi + 1).map((r) => r.dataset.session!);
  selectAnchor = anchor;
  S.roveKey = order[to].dataset.session!;
  render();
  listItems().find((x) => itemKey(x) === S.roveKey)?.focus();
}

/** Shift+click: add every row from the anchor to `id`, in the order shown. */
export function selectRange(id: string) {
  const order = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .session-row")]
    .map((r) => r.dataset.session!)
    .filter(Boolean);
  const from = [selectAnchor, S.focused].find((x) => x && order.includes(x)) ?? id;
  const [i, j] = [order.indexOf(from), order.indexOf(id)].sort((a, b) => a - b);
  for (const x of order.slice(i, j + 1)) if (!S.selection.includes(x)) S.selection.push(x);
  selectAnchor = from;
  render();
  keepRow(id);
}

/**
 * What the selection's button does. All members of one group: show it.
 * A whole group plus loose sessions: add the loose ones to it.
 * Anything else: a new group. `status` says it in the bar.
 */
export function selectionPlan(): { label: string; status: string; size: number; run: () => void } | null {
  const ids = S.selection.filter((id) => sessions.has(id));
  const n = ids.length;
  const touched = [...new Set(ids.map((id) => groupOf(id)).filter((g): g is Group => !!g))];
  const loose = ids.filter((id) => !groupOf(id));
  // The selection is one whole group: it is already on screen, so there is nothing to do.
  if (touched.length === 1 && !loose.length && ids.length === sessionsOf(touched[0].layout).filter((x) => sessions.has(x)).length) {
    return null;
  }
  // A whole group plus loose sessions: they join it. Part of a group is a new group.
  if (touched.length === 1 && loose.length && members(touched[0]).every((x) => ids.includes(x))) {
    const g = touched[0];
    const all = [...sessionsOf(g.layout), ...loose];
    return {
      label: `Add to ${g.name}`,
      status: `${n} selected. Enter to add ${loose.length} to ${g.name}.`,
      size: all.length,
      run: () => {
        clearSelection();
        showGroup(g.id);
        applyLayout(build(all), loose[0]);
      },
    };
  }
  return {
    label: "New group",
    status: touched.length ? `${n} selected. Enter to make a new group; they leave their groups.` : `${n} selected. Enter to group them.`,
    size: n,
    run: () => {
      clearSelection();
      S.activeGroup = null;
      applyLayout(build(ids), ids[0]);
    },
  };
}

export function splitSelection() {
  const plan = selectionPlan();
  if (!plan || S.selection.length < 2 || plan.size > MAX_PANES) return;
  plan.run();
}

export function renderSelectBar() {
  const bar = $<HTMLElement>("select-bar");
  const n = S.selection.length;
  const plan = selectionPlan();
  bar.hidden = n === 0 || !plan;
  if (!n || !plan) {
    delete bar.dataset.sig;
    return;
  }
  const tooMany = plan.size > MAX_PANES;
  const ready = n >= 2 && !tooMany;
  const split = button("sel-open", plan.label, splitSelection);
  split.disabled = !ready;
  split.title = ready ? plan.label : tooMany ? `A group holds ${MAX_PANES}. Deselect ${plan.size - MAX_PANES}.` : "Select at least 2 sessions.";
  const label = tooMany
    ? `${n} selected. A group holds ${MAX_PANES}: deselect ${plan.size - MAX_PANES}.`
    : n === 1
      ? "1 selected. Ctrl+click to add more."
      : plan.status;
  // Renders run while agents print. Rebuilding the button between mouse down
  // and mouse up would lose the click, so the bar changes only when its text does.
  const sig = `${plan.label}|${label}|${ready}|${tooMany}`;
  if (bar.dataset.sig === sig) return;
  bar.dataset.sig = sig;
  const actions = h("div", "sel-actions");
  actions.append(
    split,
    button("sel-clear", "Clear", () => {
      clearSelection();
      render();
      refocusTerminal();
    }),
  );
  bar.replaceChildren(
    h("div", `sel-label${tooMany ? " warn" : ready ? " ready" : ""}`, label),
    actions,
  );
}

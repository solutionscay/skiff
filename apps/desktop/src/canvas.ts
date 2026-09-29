/**
 * A split canvas: pick a layout, then an agent or a shell for each pane.
 * The group exists from the first click. Its empty panes are layout leaves
 * with a `slot:` id; skiffd keeps them, so the canvas survives a reload.
 */
import { launchIcon } from "./agentIcon";
import { newSession } from "./daemon";
import { button, h, icon, showError } from "./dom";
import { autoName, deleteGroup, saveGroup } from "./groups";
import { leaf, removePane, replacePane, sessionsOf } from "./layout";
import { render } from "./render";
import { collapsed, enabledAgents, S, selectedWorktree, sessions } from "./state";
import type { AgentInfo, Group, Layout, Project, SplitDir, Worktree } from "./types";
import { focusPane, showSingle, unfocus } from "./view";

export const isSlot = (id: string) => id.startsWith("slot:");

export const slot = () => leaf(`slot:${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`);
const sp = (dir: SplitDir, a: Layout, b: Layout, ratio = 0.5): Layout => ({ type: "split", dir, ratio, a, b });

/** The layouts the + menu offers. Four panes at most, as any split. */
export const PRESETS: { name: string; make: () => Layout }[] = [
  { name: "2 side by side", make: () => sp("row", slot(), slot()) },
  { name: "2 stacked", make: () => sp("col", slot(), slot()) },
  { name: "1 left, 2 right", make: () => sp("row", slot(), sp("col", slot(), slot())) },
  { name: "1 over 2", make: () => sp("col", slot(), sp("row", slot(), slot())) },
  { name: "3 side by side", make: () => sp("row", slot(), sp("row", slot(), slot()), 1 / 3) },
  { name: "2 by 2", make: () => sp("col", sp("row", slot(), slot()), sp("row", slot(), slot())) },
];

export const slotsOf = (l: Layout | null) => sessionsOf(l).filter(isSlot);
export const filledOf = (l: Layout | null) => sessionsOf(l).filter((id) => !isSlot(id));

const holding = (id: string) => S.groups.find((g) => sessionsOf(g.layout).includes(id)) ?? null;

/** Slots whose session is being created, with what is starting. */
const starting = new Map<string, string>();
/** Groups whose name follows their agents, with the name given last; a rename stops it. */
const autoNamed = new Map<Group, string>();

/** Make the group now, all panes empty, and show it. */
export function startCanvas(p: Project, w: Worktree, preset: number) {
  S.selectedProject = p.name;
  selectedWorktree.set(p.name, w.path);
  collapsed.delete(w.path);
  S.justAdded = null;
  const g: Group = { id: `tmp-${++S.tmpSeq}`, name: PRESETS[preset].name, layout: PRESETS[preset].make(), focus: null, cwd: w.path };
  S.groups.push(g);
  autoNamed.set(g, g.name);
  saveGroup(g);
  S.activeGroup = g.id;
  S.single = null;
  S.focused = null;
  render();
  focusFirstSlot();
}

export function focusFirstSlot() {
  requestAnimationFrame(() => document.querySelector<HTMLElement>("#terminal-host .slot button")?.focus());
}

/** Where a group's empty panes start: its own folder, else its first session's. */
export function cwdOf(g: Group): string | null {
  if (g.cwd) return g.cwd;
  const first = sessions.get(filledOf(g.layout)[0]);
  return first?.cwd ?? null;
}

async function fill(id: string, a: AgentInfo | null, body: HTMLElement) {
  const g = holding(id);
  if (!g || starting.has(id)) return;
  starting.set(id, a?.id ?? "shell");
  slotBody(id, body);
  try {
    await newSession(cwdOf(g), a?.command ?? null, a?.id ?? "", a ? "agent" : "shell", { slot: id });
  } catch (e) {
    showError(e);
  } finally {
    starting.delete(id);
    if (body.isConnected && holding(id)) slotBody(id, body);
  }
}

/** newSession calls this once the session exists. False: the slot is gone; the session stays loose. */
export function placeInSlot(id: string, session: string): boolean {
  const g = holding(id);
  if (!g) return false;
  g.layout = replacePane(g.layout, id, () => leaf(session));
  if (autoNamed.get(g) === g.name) {
    g.name = autoName(filledOf(g.layout));
    autoNamed.set(g, g.name);
  }
  saveGroup(g);
  if (g.id !== S.activeGroup) return true;
  if (slotsOf(g.layout).length) {
    // Keys stay on the canvas until every pane has an agent.
    S.focused = session;
    g.focus = session;
    render();
    focusFirstSlot();
  } else {
    focusPane(session);
  }
  return true;
}

function removeSlot(id: string) {
  const g = holding(id);
  if (!g) return;
  const l = removePane(g.layout, id);
  if (l && sessionsOf(l).length >= 2) {
    g.layout = l;
    saveGroup(g);
    render();
    if (!slotsOf(l).length && S.focused) focusPane(S.focused);
    return;
  }
  // One pane left is not a split.
  const active = g.id === S.activeGroup;
  deleteGroup(g);
  const rest = filledOf(l);
  if (!active) return render();
  if (rest.length) showSingle(rest[0]);
  else unfocus();
}

/** An empty pane: every enabled agent, then Shell, as one flush list. Digits pick. */
export function slotBody(id: string, body: HTMLElement) {
  const box = h("div", "slot");
  box.dataset.slot = id;
  const pending = starting.get(id);
  if (pending) {
    box.append(h("div", "slot-wait", `Starting ${pending}…`));
    return void body.replaceChildren(box);
  }
  const list = h("div", "slot-list");
  [...enabledAgents(), null].forEach((a, i) => {
    const b = button("slot-item", "", () => void fill(id, a, body));
    b.append(h("span", "slot-key mono", i < 9 ? String(i + 1) : ""), launchIcon(a), h("span", "slot-name", a ? a.id : "shell"), h("span", "slot-cmd mono", a?.command ?? "$SHELL"));
    list.appendChild(b);
  });
  box.append(list);
  box.addEventListener("keydown", (e) => {
    const bs = [...list.querySelectorAll<HTMLButtonElement>("button")];
    const i = bs.indexOf(document.activeElement as HTMLButtonElement);
    const slots = [...document.querySelectorAll<HTMLElement>("#terminal-host .slot")];
    const k = slots.indexOf(box);
    const toSlot = (n: number) => slots[(n + slots.length) % slots.length]?.querySelector<HTMLElement>("button")?.focus();
    if (/^[1-9]$/.test(e.key)) bs[Number(e.key) - 1]?.click();
    else if (e.key === "ArrowDown") bs[(i + 1) % bs.length]?.focus();
    else if (e.key === "ArrowUp") bs[(i - 1 + bs.length) % bs.length]?.focus();
    else if (e.key === "Tab") toSlot(k + (e.shiftKey ? -1 : 1));
    else if (e.key === "Delete") removeSlot(id);
    else return;
    e.preventDefault();
    e.stopPropagation();
  });
  body.replaceChildren(box);
}

/** The head of an empty pane: what it is, and × to drop it. */
export function slotHead(id: string, head: HTMLElement) {
  if (head.dataset.sig === "slot") return;
  head.dataset.sig = "slot";
  head.replaceChildren(h("span", "title dim", "Start a session"));
  const x = button("head-btn", "", () => removeSlot(id));
  x.title = "Remove this pane";
  x.setAttribute("aria-label", "Remove this pane");
  x.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
  head.appendChild(x);
}

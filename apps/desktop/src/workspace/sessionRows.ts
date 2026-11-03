/** Session, pointer and group rows. */
import { type Line } from "./sidebarLines";
import { groupRenameInput } from "./rename";
import { filledOf } from "./layoutSlots";
import { agentIcon, agentKind } from "../appearance/agentIcon";
import { appliedTheme, ownTheme, signatureColor } from "../appearance/themes";
import { ink } from "../appearance/appTheme";
import { glyph } from "./layoutIcon";
import { agentName, basename, branchName, taskTitle } from "./model";
import type { Group, SessionInfo } from "../platform/types";
import { h } from "../ui/dom";
import { groupMenu, rowMenu } from "./menus";
import { renameRow, startRename, startSessionRename } from "./rename";
import { clearSelection, keepRow, selectRange, toggleSelect, toggleSelectGroup } from "./selection";
import { stateIcon } from "../appearance/stateIcon";
import { stateLabel, statusSig } from "./programStatus";
import { S, sessions } from "../app/state";
import { accent, awayPlaces, place } from "../app/stateQueries";
import { dragGroup, dragSessions, revealSession, showGroup } from "./view";
import { t } from "../i18n";
import { tally } from "../diagnostics/stalls";

/**
 * Puts `nodes` in `parent` in order. Kept nodes stay where they are: a node
 * taken out and put back loses its style and restarts its animations.
 * `before` runs first when anything moves.
 */
export function setChildren(parent: HTMLElement, nodes: HTMLElement[], before: () => void = () => {}) {
  if (parent.children.length === nodes.length && nodes.every((n, i) => parent.children[i] === n)) return;
  before();
  // A new parent: no node to keep in place.
  if (!parent.children.length) return parent.append(...nodes);
  const want = new Set<Node>(nodes);
  for (const c of [...parent.children]) if (!want.has(c)) c.remove();
  nodes.forEach((n, i) => {
    if (parent.children[i] !== n) parent.insertBefore(n, parent.children[i] ?? null);
  });
}

/** A row to draw: what it shows apart from its state, how to make it, and how a kept one takes the new state. */
type Keyed = [key: string, build: () => HTMLElement, update?: (row: HTMLElement) => void];

/** Each session block's rows, by key: the same block drawn again keeps them. */
const keptRows = new WeakMap<HTMLElement, Map<string, HTMLElement>>();

/**
 * Draws `rows` into `block`. A row whose key is unchanged is the row drawn
 * last time: it keeps its node, listeners, icons, hover and focus, and only
 * takes the new state. The rest are built. Rows that stay do not move.
 */
function keyedRows(block: HTMLElement, rows: Keyed[]): HTMLElement {
  const old = keptRows.get(block);
  const next = new Map<string, HTMLElement>();
  const nodes = rows.map(([key, build, update]) => {
    let row = next.has(key) ? undefined : old?.get(key);
    if (row) {
      tally("sidebar.kept");
      update?.(row);
    } else row = build();
    next.set(key, row);
    return row;
  });
  keptRows.set(block, next);
  setChildren(block, nodes);
  return block;
}

/** What a session row draws apart from its state: applyLook writes the state, keyedRows keeps the row. */
function sessionKey(s: SessionInfo, color: string, o: RowOpts): string {
  const at = o.branch ? place(s) : null;
  return JSON.stringify([
    S.renamingSession === s.id ? "rename" : "session", s.id, color, o.in?.id ?? null, o.in ? groupColor(o.in) : null, !!o.groupRail,
    o.branch ? [at ? branchName(at.worktree) : null] : null, o.where ?? null,
    awayPlaces(s).map((a) => branchName(a.worktree)), iconKey(s),
  ]);
}

/** What a row's agent icon draws: the agent, the session's own theme color, the app theme it is inked for. */
function iconKey(s: SessionInfo) {
  const own = ownTheme(s);
  return [agentKind(s), own ? signatureColor(own) : null, appliedTheme()];
}

const keepSession = (s: SessionInfo) => (row: HTMLElement) => {
  // The rename field keeps its text and caret.
  if (row.classList.contains("renaming")) return;
  row.classList.toggle("selected", S.selection.includes(s.id));
  applyLook(row, s);
};

/** Groups by object: a new group list brings new objects, and the rows that hold them are built again. */
const groupIds = new WeakMap<Group, number>();
let groupNext = 0;
const groupId = (g: Group) => groupIds.get(g) ?? (groupIds.set(g, ++groupNext), groupNext);

function lineRow(l: Line, color: string): Keyed {
  if (l.group) {
    const g = l.group;
    const lead = sessions.get(g.focus ?? filledOf(g.layout).filter((id) => sessions.has(id))[0]);
    const key = JSON.stringify(["group", groupId(g), g.name, g.layout, l.hasPrevious, l.hasNext, groupColor(g), accent(lead ? place(lead)?.project : null), S.renaming?.id === g.id, appliedTheme()]);
    return [key, () => groupRow(g, l.hasPrevious, l.hasNext)];
  }
  if (l.pointer) {
    const s = l.pointer;
    const at = place(s);
    const key = JSON.stringify(["pointer", s.id, at ? branchName(at.worktree) : basename(s.cwd), taskTitle(s), iconKey(s)]);
    return [key, () => pointerRow(s)];
  }
  return [sessionKey(l.session, color, l), () => sessionRow(l.session, color, l), keepSession(l.session)];
}

/** 20px lines, flush against whatever follows. `block`: the one drawn last time, whose rows stay. */
export function sessionBlock(lines: Line[], color: string, block: HTMLElement = h("div", "sessions-block")): HTMLElement {
  const n = lines.length;
  const height = `calc(${n ? Math.max(40, n * 20) : 0} * var(--u))`;
  if (block.style.height !== height) block.style.height = height;
  return keyedRows(block, lines.map((l) => lineRow(l, color)));
}

/** Session rows that each name their project, as Not in this workspace and a fleet's shown rows draw them. */
export function placedRows(block: HTMLElement, rows: [s: SessionInfo, color: string, where: string][]): HTMLElement {
  return keyedRows(block, rows.map(([s, color, where]) => [sessionKey(s, color, { where }), () => sessionRow(s, color, { where }), keepSession(s)]));
}

type RowOpts = { branch?: boolean; in?: Group; groupRail?: boolean; where?: string };

export function sessionRow(s: SessionInfo, color: string, o: RowOpts): HTMLElement {
  if (S.renamingSession === s.id) return renameRow(s, color);
  const cls = ["session-row"];
  if (o.in) cls.push("nested");
  if (o.groupRail) cls.push("group-rail");
  if (S.selection.includes(s.id)) cls.push("selected");
  const row = h("button", cls.join(" "));
  row.type = "button";
  row.dataset.session = s.id;
  if (o.in) row.dataset.inGroup = o.in.id;
  row.addEventListener("mousedown", (e) => {
    // dragSessions prevents the browser's default mousedown behavior to avoid
    // text selection, so focus the row explicitly before starting the drag.
    // Otherwise Delete stays in the terminal after a mouse selection.
    if (e.button === 0) row.focus();
    dragSessions(e, S.selection.length > 1 && S.selection.includes(s.id) ? [...S.selection] : [s.id]);
  });
  row.addEventListener("click", (e) => {
    // detail counts clicks across the re-render the first click causes.
    if (e.detail >= 2 && !e.shiftKey && !e.ctrlKey && !e.metaKey) startSessionRename(s.id);
    else if (e.shiftKey) selectRange(s.id);
    else if (e.ctrlKey || e.metaKey) toggleSelect(s.id);
    else {
      // A plain click opens and ends any selection. It never picks.
      clearSelection();
      revealSession(s.id);
      // The keys stay on the row, as on a picked group's, so Delete can end it.
      // Typing still reaches the terminal: see passToTerminal in keyboard.ts.
      requestAnimationFrame(() => keepRow(s.id, o.in?.id ?? null));
    }
  });
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    // The row outlives this render: the session as it is now.
    rowMenu(sessions.get(s.id) ?? s, e.clientX, e.clientY);
  });
  row.style.setProperty("--pc", color);
  if (o.in) row.style.setProperty("--gc", groupColor(o.in));
  row.append(agentIcon(s), h("span", "title", taskTitle(s)));
  if (o.branch) {
    const at = place(s);
    row.appendChild(h("span", "where mono", at ? branchName(at.worktree) : t("sidebar.otherWhere")));
  }
  if (o.where) row.appendChild(h("span", "where mono", o.where));
  const away = awayPlaces(s);
  if (away.length) {
    const names = away.map((a) => branchName(a.worktree)).join(", ");
    const tag = h("span", "where away mono", `→ ${names}`);
    row.appendChild(tag);
  }
  applyLook(row, s);
  return row;
}

/** What a session row shows of the session's state. A kept row takes a new one in place. */
export const look = (s: SessionInfo) => JSON.stringify([s.state, s.unread, s.exit_code, s.agent_exit, statusSig(s), taskTitle(s)]);

/**
 * Writes the session's state into its row: the state class, the label, the
 * mark at the row's end and the bold title. State changes many times a
 * minute while agents work; this keeps the row, its hover and its icons.
 * A mark of the same kind stays, so a working skiff keeps rocking.
 */
export function applyLook(row: HTMLElement, s: SessionInfo) {
  const l = look(s);
  if (row.dataset.look === l) return;
  const kept = row.dataset.look !== undefined;
  row.dataset.look = l;
  if (kept) {
    const title = row.querySelector<HTMLElement>(".title");
    const text = taskTitle(s);
    if (title && title.textContent !== text) title.textContent = text;
  }
  // State shows only when it matters: waiting stands out, working pulses, done fades.
  for (const st of STATES) if (st !== s.state) row.classList.remove(`st-${st}`);
  row.classList.add(`st-${s.state}`);
  row.setAttribute("aria-label", t("sidebar.rowLabel", { agent: agentName(s), title: taskTitle(s), state: stateLabel(s) }));
  const mark = stateIcon(s);
  row.classList.toggle("unread-row", s.state === "idle" && mark.classList.contains("unread"));
  // The mark is the row's last child. A kept mark takes the new note.
  if (!kept) row.appendChild(mark);
  else if (row.lastElementChild!.className !== mark.className) row.lastElementChild!.replaceWith(mark);
  else (row.lastElementChild as HTMLElement).title = mark.title;
}

const STATES: SessionInfo["state"][] = ["idle", "working", "waiting", "done"];

/**
 * A session from another worktree that works in this one. Not a second
 * session: a click goes to the real one. The keys skip it.
 */
function pointerRow(s: SessionInfo): HTMLElement {
  const at = place(s);
  const from = at ? branchName(at.worktree) : basename(s.cwd);
  const row = h("div", "session-row pointer-row");
  row.append(agentIcon(s), h("span", "title", taskTitle(s)), h("span", "where mono", t("sidebar.from", { name: from })));
  row.addEventListener("click", () => {
    clearSelection();
    revealSession(s.id);
    requestAnimationFrame(() => keepRow(s.id));
  });
  return row;
}

/** One color per group, by creation order, so a group keeps its color. */
const GROUP_COLORS = ["#9ec1ff", "#f28fd0", "#7ee0cb", "#e0c07e", "#ff9e7a", "#c3e88d"];

export const groupColor = (g: Group) => ink(GROUP_COLORS[Math.max(0, S.groups.indexOf(g)) % GROUP_COLORS.length]);

function groupRow(g: Group, hasPrevious = false, hasNext = false): HTMLElement {
  const ids = filledOf(g.layout).filter((id) => sessions.has(id));
  const row = h("div", "group-row" + (hasPrevious ? " has-previous" : "") + (hasNext ? " has-next" : "") );
  row.dataset.group = g.id;
  row.style.setProperty("--gc", groupColor(g));
  const lead = sessions.get(g.focus ?? ids[0]);
  row.style.setProperty("--pc", accent(lead ? place(lead)?.project : null));
  if (S.renaming?.id === g.id) {
    row.classList.add("renaming");
    const input = groupRenameInput(g);
    row.append(glyph(g.layout), input);
    return row;
  }
  const pick = h("button", "group-pick");
  pick.dataset.group = g.id;
  pick.type = "button";
  pick.addEventListener("mousedown", (e) => {
    // The drag stops the default focus; the row still takes the keys.
    if (e.button === 0) pick.focus();
    dragGroup(e, g);
  });
  pick.append(glyph(g.layout), h("span", "group-name", g.name));
  pick.addEventListener("click", (e) => {
    if (e.ctrlKey || e.metaKey) return toggleSelectGroup(g);
    // detail counts clicks across the re-render the first click causes.
    if (e.detail >= 2) startRename(g);
    else if (g.id !== S.activeGroup || g.id !== S.groupPicked) showGroup(g.id, true);
  });
  pick.addEventListener("dblclick", () => startRename(g));
  pick.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    groupMenu(g, e.clientX, e.clientY);
  });
  row.appendChild(pick);
  return row;
}

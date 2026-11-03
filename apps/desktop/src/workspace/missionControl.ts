/**
 * Mission control (#88): the Host's whole agent hierarchy as a graph on a
 * canvas, over the whole window. Admiral at the top, the fleet and project
 * Skippers under it, each project a framed cluster of session cards, Other
 * last. The strings come from the ledger: Admiral to a Skipper (a hand-off),
 * a Skipper to a pane (a task), Admiral to a pane (its own task). A plain
 * session has no string. A string takes its child's state color, and a
 * working string's dashes march toward the child. missionLayout.ts places it all.
 *
 * Running / All at the right of the search (#133): Running dims every card,
 * cluster and string that is not working or waiting. Nothing moves. The choice is stored in
 * `[appearance] mission_filter`, so the board opens with the last one, also
 * after a restart. With none stored: Running.
 * Ctrl+Shift+P searches the cards and dims the rest. The search stays until
 * Esc clears it, so a close and an open keep it.
 *
 * The layout (missionControl/grid.ts) comes from the data alone: each
 * cluster a tidy tree, the clusters in a fixed grid in the sidebar's order,
 * Admiral's trunk down the left. Nothing is dragged; the window size is never
 * an input. Drag or the wheel pans, Ctrl+wheel and pinch zoom at the pointer,
 * Home fits. A click on a cluster's head lights it. The panel's fold persists
 * on this computer, per Host.
 *
 * Admiral and each Skipper show as a terminal window with its face and its
 * state, task and counts (missionControl/terminal.ts, state.ts).
 *
 * The panel under the canvas shows the lit card: its last 12 screen lines,
 * read-only text from skiffd, its task, or a Skipper's hand-offs. One read a
 * second, for the lit card only. Ctrl+Alt+Shift+L folds it to a rail.
 *
 * The keys: Up the parent, Down the first child, Left and Right the siblings,
 * Enter goes to the lit session. A click goes
 * too. Ctrl+Shift+A or Esc closes. This file is the board. missionMotion.ts
 * opens and closes it.
 */

import { invoke } from "@tauri-apps/api/core";

import type { Project, SessionInfo } from "../platform/types";
import { agentIcon } from "../appearance/agentIcon";
import { stateIcon } from "../appearance/stateIcon";
import { childrenNote, stateLabel, statusNote } from "./programStatus";
import { DEFAULT_THEME, isLight } from "../appearance/colors";
import { themeIdFor, xtermTheme } from "../appearance/themes";
import { type Action, actionFor, keyLabel } from "../app/keys";
import { hostCan, hostInvoke } from "../app/hostCaps";
import { hostName, shownHost } from "../app/hosts";
import { settings } from "../app/panels";
import { openSkiffAgent, skiffAgentName, skiffAgentSession } from "../app/skiffAgent";
import { S, sessions } from "../app/state";
import { accent, place } from "../app/stateQueries";
import { ctxMenu } from "../ui/contextMenu";
import { h } from "../ui/dom";
import { type Entry, ledgerRows, ledgerTitle, needWord, taskState } from "./ledger";
import { fuzzy, taskTitle } from "./model";
import { closeMission, goToSession } from "./missionMotion";
import * as G from "./missionLayout";
import { worktreeLines } from "./sidebarLines";
import { projectBadge, treeSections } from "./tree";
import { t, tn, upper } from "../i18n";
import { matchText, sinceText } from "./missionControl/model";
import * as T from "./missionControl/terminal";
import { clearEdges, drawEdges, type Edge, mountEdges } from "./missionControl/edges";
import { placeGraph, route } from "./missionControl/grid";
import { clearConnectors, drawConnectors, type Link, mountConnectors } from "./missionControl/connectors";
import { indexLedger, type LedgerIndex, stateParts } from "./missionControl/state";

/** Rows of text in the panel's screen. */
const ROWS = 12;
/** How often the open board reads the graph and the lit screen again. */
const TICK = 1000;
/** The key of the Other cluster: the sessions in no project. */
const OTHER = "project:\u0000other";

const root = h("div", "mission");
root.id = "mission-control";
root.hidden = true;
root.tabIndex = -1;
root.setAttribute("role", "dialog");
root.setAttribute("aria-label", t("cmd.missionControl"));
document.body.appendChild(root);

const fault = h("div", "mission-fault");
fault.hidden = true;
const canvas = h("div", "mission-canvas");
const world = h("div", "mission-world");
const strings = document.createElementNS("http://www.w3.org/2000/svg", "svg");
strings.setAttribute("class", "mission-strings");
// One solid path per link, keyed, in three state layers (missionControl/connectors.ts).
mountConnectors(strings);
const items = h("div", "mission-items");
world.append(strings, T.faceLayer, items);
const ticks = h("div", "mission-ticks");
mountEdges(ticks);
const zoomBar = h("div", "mission-zoom");
const zoomLevel = h("span", "level");
const zoomBtn = (text: string, title: string, run: () => void) => {
  const b = h("button", "", text);
  b.type = "button";
  b.tabIndex = -1;
  b.title = title;
  b.addEventListener("pointerdown", (e) => e.stopPropagation());
  b.addEventListener("click", run);
  return b;
};
canvas.append(world, ticks, zoomBar);
const panel = h("div", "mission-panel");
root.append(fault, canvas, panel);
const missionState = {
  lit: null as string | null,
  timer: undefined as number | undefined,
  reading: false,
  paused: false,
  screenFor: null as string | null,
  view: { x: 0, y: 0, z: 1 } as G.View,
  panelOpen: true,
  keptFor: undefined as string | null | undefined,
  running: true,
  m: { g: { admiral: null, fleets: [], projects: [] }, sess: new Map(), projects: new Map(), members: new Map(), rows: new Map(), strs: [], found: null, idx: indexLedger([]) } as Model,
  placed: { cards: new Map(), clusters: new Map(), plans: [], rungs: new Map(), at: new Map(), trunk: null } as G.Placed,
  cards: new Map<string, HTMLElement>(),
  numbered: [] as string[],
  clusters: new Map<string, HTMLElement>(),
  plans: new Map<string, HTMLElement>(),
  drawnSig: "",
  pan: null as Pan | null,
  pointer: "",
};

const screen = h("pre", "mission-screen");
const appZoom = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--zoom")) || 1;

// ---- What persists on this computer, per Host ----

interface Kept {
  panel: boolean;
}
const keptKey = () => `skiff.mission.${shownHost() ?? "local"}`;

function loadKept() {
  const host = shownHost();
  if (missionState.keptFor === host) return;
  missionState.keptFor = host;
  let k: Partial<Kept> = {};
  try {
    const v: unknown = JSON.parse(localStorage.getItem(keptKey()) ?? "{}");
    if (v && typeof v === "object") k = v as Partial<Kept>;
  } catch {
    /* start as laid out */
  }
  missionState.panelOpen = k.panel !== false;
  // A record from before the fixed layout still holds old layout and fold state: written back, they go.
  if ("offs" in k || "order" in k || "folded" in k || "opened" in k) saveKept();
}

function saveKept() {
  try {
    localStorage.setItem(keptKey(), JSON.stringify({ panel: missionState.panelOpen } satisfies Kept));
  } catch {
    /* the panel state lasts for this run */
  }
}

// ---- The search and Running / All, in the app's top bar ----

/**
 * The search, in the app's top bar where the palette field sits; it shows
 * only while the board is open.
 */
const search = h("input", "mission-search");
search.type = "text";
search.autocomplete = "off";
search.spellcheck = false;
search.placeholder = t("mission.search");
search.setAttribute("aria-label", t("mission.search"));
const find = h("label", "tb-cell mission-find");
find.appendChild(search);
document.getElementById("open-switcher")?.after(find);
const scope = h("div", "mission-scope");
scope.setAttribute("role", "group");
scope.setAttribute("aria-label", t("mission.scopeAria"));
const allCount = h("span", "mission-scope-n");
const runningCount = h("span", "mission-scope-n");
const scopeAll = h("button", "mission-scope-btn");
scopeAll.append(t("mission.all"), allCount);
const scopeRunning = h("button", "mission-scope-btn");
scopeRunning.append(h("span", "mission-scope-dot"), t("mission.running"), runningCount);
for (const [b, on] of [[scopeRunning, true], [scopeAll, false]] as const) {
  b.type = "button";
  b.tabIndex = -1;
  // The keys stay where they are: on the board or in the search.
  b.addEventListener("mousedown", (e) => e.preventDefault());
  b.addEventListener("click", (e) => {
    // Inside the search's label: the click must not go on to the search.
    e.preventDefault();
    setRunning(on, true);
  });
  scope.appendChild(b);
}
find.appendChild(scope);

/** A session that works or waits. An idle session, a shell at its prompt and an exited one are not. */
const isRunning = (s: SessionInfo) => s.state === "working" || s.state === "waiting";

/** Shows Running or All. `store`: a change by the user, kept in the config. */
function setRunning(on: boolean, store = false) {
  if (store && on !== missionState.running) invoke("set_mission_filter", { running: on }).catch(console.error);
  missionState.running = on;
  scopeAll.setAttribute("aria-pressed", String(!on));
  scopeRunning.setAttribute("aria-pressed", String(on));
  if (!root.hidden) draw();
}
setRunning(true);

/** The stored filter, from `[appearance] mission_filter` when the app starts. Only `all` shows all. */
export function restoreRunning(filter: string | null | undefined) {
  setRunning(filter !== "all");
}

/** The Running key here: only the running sessions stand out, or all. */
export function toggleRunning() {
  setRunning(!missionState.running, true);
}

// ---- The model: the graph from the sessions, the projects and the ledger ----

/** A project's sessions in tree order, less its Skipper, which has a card of its own. */
function projectSessions(p: Project | null): SessionInfo[] {
  const seen = new Set<string>();
  const out: SessionInfo[] = [];
  for (const w of p ? p.worktrees : [null]) {
    for (const l of worktreeLines(w)) {
      if (l.pointer || l.group) continue;
      const s = l.session;
      if (!seen.has(s.id) && !s.coordinator && !s.skipper) {
        seen.add(s.id);
        out.push(s);
      }
    }
  }
  return out;
}

interface Str {
  from: string;
  /** The session the string goes to; its card is where it ends. */
  to: string;
  label: string;
}

interface Model {
  g: G.Graph;
  /** Every session on the board by id: the cards, the Skippers and Admiral. */
  sess: Map<string, SessionInfo>;
  /** Each cluster's project. Other has none. */
  projects: Map<string, Project | null>;
  /** Each project cluster's sessions. */
  members: Map<string, SessionInfo[]>;
  /** A session's ledger task, for its meta line and its string. */
  rows: Map<string, Entry>;
  strs: Str[];
  /** The sessions the search leaves. `null`: no search. */
  found: Set<string> | null;
  /** The open ledger rows by owner and pane, once per draw, for the windows and the panel. */
  idx: LedgerIndex;
}

/** The Skipper whose rows these are: Admiral for `grand`, else the session with that scope. */
function ownerOf(owner: string, admiral: SessionInfo | null): SessionInfo | null {
  if (owner === "grand") return admiral;
  return [...sessions.values()].find((s) => s.skipper === owner && s.state !== "done") ?? null;
}

function model(): Model {
  const admiral = skiffAgentSession();
  const sess = new Map<string, SessionInfo>();
  const projects = new Map<string, Project | null>();
  const members = new Map<string, SessionInfo[]>();
  const rows = new Map<string, Entry>();
  const ledger = hostCan("ledger") ? ledgerRows().filter((r) => r.bucket !== "finished") : [];
  for (const r of ledger) if (r.kind === "task" && r.pane_id) rows.set(r.pane_id, r);
  const q = search.value.trim();
  const found = q ? new Set<string>() : null;
  if (admiral) sess.set(admiral.id, admiral);

  const cluster = (p: Project | null, skipper: SessionInfo | null): G.GProject => {
    const key = p ? `project:${p.name}` : OTHER;
    // The cards in tree order: the board mirrors the sidebar.
    const list = projectSessions(p);
    projects.set(key, p);
    members.set(key, list);
    for (const s of list) {
      sess.set(s.id, s);
      if (found && fuzzy(q, matchText(s, p, rows.get(s.id))) >= 0) found.add(s.id);
    }
    if (skipper) sess.set(skipper.id, skipper);
    // A plan heads the cards of its tasks in this cluster.
    const plans: G.GPlan[] = [];
    for (const s of list) {
      const r = rows.get(s.id);
      const plan = r?.parent ? ledger.find((x) => x.uid === r.parent && x.kind === "plan") : null;
      if (!plan || plans.some((pl) => pl.uid === plan.uid)) continue;
      const cards = list.filter((t) => rows.get(t.id)?.parent === plan.uid);
      plans.push({ uid: plan.uid, title: ledgerTitle(plan), cards: cards.map((t) => t.id), done: cards.filter((t) => rows.get(t.id)!.state === "done").length });
    }
    return { key, name: p?.name ?? t("sidebar.other"), skipper: skipper?.id ?? null, cards: list.map((s) => s.id), plans };
  };

  const g: G.Graph = { admiral: admiral?.id ?? null, fleets: [], projects: [] };
  for (const sec of treeSections()) {
    if (sec.fleet) {
      const skipper = [...sessions.values()].find((s) => s.skipper === `fleet:${sec.fleet}` && s.state !== "done") ?? null;
      if (skipper) sess.set(skipper.id, skipper);
      g.fleets.push({ key: `fleet:${sec.fleet}`, name: sec.fleet, skipper: skipper?.id ?? null, projects: sec.projects.map((p) => cluster(p, [...sessions.values()].find((s) => s.skipper === p.name && s.state !== "done") ?? null)) });
    } else {
      for (const p of sec.projects) g.projects.push(cluster(p, [...sessions.values()].find((s) => s.skipper === p.name && s.state !== "done") ?? null));
    }
  }
  if (projectSessions(null).length) g.projects.push(cluster(null, null));
  if (found) for (const s of sess.values()) if (s.coordinator || !!s.skipper) if (fuzzy(q, `${cardName(s)} ${s.state}`) >= 0) found.add(s.id);

  // The strings. A hand-off ties Admiral to the Skipper it went to; a task ties its owner to its pane.
  const strs: Str[] = [];
  for (const r of ledger) {
    if (!r.pane_id || !sess.has(r.pane_id)) continue;
    const from = r.kind === "handoff" ? admiral : r.kind === "task" ? ownerOf(r.owner, admiral) : null;
    if (!from || !sess.has(from.id) || from.id === r.pane_id) continue;
    strs.push({ from: from.id, to: r.pane_id, label: r.kind === "handoff" ? ledgerTitle(r) : "" });
  }
  return { g, sess, projects, members, rows, strs, found, idx: indexLedger(ledger) };
}

// ---- Cards and clusters ----

/** A card's name: Admiral's, "Skipper · gesher" for a Skipper, else the session's title. */
const cardName = (s: SessionInfo) => (s.coordinator ? skiffAgentName() : taskTitle(s));
const dimmed = (s: SessionInfo) => (missionState.running && !isRunning(s)) || (!!missionState.m.found && !missionState.m.found.has(s.id));
const stateWord = (s: SessionInfo) => t(s.state === "working" ? "state.working" : s.state === "waiting" ? "state.waiting" : s.state === "done" ? "state.done" : "state.idle");

/** A session card's meta line: the task and its uid, a shell's program, or the state. */
function metaOf(s: SessionInfo): HTMLElement {
  const el = h("span", "m");
  // What the program reports: what it waits for, how its turn ended, its subtasks.
  const note = [statusNote(s), childrenNote(s)].filter(Boolean).join(" · ");
  const r = missionState.m.rows.get(s.id);
  if (r) {
    el.append(ledgerTitle(r), " ", h("span", "mono", r.uid));
    if (note) el.append(" · ", note);
    return el;
  }
  if (note) {
    el.textContent = note;
    return el;
  }
  if (s.role === "shell" && s.foreground_program && s.state === "working") el.textContent = s.foreground_program;
  else if (s.state === "done") el.textContent = s.exit_code ? t("mission.exitedWith", { code: s.exit_code }) : t("mission.exited");
  else if (s.state === "idle" && s.unread) el.textContent = t("mission.turnEnded");
  else if (s.state === "idle" && s.role === "shell") el.textContent = t("mission.atPrompt");
  else el.textContent = stateWord(s);
  return el;
}

/** A session card's inside: the head and the body. Admiral and the Skippers are windows (terminal.ts). */
function cardParts(s: SessionInfo, now: number): HTMLElement[] {
  const head = h("span", "head");
  head.appendChild(h("span", "at", sinceText(s, now)));
  const body = h("span", "body");
  const tx = h("span", "tx");
  tx.append(h("span", "t", cardName(s)), metaOf(s));
  body.append(stateIcon(s), agentIcon(s), tx);
  return [head, body];
}

/** A cluster's head: the badge, the name and the counts. */
function clusterHeadParts(key: string, fleet: boolean): HTMLElement[] {
  const list = fleet ? (missionState.m.g.fleets.find((f) => f.key === key)?.projects ?? []).flatMap((p) => missionState.m.members.get(p.key) ?? []) : (missionState.m.members.get(key) ?? []);
  const waits = list.filter((s) => s.state === "waiting").length;
  const works = list.filter((s) => s.state === "working").length;
  const roll = h("span", "mission-roll");
  if (waits) roll.appendChild(h("span", "w", tn("mission.wait", waits)));
  if (works) roll.appendChild(h("span", "g", tn("mission.working", works)));
  roll.appendChild(h("span", "", fleet ? tn("palette.sessions", list.length) : String(list.length)));
  if (fleet) return [h("span", "name", key.slice("fleet:".length)), roll];
  const p = missionState.m.projects.get(key) ?? null;
  return [p ? projectBadge(p) : h("span", "name", "…"), h("span", "name", p?.name ?? t("sidebar.other")), roll];
}

/** Markup to compare, less the busy icon's phase, which is new on each call. */
const strip = (html: string) => html.replace(/--(water|skiff|blink|face-turn)-delay:[^;"]*;?\s*/g, "");

/** Puts `parts` in `el` when they differ from what is there. The element stays, so its light does not flicker. */
function refill(el: HTMLElement, parts: HTMLElement[]) {
  const next = parts.map((p) => strip(p.outerHTML)).join("");
  if (el.dataset.html === next) return;
  el.dataset.html = next;
  el.replaceChildren(...parts);
}

function setClass(el: HTMLElement, cls: string, on: boolean) {
  el.classList.toggle(cls, on);
}

/** Draws the graph as the model has it now: cards, clusters, plans and strings, keyed so what stays stays. */
function draw() {
  const now = Date.now();
  missionState.drawnSig = boardSignature(now);
  missionState.m = model();
  missionState.placed = placeGraph(missionState.m.g);
  // The light: a card that left the board hands it to its parent, else the first node.
  if (missionState.lit && !missionState.placed.cards.has(missionState.lit) && !missionState.placed.clusters.has(missionState.lit)) missionState.lit = G.navParent(missionState.m.g, missionState.lit) ?? G.navFirst(missionState.m.g);
  if (!missionState.lit) missionState.lit = G.navFirst(missionState.m.g);

  missionState.numbered = G.skipperOrder(missionState.m.g).filter((id) => missionState.placed.cards.has(id)).slice(0, 9);
  const seen = new Set<string>();
  // Cards: sessions, Skippers and Admiral.
  const putCard = (id: string, r: G.Rect, parts: HTMLElement[] | G.Rung, cls: string, pc: string, dim: boolean) => {
    seen.add(id);
    let el = missionState.cards.get(id);
    if (!el) {
      el = h("div", "mission-card");
      el.dataset.id = id;
      missionState.cards.set(id, el);
      items.appendChild(el);
    }
    el.className = `mission-card ${cls}`;
    setClass(el, "lit", id === missionState.lit);
    setClass(el, "dim", dim);
    el.style.setProperty("--pc", pc);
    el.style.left = `${r.x}px`;
    el.style.top = `${r.y}px`;
    if (typeof parts !== "string") refill(el, parts);
    else Object.assign(el.style, { width: `${r.w}px`, height: `${r.h}px` });
  };
  for (const [id, r] of missionState.placed.cards) {
    const s = missionState.m.sess.get(id);
    if (!s) continue;
    const rung = missionState.placed.rungs.get(id);
    const cls = [rung ? `window ${rung === "admiral" ? "admiral" : "skipper"}` : "", s.state === "waiting" ? "waiting" : ""].join(" ");
    putCard(id, r, rung ?? cardParts(s, now), cls, rung ? "var(--accent)" : accent(place(s)?.project ?? null), dimmed(s));
    if (rung) {
      const w = T.windowOf(missionState.cards.get(id)!, rung, s);
      refill(w.head, T.windowHead(s, cardName(s), missionState.numbered.indexOf(id) + 1));
      refill(w.state, stateParts(s, now, missionState.m.idx));
    }
    const label2 = t("mission.tileLabel", { title: cardName(s), state: stateLabel(s) });
    if (missionState.cards.get(id)!.getAttribute("aria-label") !== label2) missionState.cards.get(id)!.setAttribute("aria-label", label2);
  }
  for (const [id, el] of missionState.cards) {
    if (seen.has(id)) continue;
    el.remove();
    missionState.cards.delete(id);
  }

  // Clusters: the frames.
  const seenClusters = new Set<string>();
  for (const [key, c] of missionState.placed.clusters) {
    seenClusters.add(key);
    let el = missionState.clusters.get(key);
    if (!el) {
      el = h("div", "mission-cluster");
      el.dataset.id = key;
      el.appendChild(h("div", "mission-cluster-head"));
      missionState.clusters.set(key, el);
      items.appendChild(el);
    }
    const list = c.fleet ? (missionState.m.g.fleets.find((f) => f.key === key)?.projects ?? []).flatMap((p) => missionState.m.members.get(p.key) ?? []) : (missionState.m.members.get(key) ?? []);
    const skipper = c.fleet ? missionState.m.g.fleets.find((f) => f.key === key)?.skipper : null;
    const run = list.some(isRunning) || (skipper ? isRunning(missionState.m.sess.get(skipper)!) : false);
    setClass(el, "fleet", c.fleet);
    setClass(el, "lit", key === missionState.lit);
    setClass(el, "dim", (missionState.running && !run) || (!!missionState.m.found && !list.some((s) => missionState.m.found!.has(s.id))));
    el.style.setProperty("--pc", c.fleet ? "var(--accent)" : accent(missionState.m.projects.get(key) ?? null));
    Object.assign(el.style, { left: `${c.x}px`, top: `${c.y}px`, width: `${c.w}px`, height: `${c.h}px` });
    refill(el.firstElementChild as HTMLElement, clusterHeadParts(key, c.fleet));
  }
  for (const [key, el] of missionState.clusters) {
    if (seenClusters.has(key)) continue;
    el.remove();
    missionState.clusters.delete(key);
  }

  // Plans: captions.
  const seenPlans = new Set<string>();
  for (const pl of missionState.placed.plans) {
    seenPlans.add(pl.key);
    let el = missionState.plans.get(pl.key);
    if (!el) {
      el = h("div", "mission-plan");
      missionState.plans.set(pl.key, el);
      items.appendChild(el);
    }
    const text = t("mission.plan", { title: pl.plan.title, done: pl.plan.done, total: pl.plan.cards.length });
    if (el.textContent !== text) el.textContent = text;
    setClass(el, "dim", missionState.running && !pl.plan.cards.some((id) => isRunning(missionState.m.sess.get(id)!)));
    Object.assign(el.style, { left: `${pl.x}px`, top: `${pl.y}px`, width: `${pl.w}px` });
  }
  for (const [key, el] of missionState.plans) {
    if (seenPlans.has(key)) continue;
    el.remove();
    missionState.plans.delete(key);
  }

  drawStrings();
  T.placeFaces(missionState.placed, missionState.m.sess, lookOf, (id) => `${id === missionState.lit ? "lit" : ""} ${missionState.cards.get(id)?.classList.contains("dim") ? "dim" : ""}`);
  counts();
  find.style.setProperty("--pc", missionState.cards.get(missionState.lit ?? "")?.style.getPropertyValue("--pc") ?? missionState.clusters.get(missionState.lit ?? "")?.style.getPropertyValue("--pc") ?? "");
  fault.hidden = hostCan("ledger");
  if (!fault.hidden) fault.textContent = t("mission.noLedgerFault", { host: hostName() });
  drawTicks();
  drawPanel();
}

/** The connectors: one per link, from its owner to the child. */
function drawStrings() {
  const links: Link[] = [];
  const done = new Set<string>();
  for (const st of missionState.m.strs) {
    const to = missionState.placed.cards.has(st.to) ? st.to : null;
    const d = to ? route(missionState.placed, st.from, to) : null;
    const key = `${st.from}>${to}`;
    if (!to || !d || done.has(key)) continue;
    done.add(key);
    const child = missionState.m.sess.get(st.to)!;
    const state = child.state === "waiting" ? "waiting" : child.state === "working" ? "working" : child.state === "done" ? "done" : "idle";
    links.push({ key, d, state, dim: dimmed(child) });
  }
  drawConnectors(links);
  // The box holds every connector, so WebKit repaints them as its own content.
  const b = G.bounds(missionState.placed);
  const w = String(Math.max(0, Math.ceil(b.x + b.w + 64)));
  const h = String(Math.max(0, Math.ceil(b.y + b.h + 64)));
  if (strings.getAttribute("width") !== w) strings.setAttribute("width", w);
  if (strings.getAttribute("height") !== h) strings.setAttribute("height", h);
}

/** The side a waiting window's face turns to: toward the task it waits on, else its first. */
function lookOf(id: string): 1 | -1 {
  const kids = missionState.m.strs.filter((x) => x.from === id).map((x) => missionState.m.sess.get(x.to)!);
  const k = rectOf((kids.find((c) => c.state === "waiting") ?? kids[0])?.id ?? null);
  const r = missionState.placed.cards.get(id)!;
  return k && k.x + k.w / 2 < r.x + r.w / 2 ? -1 : 1;
}

/** The top bar's counts: Running and All, of what the search leaves. */
function counts() {
  const all = [...missionState.m.sess.values()].filter((s) => !missionState.m.found || missionState.m.found.has(s.id));
  allCount.textContent = String(all.length);
  runningCount.textContent = String(all.filter(isRunning).length);
}

/** A waiting card out of view: a tick at the edge of the view, toward it. */
function drawTicks() {
  const vw = canvas.clientWidth;
  const vh = canvas.clientHeight;
  const size = 10 * appZoom();
  const edges: Edge[] = [];
  for (const [id, el] of missionState.cards) {
    if (!el.classList.contains("waiting")) continue;
    const r = missionState.placed.cards.get(id) ?? missionState.placed.clusters.get(id);
    if (!r) continue;
    edges.push({ id, at: G.edgeTick(missionState.view, r, vw, vh, size), title: t("mission.waits", { name: el.querySelector(".t, .win-head .title")?.textContent ?? "" }) });
  }
  drawEdges(edges);
}

// ---- The view ----

function applyView() {
  world.style.transform = `translate(${missionState.view.x}px, ${missionState.view.y}px) scale(${missionState.view.z})`;
  zoomLevel.textContent = `${Math.round((missionState.view.z / appZoom()) * 100)}%`;
  drawTicks();
}

function fit() {
  missionState.view = G.fitView(G.bounds(missionState.placed), canvas.clientWidth, canvas.clientHeight, 24 * appZoom());
  missionState.view.z = Math.min(missionState.view.z, appZoom());
  const b = G.bounds(missionState.placed);
  missionState.view.x = (canvas.clientWidth - b.w * missionState.view.z) / 2 - b.x * missionState.view.z;
  missionState.view.y = (canvas.clientHeight - b.h * missionState.view.z) / 2 - b.y * missionState.view.z;
  applyView();
}

/** The whole graph, then closer on the lit card when the fit is small. */
function openView() {
  fit();
  const z = appZoom();
  if (missionState.view.z >= 0.85 * z) return;
  const r = rectOf(missionState.lit);
  if (!r) return;
  missionState.view = G.centerOn({ ...missionState.view, z: 0.85 * z }, r, canvas.clientWidth, canvas.clientHeight);
  missionState.view.y = Math.min(24, missionState.view.y);
  applyView();
}

function zoomBy(f: number, px = canvas.clientWidth / 2, py = canvas.clientHeight / 2) {
  const z = appZoom();
  const next = G.zoomView({ ...missionState.view, z: missionState.view.z / z }, f, px, py);
  missionState.view = { ...next, z: next.z * z };
  applyView();
}

/** Where `id` is on the canvas: its card or cluster. */
function rectOf(id: string | null): G.Rect | null {
  if (!id) return null;
  const stand = missionState.placed.cards.has(id) || missionState.placed.clusters.has(id) ? id : null;
  return stand ? (missionState.placed.cards.get(stand) ?? missionState.placed.clusters.get(stand) ?? null) : null;
}

/** Brings `id` into view when it is out of it. */
function panTo(id: string | null) {
  const r = rectOf(id);
  if (!r || G.inView(missionState.view, r, canvas.clientWidth, canvas.clientHeight)) return;
  missionState.view = G.centerOn(missionState.view, r, canvas.clientWidth, canvas.clientHeight);
  applyView();
}

zoomBar.append(zoomBtn("−", t("mission.zoomOut", { key: "-" }), () => zoomBy(1 / 1.2)), zoomBtn("+", t("mission.zoomIn", { key: "=" }), () => zoomBy(1.2)), zoomBtn("⌂", t("mission.fitTip", { key: "Home" }), fit), zoomLevel);

// ---- The light, and the moves ----

/** `pan`: bring the card into view, for keys. The pointer is already on it. */
function light(id: string | null, pan = true) {
  if (!id) return;
  missionState.lit = id;
  for (const [k, el] of missionState.cards) el.classList.toggle("lit", k === id);
  for (const [k, el] of missionState.clusters) el.classList.toggle("lit", k === id);
  for (const el of T.faceLayer.children) el.classList.toggle("lit", (el as HTMLElement).dataset.id === id);
  find.style.setProperty("--pc", missionState.cards.get(id)?.style.getPropertyValue("--pc") ?? missionState.clusters.get(id)?.style.getPropertyValue("--pc") ?? "");
  if (pan) panTo(id);
  drawPanel();
  void read();
}

const isCluster = (id: string) => id.startsWith("project:") || id.startsWith("fleet:");

/** The next waiting card after the lit one, around the board. */
function nextWaitingCard(): string | null {
  const ids = [...missionState.cards].filter(([id, el]) => el.classList.contains("waiting") && !isCluster(id)).map(([id]) => id);
  if (!ids.length) return null;
  const i = ids.indexOf(missionState.lit ?? "");
  return ids[(i + 1) % ids.length];
}

/** The lit node's session: a card's own, a cluster's first. `null` with nothing to go to. */
function litSession(): string | null {
  if (!missionState.lit) return null;
  const id = isCluster(missionState.lit) ? (missionState.m.g.fleets.find((f) => f.key === missionState.lit)?.projects.flatMap((p) => p.cards)[0] ?? [...missionState.m.g.fleets.flatMap((f) => f.projects), ...missionState.m.g.projects].find((p) => p.key === missionState.lit)?.cards[0] ?? null) : missionState.lit;
  return id && missionState.m.sess.has(id) ? id : null;
}

/** To the lit node's session, maximized or not. */
function go(max = false) {
  const id = litSession();
  if (id) closeMission(id, max);
}

/** The board menu: the layout as a whole. */
function openMenu() {
  const at = missionState.cards.get(missionState.lit ?? "") ?? missionState.clusters.get(missionState.lit ?? "");
  const r = (at ?? canvas).getBoundingClientRect();
  const back = () => requestAnimationFrame(() => root.focus());
  ctxMenu.open(
    r.left,
    at ? r.bottom : r.top,
    upper(t("cmd.missionControl")),
    [
      { label: t("mission.fit"), hint: "Home", run: () => (fit(), back()) },
    ],
    back,
  );
}

// ---- The panel under the canvas ----

const FOLD_SVG = '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M1.5 3.5 5 7l3.5-3.5"/></svg>';

/** The session's terminal theme, as its pane shows it. */
function paint(pre: HTMLElement, s: SessionInfo) {
  const id = themeIdFor(s);
  const t = S.themes.find((x) => x.id === id) ?? S.themes.find((x) => x.id === DEFAULT_THEME);
  if (!t) return;
  pre.style.background = t.background;
  pre.style.color = xtermTheme(t).foreground;
  pre.style.colorScheme = isLight(t.background) ? "light" : "dark";
}

function togglePanel() {
  missionState.panelOpen = !missionState.panelOpen;
  saveKept();
  drawPanel();
  requestAnimationFrame(drawTicks);
}

const panelHead = h("div", "mission-panel-head");
const panelBody = h("div", "mission-panel-body");
const side = h("div", "mission-side");
panelBody.append(screen, side);
panel.append(panelHead, panelBody);
const foldBtn = h("button", "fold");
foldBtn.type = "button";
foldBtn.tabIndex = -1;
foldBtn.innerHTML = FOLD_SVG;
foldBtn.addEventListener("click", togglePanel);

/** The panel: its nodes stay; a part is replaced only when its text changes. Folded, only the head updates. */
function drawPanel() {
  panel.classList.toggle("folded", !missionState.panelOpen);
  foldBtn.title = t("mission.foldPanel", { key: keyLabel("ledger") });
  const sideHead = (text: string, mono = "") => {
    const el = h("div", "mission-side-head", text);
    el.appendChild(h("span", "mono", mono));
    return el;
  };
  const kv = (pairs: [string, string, string?][]) => {
    const el = h("div", "mission-kv");
    for (const [k, v, cls] of pairs) el.append(h("div", "", k), h("div", cls ?? "", v));
    return el;
  };
  const head = (parts: HTMLElement[]) => {
    refill(panelHead, parts);
    if (foldBtn.parentNode !== panelHead) panelHead.appendChild(foldBtn);
  };
  const s = missionState.lit ? missionState.m.sess.get(missionState.lit) : null;
  if (!s) {
    const list = missionState.lit ? (missionState.m.g.fleets.find((f) => f.key === missionState.lit)?.projects ?? []).flatMap((p) => missionState.m.members.get(p.key) ?? []).concat(missionState.m.members.get(missionState.lit) ?? []) : [];
    const name = missionState.lit?.startsWith("fleet:") ? missionState.lit.slice("fleet:".length) : (missionState.m.projects.get(missionState.lit ?? "")?.name ?? (missionState.lit === OTHER ? t("sidebar.other") : ""));
    head([h("span", "title", name), h("span", "state", `${tn("mission.waiting", list.filter((x) => x.state === "waiting").length)} · ${tn("mission.working", list.filter((x) => x.state === "working").length)} · ${tn("palette.sessions", list.length)}`)]);
    if (missionState.screenFor !== null) {
      screen.textContent = "";
      screen.classList.add("note");
      screen.removeAttribute("style");
      missionState.screenFor = null;
    }
    if (missionState.panelOpen) refill(side, [h("div", "mission-task none", t("mission.clusterNoPane"))]);
    return;
  }
  const p = place(s)?.project ?? null;
  const title = s.coordinator || !!s.skipper ? cardName(s) : p ? `${p.name} · ${cardName(s)}` : cardName(s);
  head([stateIcon(s), h("span", "title", title), h("span", `state${s.state === "waiting" ? " w" : ""}`, [stateWord(s), statusNote(s), childrenNote(s), sinceText(s, Date.now())].filter(Boolean).join(" · "))]);
  if (missionState.screenFor !== s.id) {
    missionState.screenFor = s.id;
    screen.textContent = "";
    screen.classList.remove("note");
  }
  if (!missionState.panelOpen) return;
  paint(screen, s);
  const parts: HTMLElement[] = [];
  const r = missionState.m.rows.get(s.id);
  if (s.coordinator || !!s.skipper) {
    const hos = s.coordinator ? missionState.m.idx.open : (missionState.m.idx.paneHandoffs.get(s.id) ?? []);
    parts.push(sideHead(t("mission.handoffsHead"), String(hos.length)));
    if (!hostCan("ledger")) parts.push(h("div", "mission-task none", t("mission.noLedger")));
    else if (!hos.length) parts.push(h("div", "mission-task none", t("mission.noneDot")));
    else parts.push(kv(hos.map((x) => [x.uid, `${ledgerTitle(x)} · ${s.coordinator ? (x.pane_id && sessions.get(x.pane_id) ? cardName(sessions.get(x.pane_id)!) : x.project) : t("mission.from", { name: skiffAgentName() })}`])));
  } else {
    parts.push(sideHead(t("mission.taskHead"), r ? r.uid : ""));
    if (!hostCan("ledger")) parts.push(h("div", "mission-task none", t("mission.noLedger")));
    else if (!r) parts.push(h("div", "mission-task none", t("mission.noTask")));
    else {
      const st = taskState(r);
      const owner = ownerOf(r.owner, skiffAgentSession());
      parts.push(h("div", "mission-task", r.task), kv([
        [t("mission.owner"), owner ? cardName(owner) : r.owner === "grand" ? skiffAgentName() : r.owner],
        [t("mission.state"), needWord(st) ?? st, needWord(st) ? "w" : ""],
        [t("mission.result"), t(r.state === "done" ? (r.result_exists ? "mission.resultReady" : r.flushed_at ? "mission.resultFlushed" : "mission.resultNone") : "mission.resultNotYet"), "mono"],
      ]));
    }
  }
  refill(side, parts);
}

/** Reads the lit card's screen. One read a second, for that card only. */
async function read() {
  const id = missionState.screenFor;
  if (!id || missionState.reading || !missionState.panelOpen || missionState.paused) return;
  missionState.reading = true;
  try {
    const lines = await hostInvoke<string[]>("session_screen", "session_screen", { session: id, rows: ROWS });
    if (missionState.paused || missionState.screenFor !== id) return;
    const text = lines.join("\n");
    if (screen.textContent !== text) screen.textContent = text;
    screen.classList.remove("note");
  } catch (e) {
    if (missionState.paused || missionState.screenFor !== id) return;
    // An older skiffd does not know the request.
    screen.textContent = /unknown variant|bad request/.test(String(e)) ? t("mission.updateDaemon") : String(e);
    screen.classList.add("note");
  } finally {
    missionState.reading = false;
  }
}

/** What the board shows, as one string: the sessions, the ledger, the tree, the filter, the search and the time text. */
function boardSignature(now: number): string {
  const secs = treeSections().map((sec) => [sec.fleet ?? "", sec.projects.map((p) => p.name)]);
  const ledger = hostCan("ledger") ? ledgerRows() : [];
  const since = [...sessions.values()].map((s) => sinceText(s, now));
  return JSON.stringify([[...sessions.values()], ledger, secs, since, missionState.running, search.value, hostCan("ledger"), hostName(), missionState.view, missionState.lit]);
}

function tick() {
  if (missionState.paused) return;
  // A pan draws its own frames. The model and layout rebuild only when an input or the time text changed.
  if (!missionState.pan?.moved) {
    const sig = boardSignature(Date.now());
    if (sig !== missionState.drawnSig) draw();
  }
  void read();
}

// ---- The pointer: pan and click ----

interface Pan {
  x: number;
  y: number;
  /** The card or face under the pointer, for the click. */
  id: string | null;
  /** The cluster whose head is under the pointer. */
  head: string | null;
  moved: boolean;
  vx: number;
  vy: number;
}

canvas.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  const t = e.target as Element;
  const tick = t.closest<HTMLElement>(".mission-tick");
  if (tick?.dataset.id) {
    light(tick.dataset.id);
    return;
  }
  // A face is its window.
  const id = t.closest<HTMLElement>(".mission-card, .mission-face")?.dataset.id ?? null;
  const head = t.closest(".mission-cluster-head")?.closest<HTMLElement>(".mission-cluster")?.dataset.id ?? null;
  missionState.pan = { x: e.clientX, y: e.clientY, id, head, moved: false, vx: missionState.view.x, vy: missionState.view.y };
  canvas.setPointerCapture(e.pointerId);
  canvas.classList.add("panning");
  e.preventDefault();
});
canvas.addEventListener("pointermove", (e) => {
  if (!missionState.pan) return;
  const dx = e.clientX - missionState.pan.x;
  const dy = e.clientY - missionState.pan.y;
  if (!missionState.pan.moved && Math.hypot(dx, dy) < 4) return;
  missionState.pan.moved = true;
  missionState.view.x = missionState.pan.vx + dx;
  missionState.view.y = missionState.pan.vy + dy;
  applyView();
});
const endPan = (e: PointerEvent) => {
  if (!missionState.pan) return;
  canvas.classList.remove("panning");
  const d = missionState.pan;
  missionState.pan = null;
  if (d.moved || e.type === "pointercancel") return;
  root.focus();
  if (d.head) return light(d.head, false);
  if (!d.id) return;
  light(d.id, false);
  go();
};
canvas.addEventListener("pointerup", endPan);
canvas.addEventListener("pointercancel", endPan);
// The wheel pans. With Ctrl (or a pinch), it zooms at the pointer. Shift+wheel pans sideways.
canvas.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    const r = canvas.getBoundingClientRect();
    if (e.ctrlKey || e.metaKey) zoomBy(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top);
    else {
      missionState.view.x -= e.shiftKey ? e.deltaY : e.deltaX;
      missionState.view.y -= e.shiftKey ? 0 : e.deltaY;
      applyView();
    }
  },
  { passive: false },
);
canvas.addEventListener("mousemove", (e) => {
  if (missionState.pan) return;
  const at = `${e.clientX},${e.clientY}`;
  if (at === missionState.pointer) return;
  missionState.pointer = at;
  const el = (e.target as Element).closest<HTMLElement>(".mission-card, .mission-face");
  if (el?.dataset.id && el.dataset.id !== missionState.lit) light(el.dataset.id, false);
});

// ---- The keys ----

export function missionOpen(): boolean {
  return !root.hidden;
}

/** A session's card, for the fly of the focus frame. */
function tileOf(id: string | null): HTMLElement | null {
  return id && missionState.placed.cards.has(id) ? (missionState.cards.get(id) ?? null) : null;
}

/** Runs a board command. `false`: the board has no such command. */
function run(cmd: G.BoardCommand | null): boolean {
  if (!cmd) return false;
  switch (cmd) {
    case "parent": light(missionState.lit ? G.navParent(missionState.m.g, missionState.lit) : null); break;
    case "child": light(missionState.lit ? (G.navChildren(missionState.m.g, missionState.lit)[0] ?? null) : null); break;
    case "prev": light(missionState.lit ? G.navSibling(missionState.m.g, missionState.lit, -1) : null); break;
    case "next": light(missionState.lit ? G.navSibling(missionState.m.g, missionState.lit, 1) : null); break;
    case "go": go(); break;
    case "go-max": go(true); break;
    case "fit": fit(); break;
    case "zoom-in": zoomBy(1.2); break;
    case "zoom-out": zoomBy(1 / 1.2); break;
    case "close": closeMission(); break;
    case "clear-search":
      search.value = "";
      draw();
      break;
    case "search": toggleSearch(); break;
    case "running": toggleRunning(); break;
    case "next-waiting": light(nextWaitingCard()); break;
    case "panel": togglePanel(); break;
    case "menu": openMenu(); break;
    case "settings":
      closeMission();
      void settings.open();
      break;
    case "skipper": {
      const s = skiffAgentSession();
      if (s) goToSession(s.id);
      else {
        closeMission();
        openSkiffAgent();
      }
      break;
    }
  }
  return true;
}

/** The action keys with a meaning on the open board. The menu bar sends its accelerators here through `runAction()`. */
const BOARD_ACTIONS = new Set<Action>(["session-next", "session-prev", "list-project", "list-back", "next-waiting", "ledger", "project-menu"]);

/** An action key on the open board: the moves, the next waiting card, the panel and the menu. */
export function missionKey(a: Action | null): boolean {
  if (!a || !missionOpen() || !BOARD_ACTIONS.has(a)) return false;
  return run(G.boardCommand("", a, false, false, { ctrl: false, shift: false, alt: false, meta: false }));
}

/** Ctrl+Shift+1..9 here: the Skipper card with that number, as a click on it. */
export function pickSkipper(n: number) {
  const id = missionState.numbered[n - 1];
  if (!id) return;
  light(id, false);
  go();
}

/** Ctrl+Shift+P here: into the search, or back out to the board. The search stays. */
export function toggleSearch() {
  if (document.activeElement === search) root.focus();
  else {
    search.focus();
    search.select();
  }
}

search.addEventListener("input", () => draw());
/** The board's keys, on the board and in its search. */
function onKey(e: KeyboardEvent) {
  const a = actionFor(e);
  if (a === "quit") return;
  const cmd = G.boardCommand(e.key, a, e.target === search, !!search.value, { ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey, meta: e.metaKey });
  if (!run(cmd)) return;
  e.preventDefault();
  e.stopPropagation();
}
root.addEventListener("keydown", onKey);
find.addEventListener("keydown", onKey);

const topbar = () => document.getElementById("topbar");

/**
 * What the motion code (missionMotion.ts) drives. The board owns what shows;
 * the motion owns when and how it comes and goes.
 */
export const board = {
  root,
  lit: litSession,
  tile: tileOf,
  /** Builds the board lit on `focus` and reads its screen once. Shows nothing. */
  async prepare(focus: string | null) {
    missionState.paused = false;
    loadKept();
    search.title = t("mission.searchTip", { key: keyLabel("palette") });
    scope.title = t("mission.scopeTip", { key: keyLabel("focus-mode") });
    missionState.lit = focus;
    draw();
    await read();
  },
  /** With the root shown: fit the view, bring the lit card in, and keep the graph fresh. */
  start() {
    missionState.paused = false;
    // Focus mode and maximize leave the top bar inert. The board shows it, and its search is in it.
    topbar()?.toggleAttribute("inert", false);
    openView();
    light(missionState.lit, false);
    window.clearInterval(missionState.timer);
    missionState.timer = window.setInterval(tick, TICK);
  },
  /** No draw or read from here on: an animation is about to measure. */
  stop() {
    missionState.paused = true;
    window.clearInterval(missionState.timer);
  },
  /** Empty and hidden. */
  clear() {
    window.clearInterval(missionState.timer);
    topbar()?.toggleAttribute("inert", document.body.classList.contains("focus-mode"));
    root.hidden = true;
    missionState.pan = null;
    canvas.classList.remove("panning");
    missionState.screenFor = null;
    for (const el of [...missionState.cards.values(), ...missionState.clusters.values(), ...missionState.plans.values()]) el.remove();
    missionState.cards.clear();
    missionState.clusters.clear();
    missionState.plans.clear();
    clearConnectors();
    clearEdges();
    T.clearFaces();
  },
};

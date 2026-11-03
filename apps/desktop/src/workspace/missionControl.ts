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
import { keyLabel } from "../app/keys";
import { hostCan } from "../app/hostCaps";
import { hostName, shownHost } from "../app/hosts";
import { skiffAgentName, skiffAgentSession } from "../app/skiffAgent";
import { sessions } from "../app/state";
import { h } from "../ui/dom";
import { type Entry, ledgerRows, ledgerTitle } from "./ledger";
import { fuzzy, taskTitle } from "./model";
import * as G from "./missionLayout";
import { worktreeLines } from "./sidebarLines";
import { treeSections } from "./tree";
import { t } from "../i18n";
import { matchText, sinceText } from "./missionControl/model";
import * as T from "./missionControl/terminal";
import { clearEdges, drawEdges, type Edge, mountEdges } from "./missionControl/edges";
import { clearConnectors, mountConnectors } from "./missionControl/connectors";
import { indexLedger, type LedgerIndex } from "./missionControl/state";
import { createPersistence } from "./missionControl/persistence";
import { createRenderer } from "./missionControl/render";
import { createPanel } from "./missionControl/panel";
import { attachPointer, createNavigation, createKeyboard, type Pan } from "./missionControl/interact";

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

export type BoardState = typeof missionState;

const screen = h("pre", "mission-screen");
const appZoom = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--zoom")) || 1;

// ---- What persists on this computer, per Host ----

const { loadKept, saveKept } = createPersistence(missionState, () => shownHost());

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

const { draw } = createRenderer(missionState, { items, strings, find, fault }, {
  model, boardSignature, isRunning, dimmed, stateWord, cardName, lookOf, counts, drawTicks,
  drawPanel: () => drawPanel(),
});

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

const { applyView, fit, openView, zoomBy, rectOf, light, nextWaitingCard, litSession, go, openMenu } = createNavigation(missionState, { canvas, world, zoomLevel, zoomBar, find, root }, {
  appZoom, zoomBtn, drawTicks, drawPanel: () => drawPanel(), read: () => read(),
});

// ---- The panel under the canvas ----

const { drawPanel, read, togglePanel } = createPanel(missionState, panel, screen, { saveKept, drawTicks, cardName, stateWord, ownerOf });

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

attachPointer(missionState, canvas, root, { light, go, applyView, zoomBy });

// ---- The keys ----

export function missionOpen(): boolean {
  return !root.hidden;
}

/** A session's card, for the fly of the focus frame. */
function tileOf(id: string | null): HTMLElement | null {
  return id && missionState.placed.cards.has(id) ? (missionState.cards.get(id) ?? null) : null;
}

export const { missionKey, pickSkipper, toggleSearch } = createKeyboard(missionState, { root, find, search }, {
  missionOpen, light, go, fit, zoomBy, draw, toggleRunning, nextWaitingCard, togglePanel, openMenu,
});

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

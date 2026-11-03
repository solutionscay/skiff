/**
 * Mission control's graph, the pure part: its types, the windows' sizes, the
 * keys' moves through the graph, the key map and the view math. No DOM, no
 * state, no imports, so tests run it as it is. The layout and the connectors
 * are in missionControl/grid.ts. All lengths are in unzoomed px: the board
 * scales the whole world by the app zoom.
 */

export const CARD_W = 224;
/** A 20px head, a 40px body and the 1px border above and below. */
export const CARD_H = 62;
export const PLAN_H = 20;

/**
 * Admiral and each Skipper show as a terminal window: a 24px pane header,
 * then a body split by a 1px rule into the face's column and the state's
 * column. The face sets the rung, in even steps of 16: Admiral 88, a fleet
 * Skipper 72, a project Skipper 56. The face's column is the face and 8px
 * each side less the rule, the state's column twice the face, and the 1px
 * frame each side: 3 × face + 17 wide. The header, the face and 8px above
 * and below it: face + 40 high.
 */
export type Rung = "admiral" | "fleet" | "project";
const win = (face: number) => ({ w: 3 * face + 17, h: face + 40, face });
export const WINDOW: Record<Rung, { w: number; h: number; face: number }> = { admiral: win(88), fleet: win(72), project: win(56) };
/** The window's header, with the 1px frame above it. */
export const WIN_HEAD = 24;

export interface GPlan {
  uid: string;
  title: string;
  /** Its cards, in cluster order. */
  cards: string[];
  done: number;
}
/** A project cluster, or Other. `skipper`: its project Skipper's session id. */
export interface GProject {
  /** `project:<name>`. */
  key: string;
  name: string;
  skipper: string | null;
  cards: string[];
  plans: GPlan[];
  folded: boolean;
}
export interface GFleet {
  /** `fleet:<name>`. */
  key: string;
  name: string;
  skipper: string | null;
  projects: GProject[];
}
export interface Graph {
  /** The Skiff agent's session id. */
  admiral: string | null;
  fleets: GFleet[];
  /** The projects in no fleet, then Other when it has sessions. */
  projects: GProject[];
}
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Cluster extends Rect {
  fleet: boolean;
  folded: boolean;
}
export interface PlanBox extends Rect {
  key: string;
  plan: GPlan;
}
/**
 * How a connector reaches a node: its row's bus, its cluster's rail, and the
 * Skippers whose rail it hangs on. `head`: a cluster's top window, which the
 * bus reaches from above.
 */
export interface At {
  bus: number;
  rail: number;
  owners: string[];
  head?: boolean;
}
export interface Placed {
  /** Every card by id: sessions, Skippers and Admiral. A folded cluster's card is in `clusters`. */
  cards: Map<string, Rect>;
  clusters: Map<string, Cluster>;
  plans: PlanBox[];
  /** The rung of each window: Admiral and the Skippers. A card not here is a session card. */
  rungs: Map<string, Rung>;
  /** How connectors reach each card, window and folded cluster. */
  at: Map<string, At>;
  /** Admiral's trunk: the x its connectors come down. `null` with no Admiral. */
  trunk: number | null;
}

const allProjects = (g: Graph) => [...g.fleets.flatMap((f) => f.projects), ...g.projects];

/** The box around everything placed. Empty: a zero box at the origin. */
export function bounds(p: Placed): Rect {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const r of [...p.cards.values(), ...p.clusters.values()]) {
    x0 = Math.min(x0, r.x);
    y0 = Math.min(y0, r.y);
    x1 = Math.max(x1, r.x + r.w);
    y1 = Math.max(y1, r.y + r.h);
  }
  return x0 === Infinity ? { x: 0, y: 0, w: 0, h: 0 } : { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Where a window's face sits: in the face's column, 8px in from the frame's outer edge and under the header. */
export function faceBox(r: Rect, rung: Rung): Rect {
  const f = WINDOW[rung].face;
  return { x: r.x + 8, y: r.y + WIN_HEAD + 8, w: f, h: f };
}

// ---- The view: pan and zoom ----

export interface View {
  x: number;
  y: number;
  z: number;
}
export const ZOOM_MIN = 0.2;
export const ZOOM_MAX = 2;

/** The whole box in the viewport, at 100% at most. */
export function fitView(b: Rect, vw: number, vh: number, pad = 24): View {
  const z = b.w && b.h ? Math.min(1, (vw - pad * 2) / b.w, (vh - pad * 2) / b.h) : 1;
  return { z, x: (vw - b.w * z) / 2 - b.x * z, y: (vh - b.h * z) / 2 - b.y * z };
}

/** Zooms by `f` around the viewport point `px, py`. */
export function zoomView(v: View, f: number, px: number, py: number): View {
  const z = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, v.z * f));
  const k = z / v.z;
  return { z, x: px - (px - v.x) * k, y: py - (py - v.y) * k };
}

/** `r` in the middle of the viewport, at the view's zoom. */
export function centerOn(v: View, r: Rect, vw: number, vh: number): View {
  return { z: v.z, x: vw / 2 - (r.x + r.w / 2) * v.z, y: vh / 2 - (r.y + r.h / 2) * v.z };
}

/** The whole of `r` shows, `margin` px inside the viewport. */
export function inView(v: View, r: Rect, vw: number, vh: number, margin = 16): boolean {
  const sx = r.x * v.z + v.x;
  const sy = r.y * v.z + v.y;
  return sx >= margin && sy >= margin && sx + r.w * v.z <= vw - margin && sy + r.h * v.z <= vh - margin;
}

/** Where a tick for an off-screen `r` sits: at the edge of the viewport, toward it. `null` when `r` shows at all. */
export function edgeTick(v: View, r: Rect, vw: number, vh: number, size = 10): { x: number; y: number } | null {
  const sx = r.x * v.z + v.x;
  const sy = r.y * v.z + v.y;
  const sw = r.w * v.z;
  const sh = r.h * v.z;
  if (sx + sw > 0 && sx < vw && sy + sh > 0 && sy < vh) return null;
  return { x: Math.max(0, Math.min(vw - size, sx + sw / 2 - size / 2)), y: Math.max(0, Math.min(vh - size, sy + sh / 2 - size / 2)) };
}

// ---- The keys' moves through the graph ----

/** The node that stands for a fleet in the graph: its Skipper's card, else its frame. */
const fleetNode = (f: GFleet) => f.skipper ?? f.key;
const projectNode = (p: GProject) => p.skipper ?? p.key;

/** The node above `id`: a card's cluster, a cluster's Skipper or fleet, a Skipper's Admiral. `null` at the top. */
export function navParent(g: Graph, id: string): string | null {
  if (id === g.admiral) return null;
  for (const f of g.fleets) {
    if (id === f.skipper || id === f.key) return g.admiral;
    for (const p of f.projects) {
      if (id === p.skipper) return fleetNode(f);
      if (id === p.key) return p.skipper ?? fleetNode(f);
      if (p.cards.includes(id)) return p.key;
    }
  }
  for (const p of g.projects) {
    if (id === p.skipper) return g.admiral;
    if (id === p.key) return p.skipper ?? g.admiral;
    if (p.cards.includes(id)) return p.key;
  }
  return null;
}

/** The nodes under `id`, left to right. A folded cluster has none. */
export function navChildren(g: Graph, id: string): string[] {
  if (id === g.admiral) return [...g.fleets.map(fleetNode), ...g.projects.map(projectNode)];
  for (const f of g.fleets) {
    if (id === fleetNode(f)) return f.projects.map(projectNode);
    for (const p of f.projects) if (id === p.skipper) return [p.key];
  }
  for (const p of g.projects) if (id === p.skipper) return [p.key];
  const p = allProjects(g).find((p) => p.key === id);
  return p && !p.folded ? p.cards : [];
}

/** The next (`dir` 1) or previous node beside `id`, under the same parent. The ends stay. */
export function navSibling(g: Graph, id: string, dir: 1 | -1): string | null {
  const parent = navParent(g, id);
  const list = parent ? navChildren(g, parent) : [...(g.admiral ? [g.admiral] : []), ...(g.admiral ? [] : [...g.fleets.map(fleetNode), ...g.projects.map(projectNode)])];
  const i = list.indexOf(id);
  if (i < 0) return null;
  return list[Math.max(0, Math.min(list.length - 1, i + dir))] ?? null;
}

/**
 * The fleet and project Skippers in tree order, as the Skippers list numbers them: the
 * projects' in no fleet first, then each fleet's, then its projects'. Not Admiral. The
 * board shows the fleets first, but one number must pick one Skipper in both places.
 */
export function skipperOrder(g: Graph): string[] {
  const out: string[] = [];
  for (const p of g.projects) if (p.skipper) out.push(p.skipper);
  for (const f of g.fleets) {
    if (f.skipper) out.push(f.skipper);
    for (const p of f.projects) if (p.skipper) out.push(p.skipper);
  }
  return out;
}

/** The first node: Admiral, else the first block. */
export function navFirst(g: Graph): string | null {
  return g.admiral ?? g.fleets.map(fleetNode)[0] ?? g.projects.map(projectNode)[0] ?? null;
}

/** The card that stands for `id` on the canvas: itself, or its folded cluster. `null` when neither shows. */
export function standIn(g: Graph, p: Placed, id: string): string | null {
  if (p.cards.has(id)) return id;
  const pr = allProjects(g).find((x) => x.cards.includes(id));
  return pr && pr.folded && p.clusters.has(pr.key) ? pr.key : null;
}

// ---- Keys ----

export type BoardCommand =
  | "parent"
  | "child"
  | "prev"
  | "next"
  | "fold"
  | "go"
  | "go-max"
  | "fit"
  | "zoom-in"
  | "zoom-out"
  | "close"
  | "clear-search"
  | "search"
  | "running"
  | "next-waiting"
  | "panel"
  | "menu"
  | "settings"
  | "skipper";

const ACTIONS: Record<string, BoardCommand> = {
  palette: "search",
  "mission-control": "close",
  maximize: "go-max",
  "focus-mode": "running",
  "next-waiting": "next-waiting",
  ledger: "panel",
  "project-menu": "menu",
  settings: "settings",
  "skiff-agent": "skipper",
  "session-next": "child",
  "session-prev": "parent",
  "list-project": "prev",
  "list-back": "next",
};

/**
 * What a key does on the board. `action`: the app action the key maps to,
 * when it maps to one. `inSearch`: the keys are in the search field, where
 * Left, Right, Home, End and Space move the caret. `searchText`: the search
 * has text, so the first Esc clears it. `null`: not the board's key.
 */
export function boardCommand(key: string, action: string | null, inSearch: boolean, searchText: boolean, mods: { ctrl: boolean; shift: boolean; alt: boolean; meta: boolean }): BoardCommand | null {
  if (action && ACTIONS[action]) return ACTIONS[action];
  if (key === "Escape") return searchText ? "clear-search" : "close";
  if (key === "Enter") return "go";
  if (mods.ctrl || mods.shift || mods.alt || mods.meta) return null;
  if (key === "ArrowDown") return "child";
  if (key === "ArrowUp") return "parent";
  if (inSearch) return null;
  if (key === "ArrowRight") return "next";
  if (key === "ArrowLeft") return "prev";
  if (key === " ") return "fold";
  if (key === "Home") return "fit";
  if (key === "=" || key === "+") return "zoom-in";
  if (key === "-") return "zoom-out";
  return null;
}

/** "4m", "2h", "3d": how long ago `at` was, from `now`. Under a minute: "now". */
export function ago(at: number, now: number): string {
  const m = Math.floor(Math.max(0, now - at) / 60000);
  if (m < 1) return "now";
  if (m < 60) return `${m}m`;
  if (m < 1440) return `${Math.round(m / 60)}h`;
  return `${Math.round(m / 1440)}d`;
}

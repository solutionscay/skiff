/**
 * Mission control's graph, the pure part: its types, the windows' sizes, the
 * keys' moves on the board, the key map and the view math. No DOM, no
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
const WIN_HEAD = 24;

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
}
interface GFleet {
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
interface Cluster extends Rect {
  fleet: boolean;
}
interface PlanBox extends Rect {
  key: string;
  plan: GPlan;
}
/**
 * How a connector reaches a node: its row's bus, its cluster's rail (the gutter when the cluster has a Skipper), and the
 * Skippers whose rail it hangs on. `head`: a cluster's top window, which the
 * bus reaches from above.
 */
export interface At {
  bus: number;
  rail: number;
  owners: string[];
  head?: boolean;
  /** Its top cluster's lanes: the gutter inside the frame's left edge, the gap left of the frame, the frame's foot. */
  cell?: Lanes;
}
export interface Lanes {
  gutter: number;
  lane: number;
  foot: number;
}
export interface Placed {
  /** Every card by id: sessions, Skippers and Admiral. */
  cards: Map<string, Rect>;
  clusters: Map<string, Cluster>;
  plans: PlanBox[];
  /** The rung of each window: Admiral and the Skippers. A card not here is a session card. */
  rungs: Map<string, Rung>;
  /** How connectors reach each card and window. */
  at: Map<string, At>;
  /** Admiral's trunk: the x its connectors come down. `null` with no Admiral. */
  trunk: number | null;
}

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

// ---- The keys' moves on the board ----

export type Dir = "up" | "down" | "left" | "right";
const mid = (r: Rect) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 });

/**
 * The card an arrow lights: the nearest card in `dir` on the board. A card
 * counts when its near edge is past the middle of the lit card. Its score is
 * the gap along `dir` plus 3 × the gap across it. The gap across is measured
 * from `at`, the line that a run of moves keeps: the column for Up/Down, the
 * row for Left/Right. A card on that line has no gap across. `null`: no card
 * that way, so the light stays.
 */
export function navMove(cards: Map<string, Rect>, from: string, dir: Dir, at: { x: number; y: number }): string | null {
  const a = cards.get(from);
  if (!a) return null;
  const v = dir === "up" || dir === "down";
  const s = dir === "down" || dir === "right" ? 1 : -1;
  const lo = (r: Rect) => (v ? r.y : r.x);
  const hi = (r: Rect) => (v ? r.y + r.h : r.x + r.w);
  const middle = v ? mid(a).y : mid(a).x;
  const line = v ? at.x : at.y;
  let best: string | null = null;
  let score = Infinity;
  for (const [id, b] of cards) {
    const near = s > 0 ? lo(b) : hi(b);
    if (id === from || (near - middle) * s <= 0) continue;
    const along = Math.max(0, (near - (s > 0 ? hi(a) : lo(a))) * s);
    const start = v ? b.x : b.y;
    const across = Math.max(0, start - line, line - (start + (v ? b.w : b.h)));
    if (along + 3 * across < score) [best, score] = [id, along + 3 * across];
  }
  return best;
}

/** The card nearest the point `p`, center to point. `inside`: only cards in that box. `null`: no such card. */
export function navNearest(cards: Map<string, Rect>, p: { x: number; y: number }, inside?: Rect): string | null {
  let best: string | null = null;
  let d = Infinity;
  for (const [id, r] of cards) {
    if (inside && (r.x < inside.x || r.y < inside.y || r.x + r.w > inside.x + inside.w || r.y + r.h > inside.y + inside.h)) continue;
    const e = Math.hypot(mid(r).x - p.x, mid(r).y - p.y);
    if (e < d) [best, d] = [id, e];
  }
  return best;
}

// ---- Keys ----

export type BoardCommand =
  | Dir
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
  "session-next": "down",
  "session-prev": "up",
  "list-project": "left",
  "list-back": "right",
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
  if (key === "ArrowDown") return "down";
  if (key === "ArrowUp") return "up";
  if (inSearch) return null;
  if (key === "ArrowRight") return "right";
  if (key === "ArrowLeft") return "left";
  if (key === "Home") return "fit";
  if (key === "=" || key === "+") return "zoom-in";
  if (key === "-") return "zoom-out";
  return null;
}

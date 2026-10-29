/**
 * Mission control's graph, the pure part: the tree layout of cards and
 * clusters, the strings between them, the keys' moves through the graph, and
 * the view math. No DOM, no state, no imports, so tests run it as it is.
 *
 * The graph is a tree. Admiral is at the top. Under it, the blocks sit in a
 * fixed grid of three slots per row that grows down: each fleet frame, each
 * project in no fleet with its Skipper card above its cluster, and Other
 * last. A block wider than a slot takes a full row by itself. Rows top-align,
 * and each row is as tall as its tallest block. Inside a fleet frame, its
 * Skipper card sits at the top and its projects follow the same rule: three
 * per row, growing down. The grid never fits the window, so a resize moves
 * nothing and a hand offset stays valid. In a cluster, cards sit in a grid:
 * one column up to two cards, two up to six, three past that. A plan is a
 * 20px caption above its cards. A folded cluster is one card.
 *
 * A moved card or cluster keeps an offset from its automatic place, so a new
 * session or a fold never undoes a hand move. All lengths are in unzoomed px:
 * the board scales the whole world by the app zoom.
 */

export const CARD_W = 224;
/** A 20px head, a 40px body and the 1px border above and below. */
export const CARD_H = 62;
const PAD = 12;
const HEAD = 20;
const GAP = 12;
const VGAP = 16;
/** A fleet frame's padding. */
const FP = 16;
/** Between a Skipper card and what hangs from it. */
const DROP = 48;
/** Between the blocks of the outer grid, across and down; and between the projects of a fleet. */
const BLOCK_GAP = 48;
const FLEET_GAP = 24;
/** A slot of the grid: as wide as a two-column cluster. */
export const SLOT_W = PAD * 2 + 2 * CARD_W + GAP;
/** Slots in a row. */
export const GRID_COLS = 3;
/** The outer grid's width: Admiral centers on it, so it stays put when blocks come and go. */
export const GRID_W = GRID_COLS * SLOT_W + (GRID_COLS - 1) * BLOCK_GAP;
/** Between Admiral and the blocks. */
const ADMIRAL_DROP = 96;
export const PLAN_H = 20;

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
export type Offsets = Record<string, { dx: number; dy: number }>;
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
export interface Placed {
  /** Every card by id: sessions, Skippers and Admiral. A folded cluster's card is in `clusters`. */
  cards: Map<string, Rect>;
  clusters: Map<string, Cluster>;
  plans: PlanBox[];
}

const off = (offs: Offsets, id: string) => offs[id] ?? { dx: 0, dy: 0 };

interface ProjectBox {
  w: number;
  h: number;
  cards: { id: string; x: number; y: number }[];
  plans: { plan: GPlan; x: number; y: number; w: number }[];
}

function projectBox(p: GProject): ProjectBox {
  const n = p.cards.length;
  const cols = Math.max(1, n <= 2 ? n : n <= 6 ? 2 : 3);
  const w = PAD * 2 + cols * CARD_W + (cols - 1) * GAP;
  const placed = new Set(p.plans.flatMap((pl) => pl.cards));
  const groups: { plan: GPlan | null; list: string[] }[] = p.plans.map((plan) => ({ plan, list: plan.cards }));
  const loose = p.cards.filter((id) => !placed.has(id));
  if (loose.length) groups.push({ plan: null, list: loose });
  const cards: ProjectBox["cards"] = [];
  const plans: ProjectBox["plans"] = [];
  let y = HEAD + PAD;
  for (const g of groups) {
    if (g.plan) {
      plans.push({ plan: g.plan, x: PAD, y, w: w - PAD * 2 });
      y += PLAN_H;
    }
    g.list.forEach((id, i) => cards.push({ id, x: PAD + (i % cols) * (CARD_W + GAP), y: y + Math.floor(i / cols) * (CARD_H + VGAP) }));
    y += Math.ceil(g.list.length / cols) * (CARD_H + VGAP);
  }
  return { w, h: Math.max(HEAD + PAD * 2, y - VGAP + PAD), cards, plans };
}

/** A project with its Skipper card above its cluster. */
interface Block {
  p: GProject;
  w: number;
  h: number;
  skipper: { x: number; y: number } | null;
  cluster: { x: number; y: number; box: ProjectBox };
}

function block(p: GProject): Block {
  const box = p.folded ? { w: CARD_W, h: CARD_H, cards: [], plans: [] } : projectBox(p);
  const w = Math.max(box.w, p.skipper ? CARD_W : 0);
  const cy = p.skipper ? CARD_H + DROP : 0;
  return { p, w, h: cy + box.h, skipper: p.skipper ? { x: w / 2 - CARD_W / 2, y: 0 } : null, cluster: { x: w / 2 - box.w / 2, y: cy, box } };
}

/**
 * Places blocks in a grid of `GRID_COLS` slots per row: left to right, then
 * the next row. A block wider than a slot takes a full row by itself. Rows
 * top-align, and each row is as tall as its tallest block.
 */
export function gridPlace(blocks: { w: number; h: number }[], gap: number, rowGap: number): { at: { x: number; y: number }[]; w: number; h: number } {
  const at: { x: number; y: number }[] = [];
  let y = 0;
  let rowH = 0;
  let col = 0;
  let w = 0;
  let open = false;
  const endRow = () => {
    y += rowH + rowGap;
    rowH = 0;
    col = 0;
    open = false;
  };
  for (const b of blocks) {
    const wide = b.w > SLOT_W;
    if ((wide && col > 0) || col >= GRID_COLS) endRow();
    const x = col * (SLOT_W + gap);
    at.push({ x, y });
    rowH = Math.max(rowH, b.h);
    w = Math.max(w, x + b.w);
    open = true;
    if (wide) endRow();
    else col++;
  }
  return { at, w, h: open ? y + rowH : Math.max(0, y - rowGap) };
}

/** Lays the graph out and applies the hand offsets. */
export function placeGraph(g: Graph, offs: Offsets): Placed {
  const out: Placed = { cards: new Map(), clusters: new Map(), plans: [] };
  const putBlock = (b: Block, ox: number, oy: number) => {
    const o = off(offs, b.p.key);
    const bx = ox + o.dx;
    const by = oy + o.dy;
    if (b.skipper && b.p.skipper) {
      const so = off(offs, b.p.skipper);
      out.cards.set(b.p.skipper, { x: bx + b.skipper.x + so.dx, y: by + b.skipper.y + so.dy, w: CARD_W, h: CARD_H });
    }
    const cx = bx + b.cluster.x;
    const cy = by + b.cluster.y;
    out.clusters.set(b.p.key, { x: cx, y: cy, w: b.cluster.box.w, h: b.cluster.box.h, fleet: false, folded: b.p.folded });
    for (const pl of b.cluster.box.plans) out.plans.push({ key: `${b.p.key}:${pl.plan.uid}`, plan: pl.plan, x: cx + pl.x, y: cy + pl.y, w: pl.w, h: PLAN_H });
    for (const c of b.cluster.box.cards) {
      const co = off(offs, c.id);
      out.cards.set(c.id, { x: cx + c.x + co.dx, y: cy + c.y + co.dy, w: CARD_W, h: CARD_H });
    }
  };
  type Top = { w: number; h: number; draw: (x: number, y: number) => void };
  const tops: Top[] = [];
  for (const f of g.fleets) {
    const blocks = f.projects.map(block);
    // The fleet's own grid: three projects per row, under its Skipper.
    const grid = gridPlace(blocks, FLEET_GAP, FLEET_GAP);
    const by = HEAD + FP + (f.skipper ? CARD_H + DROP : 0);
    const w = Math.max(grid.w + FP * 2, f.skipper ? CARD_W + FP * 2 : 0);
    const h = by + grid.h + FP;
    tops.push({
      w,
      h,
      draw: (fx, fy) => {
        const o = off(offs, f.key);
        const tx = fx + o.dx;
        const ty = fy + o.dy;
        out.clusters.set(f.key, { x: tx, y: ty, w, h, fleet: true, folded: false });
        if (f.skipper) {
          const so = off(offs, f.skipper);
          out.cards.set(f.skipper, { x: tx + w / 2 - CARD_W / 2 + so.dx, y: ty + HEAD + FP + so.dy, w: CARD_W, h: CARD_H });
        }
        blocks.forEach((b, i) => putBlock(b, tx + FP + grid.at[i].x, ty + by + grid.at[i].y));
      },
    });
  }
  for (const p of g.projects) {
    const b = block(p);
    tops.push({ w: b.w, h: b.h, draw: (x, y) => putBlock(b, x, y) });
  }
  // The outer grid, under Admiral.
  const y0 = g.admiral ? CARD_H + ADMIRAL_DROP : 0;
  const grid = gridPlace(tops, BLOCK_GAP, BLOCK_GAP);
  tops.forEach((t, i) => t.draw(grid.at[i].x, y0 + grid.at[i].y));
  if (g.admiral) {
    const o = off(offs, g.admiral);
    out.cards.set(g.admiral, { x: GRID_W / 2 - CARD_W / 2 + o.dx, y: o.dy, w: CARD_W, h: CARD_H });
  }
  return out;
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

/** A string from a parent's foot to a child's pin: a curve that sags with its length. `lx, ly`: where its label sits. */
export function strand(a: { x: number; y: number }, b: { x: number; y: number }): { d: string; lx: number; ly: number } {
  const mx = (a.x + b.x) / 2;
  const my = (a.y + b.y) / 2;
  const sag = Math.max(8, Math.min(Math.hypot(b.x - a.x, b.y - a.y) * 0.08, 48));
  return { d: `M${a.x} ${a.y}Q${mx} ${my + sag} ${b.x} ${b.y}`, lx: mx, ly: my + sag / 2 };
}
export const foot = (r: Rect) => ({ x: r.x + r.w / 2, y: r.y + r.h });
export const pin = (r: Rect) => ({ x: r.x + r.w / 2, y: r.y });

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

const allProjects = (g: Graph) => [...g.fleets.flatMap((f) => f.projects), ...g.projects];
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

/**
 * Mission control's layout and connectors, the pure part (no DOM).
 *
 * Each cluster is a tidy tree: a fleet, a project in no fleet, or Other. Its
 * Skipper's window sits on top, and its projects and sessions hang under it,
 * indented, on a rail from the bottom of the Skipper's face. A project in a
 * fleet with its own Skipper hangs its sessions on that Skipper's rail inside
 * its frame. The clusters flow left to right in a grid of equal cells,
 * `perRow` to a row, then wrap; a row is as tall as its tallest cluster.
 * Admiral sits top left. Its trunk runs down from the bottom of its face
 * along the left edge and turns right into each row's bus.
 *
 * Only the data places things: the graph's order is the sidebar tree's, and
 * the window size is never an input. A fold or a new session can change a
 * row's height; nothing else moves. All lengths are unzoomed px.
 *
 * A connector is solid, with right angles and rounded corners. It leaves the
 * bottom middle of a window's face (or a card's foot). Into a cluster's top
 * window it comes down the row's bus; to work under a Skipper it runs down
 * that Skipper's rail and turns right into the card; from anywhere else it
 * takes the bus to the target's cluster rail, then the rail.
 */
import { type At, CARD_H, CARD_W, PLAN_H, type GProject, type Graph, type Placed, type Rect, type Rung, WINDOW, faceBox } from "../missionLayout.ts";

const PAD = 12;
const HEAD = 20;
const VGAP = 16;
/** A frame's padding around a cluster. */
const FP = 16;
/** From a cluster's window to the work under it: the rail's indent. */
const IND = 64;
/** The same inside a project frame with its own Skipper. */
const IND2 = 56;
/** Between a window and the work under it. */
const DROP = 24;
const CELL_GAP = 48;
const ROW_GAP = 64;
/** The bus runs this far above a row. */
const BUS = 24;
const TRUNK_GAP = 48;

/** Clusters per row, from the data only: ⌈√n⌉, at most 4. */
export const perRow = (n: number) => Math.min(4, Math.max(1, Math.ceil(Math.sqrt(n))));

/** The middle of a window's face: its rail's x, and where a connector ends. */
const faceMid = (r: Rect, rung: Rung) => {
  const f = faceBox(r, rung);
  return { x: f.x + f.w / 2, y: f.y + f.h / 2 };
};

interface Top {
  key: string;
  fleet: boolean;
  skipper: string | null;
  projects: GProject[];
}

/** A project frame's width: with its own Skipper, its sessions hang on that Skipper's rail. */
const projectW = (p: GProject) => (p.folded ? CARD_W : PAD + (p.skipper ? Math.max(WINDOW.project.w, IND2 + CARD_W) : CARD_W) + PAD);

/** A cluster's own width, before the grid gives every cell the widest. */
function naturalW(t: Top): number {
  const win = t.skipper ? WINDOW[t.fleet ? "fleet" : "project"].w : 0;
  const work = t.fleet ? Math.max(0, ...t.projects.map((p) => IND - PAD + projectW(p))) : IND + CARD_W;
  return FP + Math.max(win, work) + FP;
}

export function placeGraph(g: Graph): Placed {
  const out: Placed = { cards: new Map(), clusters: new Map(), plans: [], rungs: new Map(), at: new Map(), trunk: null };
  const tops: Top[] = [
    ...g.fleets.map((f) => ({ key: f.key, fleet: true, skipper: f.skipper, projects: f.projects })),
    ...g.projects.map((p) => ({ key: p.key, fleet: false, skipper: p.skipper, projects: [p] })),
  ];
  const putWindow = (id: string, rung: Rung, x: number, y: number) => {
    const w = WINDOW[rung];
    out.cards.set(id, { x, y, w: w.w, h: w.h });
    out.rungs.set(id, rung);
  };
  // The cards of one project from `y`, at `x`: plan by plan, then the loose ones. Returns the next y.
  const cardsOf = (p: GProject, x: number, y: number, at: At) => {
    const inPlan = new Set(p.plans.flatMap((pl) => pl.cards));
    const groups = [...p.plans.map((pl) => ({ pl, ids: pl.cards })), { pl: null, ids: p.cards.filter((id) => !inPlan.has(id)) }];
    for (const gr of groups) {
      if (!gr.ids.length) continue;
      if (gr.pl) {
        out.plans.push({ key: `${p.key}:${gr.pl.uid}`, plan: gr.pl, x, y, w: CARD_W, h: PLAN_H });
        y += PLAN_H;
      }
      for (const id of gr.ids) {
        out.cards.set(id, { x, y, w: CARD_W, h: CARD_H });
        out.at.set(id, at);
        y += CARD_H + VGAP;
      }
    }
    return y;
  };

  const aw = WINDOW.admiral;
  const trunk = g.admiral ? 8 + aw.face / 2 : null;
  out.trunk = trunk;
  const x0 = trunk === null ? 0 : trunk + TRUNK_GAP;
  const cell = Math.max(0, ...tops.map(naturalW));
  const cols = perRow(tops.length);
  let rowTop = g.admiral ? aw.h + ROW_GAP : BUS;
  for (let r = 0; r * cols < tops.length; r++) {
    const bus = rowTop - BUS;
    let rowH = 0;
    tops.slice(r * cols, r * cols + cols).forEach((t, i) => {
      const x = x0 + i * (cell + CELL_GAP);
      const wx = x + FP;
      // The cluster's rail: its Skipper's face; with no Skipper, where that face would be.
      const rail = wx + 8 + WINDOW[t.fleet ? "fleet" : "project"].face / 2;
      const owners = t.skipper ? [t.skipper] : [];
      let cy = rowTop + HEAD + FP;
      // A fleet's frame takes its place in the map first, so the board draws it under its projects' frames.
      if (t.fleet) out.clusters.set(t.key, { x, y: rowTop, w: cell, h: 0, fleet: true, folded: false });
      if (t.skipper) {
        putWindow(t.skipper, t.fleet ? "fleet" : "project", wx, cy);
        out.at.set(t.skipper, { bus, rail, owners: [], head: true });
        cy += WINDOW[t.fleet ? "fleet" : "project"].h + DROP;
      }
      for (const p of t.projects) {
        const fx = t.fleet ? wx + IND - PAD : x;
        if (p.folded) {
          // A folded project is one card where its work would hang.
          const r = { x: wx + IND, y: cy, w: CARD_W, h: CARD_H };
          out.clusters.set(p.key, { ...r, fleet: false, folded: true });
          out.at.set(p.key, { bus, rail, owners });
          cy += CARD_H + VGAP;
          continue;
        }
        if (!t.fleet) {
          // A project in no fleet is its own cluster: the frame holds its Skipper and its work.
          cy = cardsOf(p, wx + IND, cy, { bus, rail, owners });
          continue;
        }
        const top = cy;
        let iy = top + HEAD + PAD;
        const inner = [...owners];
        let at: At = { bus, rail, owners: inner };
        if (p.skipper) {
          putWindow(p.skipper, "project", fx + PAD, iy);
          out.at.set(p.skipper, { bus, rail, owners: [...owners] });
          iy += WINDOW.project.h + DROP;
          at = { bus, rail, owners: [...owners, p.skipper] };
        }
        iy = cardsOf(p, p.skipper ? fx + PAD + IND2 : fx + PAD, iy, at);
        const h = iy - VGAP + PAD - top;
        out.clusters.set(p.key, { x: fx, y: top, w: projectW(p), h, fleet: false, folded: false });
        cy = top + h + VGAP;
      }
      const h = Math.max(cy - VGAP, rowTop + HEAD + FP) + FP - rowTop;
      // The frame: a fleet's, or a project's in no fleet. A folded project in no fleet has no frame.
      if (t.fleet) out.clusters.set(t.key, { x, y: rowTop, w: cell, h, fleet: true, folded: false });
      else if (!t.projects[0].folded) out.clusters.set(t.key, { x, y: rowTop, w: cell, h, fleet: false, folded: false });
      rowH = Math.max(rowH, h);
    });
    rowTop += rowH + ROW_GAP;
  }
  if (g.admiral) putWindow(g.admiral, "admiral", 0, 0);
  return out;
}

type Pt = { x: number; y: number };

/** A right-angle path through `pts`, each corner rounded by up to `r`. Repeated and straight-through points drop out. */
export function orth(pts: Pt[], r = 8): string {
  const p = pts.filter((q, i) => i === 0 || q.x !== pts[i - 1].x || q.y !== pts[i - 1].y);
  const k = p.filter((q, i) => i === 0 || i === p.length - 1 || !((p[i - 1].x === q.x && q.x === p[i + 1].x) || (p[i - 1].y === q.y && q.y === p[i + 1].y)));
  let d = `M${k[0].x} ${k[0].y}`;
  for (let i = 1; i < k.length - 1; i++) {
    const a = k[i - 1];
    const b = k[i];
    const c = k[i + 1];
    const l1 = Math.hypot(b.x - a.x, b.y - a.y);
    const l2 = Math.hypot(c.x - b.x, c.y - b.y);
    const rr = Math.min(r, l1 / 2, l2 / 2);
    d += `L${b.x + ((a.x - b.x) / l1) * rr} ${b.y + ((a.y - b.y) / l1) * rr}Q${b.x} ${b.y} ${b.x + ((c.x - b.x) / l2) * rr} ${b.y + ((c.y - b.y) / l2) * rr}`;
  }
  const z = k[k.length - 1];
  return `${d}L${z.x} ${z.y}`;
}

/**
 * The connector from `from` to `to` (a card, a window or a folded cluster),
 * as path data. `null` when either is not placed.
 */
export function route(p: Placed, from: string, to: string): string | null {
  const fr = p.cards.get(from);
  const tr = p.cards.get(to) ?? p.clusters.get(to);
  const info = p.at.get(to);
  if (!fr || !tr || !info) return null;
  const fRung = p.rungs.get(from);
  const tRung = p.rungs.get(to);
  const a = fRung ? (() => { const f = faceBox(fr, fRung); return { x: f.x + f.w / 2, y: f.y + f.h - 4 }; })() : { x: fr.x + fr.w / 2, y: fr.y + fr.h };
  const end = tRung ? faceMid(tr, tRung) : { x: tr.x, y: tr.y + tr.h / 2 };
  // Into a cluster's top window: down to the row's bus, across, down into the face. Admiral comes down its trunk.
  if (info.head) return orth([a, { x: a.x, y: info.bus }, { x: end.x, y: info.bus }, end]);
  // Work under this Skipper: down its own rail, then right.
  if (info.owners.includes(from)) return orth([a, { x: a.x, y: end.y }, end]);
  // Anything else: the bus to the cluster's rail, down it, then right.
  return orth([a, { x: a.x, y: info.bus }, { x: info.rail, y: info.bus }, { x: info.rail, y: end.y }, end]);
}

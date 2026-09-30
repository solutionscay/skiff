import type { Layout, SplitDir } from "./types";

// ---------- tree ----------

export const leaf = (session: string): Layout => ({ type: "pane", session });

export const clampRatio = (r: number) => Math.min(0.9, Math.max(0.1, r));

/** Sessions in reading order: a before b. */
export function sessionsOf(l: Layout | null): string[] {
  if (!l) return [];
  return l.type === "pane" ? [l.session] : [...sessionsOf(l.a), ...sessionsOf(l.b)];
}

export function replacePane(l: Layout, session: string, f: (p: Layout) => Layout): Layout {
  if (l.type === "pane") return l.session === session ? f(l) : l;
  return { ...l, a: replacePane(l.a, session, f), b: replacePane(l.b, session, f) };
}

/** Drops a pane. A split that loses one side becomes the other side. */
export function removePane(l: Layout | null, session: string): Layout | null {
  if (!l) return null;
  if (l.type === "pane") return l.session === session ? null : l;
  const a = removePane(l.a, session);
  const b = removePane(l.b, session);
  if (!a) return b;
  if (!b) return a;
  return { ...l, a, b };
}

/** Puts `session` next to `target`: right of it for `row`, below it for `col`. */
export function splitPane(l: Layout, target: string, dir: SplitDir, session: string): Layout {
  return replacePane(l, target, (p) => ({ type: "split", dir, ratio: 0.5, a: p, b: leaf(session) }));
}

/** 2: side by side. 3: one left, two stacked right. 4+: top half over bottom half. */
export function build(ids: string[]): Layout {
  if (ids.length === 1) return leaf(ids[0]);
  if (ids.length === 2) return { type: "split", dir: "row", ratio: 0.5, a: leaf(ids[0]), b: leaf(ids[1]) };
  if (ids.length === 3) {
    return {
      type: "split", dir: "row", ratio: 0.5, a: leaf(ids[0]),
      b: { type: "split", dir: "col", ratio: 0.5, a: leaf(ids[1]), b: leaf(ids[2]) },
    };
  }
  const half = Math.ceil(ids.length / 2);
  return { type: "split", dir: "col", ratio: 0.5, a: build(ids.slice(0, half)), b: build(ids.slice(half)) };
}

/** The template a layout's shape matches, by the names the + menu uses; null for any other shape. */
export function shapeName(l: Layout): string | null {
  if (l.type === "pane") return null;
  const n = sessionsOf(l).length;
  if (n === 2) return l.dir === "row" ? "2 side by side" : "2 stacked";
  if (n === 3 && l.a.type === "pane" && l.b.type === "split") {
    if (l.dir === "row") return l.b.dir === "col" ? "1 left, 2 right" : "3 side by side";
    if (l.b.dir === "row") return "1 over 2";
  }
  if (n === 4 && l.dir === "col" && l.a.type === "split" && l.b.type === "split" && l.a.dir === "row" && l.b.dir === "row") return "2 by 2";
  return null;
}

export interface Rect { session: string; x: number; y: number; w: number; h: number }

export function rects(l: Layout, x: number, y: number, w: number, h: number, out: Rect[] = []): Rect[] {
  if (l.type === "pane") {
    out.push({ session: l.session, x, y, w, h });
  } else if (l.dir === "row") {
    const aw = Math.round(w * l.ratio);
    rects(l.a, x, y, aw, h, out);
    rects(l.b, x + aw, y, w - aw, h, out);
  } else {
    const ah = Math.round(h * l.ratio);
    rects(l.a, x, y, w, ah, out);
    rects(l.b, x, y + ah, w, h - ah, out);
  }
  return out;
}

export type Direction = "left" | "right" | "up" | "down";

/** The nearest cell from `from` in a direction, by on-screen rectangles. */
export function neighbor(cells: Map<string, DOMRect>, from: string, dir: Direction): string | null {
  const c = cells.get(from);
  if (!c) return null;
  let best: string | null = null;
  let score = Infinity;
  for (const [id, r] of cells) {
    if (id === from) continue;
    let gap: number;
    let overlap: boolean;
    let off: number;
    if (dir === "left" || dir === "right") {
      gap = dir === "left" ? c.left - r.right : r.left - c.right;
      overlap = r.top < c.bottom && r.bottom > c.top;
      off = Math.abs(r.top + r.height / 2 - (c.top + c.height / 2));
    } else {
      gap = dir === "up" ? c.top - r.bottom : r.top - c.bottom;
      overlap = r.left < c.right && r.right > c.left;
      off = Math.abs(r.left + r.width / 2 - (c.left + c.width / 2));
    }
    if (gap < -2) continue;
    const s = gap + (overlap ? 0 : 100_000) + off / 1000;
    if (s < score) {
      score = s;
      best = id;
    }
  }
  return best;
}

/** Structure without ratios, so a drag or a daemon round trip does not rebuild the DOM. */
export function shape(l: Layout | null): string {
  if (!l) return "";
  return l.type === "pane" ? l.session : `${l.dir}(${shape(l.a)},${shape(l.b)})`;
}

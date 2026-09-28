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

/** Small outline of the layout for the group list. */
export function glyph(l: Layout, w = 16, h = 12): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("width", String(w));
  svg.setAttribute("height", String(h));
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("glyph");
  for (const r of rects(l, 0, 0, w, h)) {
    const e = document.createElementNS(ns, "rect");
    e.setAttribute("x", String(r.x + 0.5));
    e.setAttribute("y", String(r.y + 0.5));
    e.setAttribute("width", String(Math.max(0, r.w - 1)));
    e.setAttribute("height", String(Math.max(0, r.h - 1)));
    svg.appendChild(e);
  }
  return svg;
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

// ---------- view ----------

interface Cell { cell: HTMLDivElement; head: HTMLDivElement; body: HTMLDivElement }

interface ViewOpts {
  /** Move the session's terminal into `body`. */
  attach: (session: string, body: HTMLElement) => void;
  /** Fill a pane header. Runs on every sync. */
  head: (session: string, head: HTMLElement, cell: HTMLElement) => void;
  focus: (session: string) => void;
  menu: (session: string, e: MouseEvent) => void;
  /** Pane sizes changed: fit the terminals. */
  resized: () => void;
  /** A divider drag ended. The layout passed to sync holds the new ratio. */
  committed: () => void;
}

/** Structure without ratios, so a drag or a daemon round trip does not rebuild the DOM. */
function shape(l: Layout | null): string {
  if (!l) return "";
  return l.type === "pane" ? l.session : `${l.dir}(${shape(l.a)},${shape(l.b)})`;
}

function nodeAt(l: Layout, path: string): Layout {
  for (const k of path) if (l.type === "split") l = k === "a" ? l.a : l.b;
  return l;
}

/**
 * Renders a layout tree into `host` as nested flex boxes with 1px dividers.
 * Rebuilds the DOM only when the tree shape changes.
 */
export function createLayoutView(host: HTMLElement, o: ViewOpts) {
  const root = document.createElement("div");
  root.className = "layout";
  host.appendChild(root);

  let current: Layout | null = null;
  let key = "";
  let cells = new Map<string, Cell>();
  /** First-side element of each split, by path from the root. */
  let firsts = new Map<string, HTMLElement>();

  const basis = (r: number) => `0 0 calc(${(r * 100).toFixed(3)}% - 0.5px)`;

  function node(l: Layout, path: string): HTMLElement {
    if (l.type === "pane") {
      const cell = document.createElement("div");
      cell.className = "cell";
      cell.dataset.session = l.session;
      const head = document.createElement("div");
      head.className = "cell-head";
      const body = document.createElement("div");
      body.className = "cell-body";
      cell.append(head, body);
      const id = l.session;
      // Head buttons act on click; a focus render on mousedown would replace them first.
      cell.addEventListener("mousedown", (e) => {
        if (!(e.target as Element).closest("button")) o.focus(id);
      }, true);
      cell.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        o.menu(id, e);
      });
      cells.set(id, { cell, head, body });
      return cell;
    }
    const box = document.createElement("div");
    box.className = `split ${l.dir}`;
    const a = node(l.a, path + "a");
    const b = node(l.b, path + "b");
    a.style.flex = basis(l.ratio);
    b.style.flex = "1 1 0";
    firsts.set(path, a);
    const div = document.createElement("div");
    div.className = "divider";
    div.addEventListener("mousedown", (e) => drag(e, box, a, path, l.dir));
    box.append(a, div, b);
    return box;
  }

  function drag(e: MouseEvent, box: HTMLElement, a: HTMLElement, path: string, dir: SplitDir) {
    if (e.button !== 0) return;
    e.preventDefault();
    const r = box.getBoundingClientRect();
    document.body.classList.add(dir === "row" ? "dragging-col" : "dragging-row");
    let frame = 0;
    const move = (m: MouseEvent) => {
      const n = current && nodeAt(current, path);
      if (!n || n.type !== "split") return;
      n.ratio = clampRatio(dir === "row" ? (m.clientX - r.left) / r.width : (m.clientY - r.top) / r.height);
      a.style.flex = basis(n.ratio);
      if (!frame) frame = requestAnimationFrame(() => {
        frame = 0;
        o.resized();
      });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("dragging-col", "dragging-row");
      o.resized();
      o.committed();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  /** Returns true when any divider moved. */
  function applyRatios(l: Layout, path: string): boolean {
    if (l.type !== "split") return false;
    let changed = false;
    const a = firsts.get(path);
    if (a) {
      const flex = a.style.flex;
      a.style.flex = basis(l.ratio);
      changed = a.style.flex !== flex;
    }
    const ca = applyRatios(l.a, path + "a");
    const cb = applyRatios(l.b, path + "b");
    return changed || ca || cb;
  }

  /** Shows `layout`. "rebuilt" when panes moved, "resized" when only dividers moved. */
  function sync(layout: Layout | null): "rebuilt" | "resized" | false {
    current = layout;
    const next = shape(layout);
    let rebuilt: "rebuilt" | "resized" | false = false;
    if (next !== key) {
      key = next;
      cells = new Map();
      firsts = new Map();
      root.replaceChildren(...(layout ? [node(layout, "")] : []));
      for (const [id, c] of cells) o.attach(id, c.body);
      rebuilt = "rebuilt";
    } else if (layout && applyRatios(layout, "")) {
      rebuilt = "resized";
    }
    for (const [id, c] of cells) o.head(id, c.head, c.cell);
    return rebuilt;
  }

  function cellRects(): Map<string, DOMRect> {
    return new Map([...cells].map(([id, c]) => [id, c.cell.getBoundingClientRect()]));
  }

  return { sync, cellRects, body: (id: string) => cells.get(id)?.body ?? null };
}

import type { Layout, SplitDir } from "./types";
import { clampRatio, shape } from "./layout";



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

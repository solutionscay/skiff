/** Where a drop lands: the pane's nearest edge, or the whole area when none is shown. */
export type Zone = "left" | "right" | "top" | "bottom" | "center";

export interface DropTarget {
  /** The pane's session, or null for an empty terminal area. */
  session: string | null;
  zone: Zone;
  /** The rim of the whole area: the new pane wraps every pane, which makes a T shape. */
  outer?: boolean;
  /** A group row in the sidebar: the sessions join that group. */
  group?: string;
}

interface Opts {
  /** Text on the ghost that follows the pointer. */
  label: string;
  /** Pane rectangles by session, read when the drag starts. */
  cells: () => Map<string, DOMRect>;
  /** The whole terminal area, for a drop when no pane is shown. Null: no drop there. */
  area: HTMLElement | null;
  /** Group rows in the sidebar that take a drop, read when the drag starts. */
  groupRows?: () => { id: string; rect: DOMRect }[];
  /** Why the target takes no drop, shown on the ghost. Null: no objection. */
  refuses?: (t: DropTarget) => string | null;
  /** False: the drop would change nothing, so the target shows no highlight. */
  accepts?: (t: DropTarget) => boolean;
  drop: (t: DropTarget) => void;
  /** A press that never moved far enough: a plain click. */
  click?: () => void;
}

const THRESHOLD = 5;
/** Width of the rim, in px, where a drop wraps the whole layout. */
const RIM = 28;
/** The top rim is thin: pane headers sit there, and a drop on a header means "above this pane". */
const RIM_TOP = 8;
/** Extra rim width, in px, while the pointer is already on the rim, so the edge does not flicker. */
const RIM_HOLD = 6;
/** How much closer, as a fraction of the pane, a new edge must be before the zone moves to it. */
const ZONE_HOLD = 0.08;

/** The pane's nearest edge. Near a diagonal the current zone holds, so the preview does not flip back and forth. */
function zoneAt(r: DOMRect, x: number, y: number, held?: Zone): Zone {
  const fx = (x - r.left) / r.width;
  const fy = (y - r.top) / r.height;
  const d: [Zone, number][] = [["left", fx], ["right", 1 - fx], ["top", fy], ["bottom", 1 - fy]];
  d.sort((a, b) => a[1] - b[1]);
  const keep = held && d.find(([z]) => z === held);
  return keep && keep[1] - d[0][1] < ZONE_HOLD ? held! : d[0][0];
}

function previewRect(r: DOMRect, z: Zone): { l: number; t: number; w: number; h: number } {
  const hw = r.width / 2;
  const hh = r.height / 2;
  switch (z) {
    case "left": return { l: r.left, t: r.top, w: hw, h: r.height };
    case "right": return { l: r.left + hw, t: r.top, w: hw, h: r.height };
    case "top": return { l: r.left, t: r.top, w: r.width, h: hh };
    case "bottom": return { l: r.left, t: r.top + hh, w: r.width, h: hh };
    default: return { l: r.left, t: r.top, w: r.width, h: r.height };
  }
}

/**
 * Pointer drag from a mousedown. Uses mouse events, not the HTML drag API,
 * which the webview's file-drop handling can swallow. A press that does not
 * move past the threshold stays a click.
 */
export function beginDrag(down: MouseEvent, o: Opts) {
  if (down.button !== 0) return;
  // Else the press starts a text selection that paints across the panes before the drag begins.
  down.preventDefault();
  const sx = down.clientX;
  const sy = down.clientY;
  let active = false;
  let cells = new Map<string, DOMRect>();
  let rows: { id: string; rect: DOMRect }[] = [];
  let target: DropTarget | null = null;
  /** The last hit, accepted or not: the zone and rim hold against it. */
  let last: DropTarget | null = null;
  /** The preview box as last painted, so an unchanged frame writes nothing. */
  let painted = "";
  let frame = 0;
  let mx = 0;
  let my = 0;
  let ghost: HTMLElement | null = null;
  let preview: HTMLElement | null = null;
  /** Tooltips are off while dragging: they would sit over the drop target. */
  const titled: [Element, string][] = [];

  const start = () => {
    active = true;
    cells = o.cells();
    rows = o.groupRows?.() ?? [];
    document.body.classList.add("dragging-session");
    for (const el of document.querySelectorAll("#sidebar-scroll [title]")) {
      titled.push([el, el.getAttribute("title") ?? ""]);
      el.removeAttribute("title");
    }
    ghost = document.createElement("div");
    ghost.className = "drag-ghost";
    ghost.textContent = o.label;
    preview = document.createElement("div");
    preview.className = "drop-preview";
    preview.hidden = true;
    document.body.append(preview, ghost);
  };

  const inside = (r: DOMRect, x: number, y: number) => x >= r.left && x < r.right && y >= r.top && y < r.bottom;
  const whole = (r: DOMRect) => ({ l: r.left, t: r.top, w: r.width, h: r.height });

  /** The target under the pointer and its preview box, before `accepts` has its say. */
  const hit = (x: number, y: number): { t: DropTarget; p: ReturnType<typeof previewRect> } | null => {
    const row = rows.find(({ rect }) => inside(rect, x, y));
    if (row) return { t: { session: null, zone: "center", group: row.id }, p: whole(row.rect) };
    const area = o.area?.getBoundingClientRect();
    if (area && cells.size > 0 && inside(area, x, y)) {
      const d: [Zone, number][] = [["left", x - area.left], ["right", area.right - x - 1], ["top", y - area.top], ["bottom", area.bottom - y - 1]];
      d.sort((a, b) => a[1] - b[1]);
      const rim = (d[0][0] === "top" ? RIM_TOP : RIM) + (last?.outer ? RIM_HOLD : 0);
      if (d[0][1] < rim) return { t: { session: null, zone: d[0][0], outer: true }, p: previewRect(area, d[0][0]) };
    }
    for (const [session, r] of cells) {
      if (inside(r, x, y)) {
        const zone = zoneAt(r, x, y, last && !last.outer && last.session === session ? last.zone : undefined);
        return { t: { session, zone }, p: previewRect(r, zone) };
      }
    }
    if (area && cells.size === 0 && inside(area, x, y)) return { t: { session: null, zone: "center" }, p: whole(area) };
    return null;
  };

  const move = (m: MouseEvent) => {
    if (cancelled) return;
    if (!active) {
      if (Math.abs(m.clientX - sx) + Math.abs(m.clientY - sy) < THRESHOLD) return;
      start();
    }
    m.preventDefault();
    mx = m.clientX;
    my = m.clientY;
    // One paint per frame: mice that report faster than the screen do not queue extra layout work.
    if (!frame) frame = requestAnimationFrame(paint);
  };

  const paint = () => {
    frame = 0;
    if (!ghost || !preview) return;
    ghost.style.transform = `translate(${mx + 12}px, ${my + 12}px)`;
    const h = hit(mx, my);
    last = h?.t ?? null;
    const why = h ? o.refuses?.(h.t) ?? null : null;
    const text = why ?? o.label;
    if (ghost.textContent !== text) ghost.textContent = text;
    ghost.classList.toggle("refused", !!why);
    document.body.classList.toggle("drop-refused", !!why);
    target = h && !why && (o.accepts?.(h.t) ?? true) ? h.t : null;
    const p = target ? h!.p : null;
    const key = p ? `${p.l},${p.t},${p.w},${p.h},${target!.zone}` : "";
    if (key === painted) return;
    painted = key;
    if (p) Object.assign(preview.style, { left: `${p.l}px`, top: `${p.t}px`, width: `${p.w}px`, height: `${p.h}px` });
    preview.hidden = !p;
    preview.dataset.zone = target?.zone ?? "";
  };

  let cancelled = false;

  /** Remove the ghost and preview. Listeners stay until the button comes up. */
  const clear = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    ghost?.remove();
    preview?.remove();
    ghost = preview = null;
    document.body.classList.remove("dragging-session", "drop-refused");
    for (const [el, t] of titled) if (el.isConnected) el.setAttribute("title", t);
    titled.length = 0;
  };

  // The click that follows a drag's mouseup must not reach the row under it.
  const swallow = (c: Event) => c.stopPropagation();

  const up = (u: MouseEvent) => {
    window.removeEventListener("mousemove", move, true);
    window.removeEventListener("mouseup", up, true);
    window.removeEventListener("keydown", key, true);
    // A move still waiting for its frame decides the target.
    if (frame && active && !cancelled) paint();
    clear();
    if (!active && !cancelled) {
      o.click?.();
      return;
    }
    u.preventDefault();
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true), 0);
    if (!cancelled && target) o.drop(target);
  };

  const key = (k: KeyboardEvent) => {
    if (k.key !== "Escape" || !active) return;
    k.preventDefault();
    k.stopPropagation();
    cancelled = true;
    active = false;
    target = null;
    clear();
  };

  window.addEventListener("mousemove", move, true);
  window.addEventListener("mouseup", up, true);
  window.addEventListener("keydown", key, true);
}

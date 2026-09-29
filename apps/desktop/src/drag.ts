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
  drop: (t: DropTarget) => void;
  /** A press that never moved far enough: a plain click. */
  click?: () => void;
}

const THRESHOLD = 5;
/** Width of the rim, in px, where a drop wraps the whole layout. */
const RIM = 28;

function zoneAt(r: DOMRect, x: number, y: number): Zone {
  const fx = (x - r.left) / r.width;
  const fy = (y - r.top) / r.height;
  const d: [Zone, number][] = [["left", fx], ["right", 1 - fx], ["top", fy], ["bottom", 1 - fy]];
  d.sort((a, b) => a[1] - b[1]);
  return d[0][0];
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
  const sx = down.clientX;
  const sy = down.clientY;
  let active = false;
  let cells = new Map<string, DOMRect>();
  let rows: { id: string; rect: DOMRect }[] = [];
  let target: DropTarget | null = null;
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

  const move = (m: MouseEvent) => {
    if (cancelled) return;
    if (!active) {
      if (Math.abs(m.clientX - sx) + Math.abs(m.clientY - sy) < THRESHOLD) return;
      start();
    }
    m.preventDefault();
    ghost!.style.transform = `translate(${m.clientX + 12}px, ${m.clientY + 12}px)`;
    target = null;
    const row = rows.find(({ rect: r }) => m.clientX >= r.left && m.clientX < r.right && m.clientY >= r.top && m.clientY < r.bottom);
    if (row) {
      target = { session: null, zone: "center", group: row.id };
      Object.assign(preview!.style, { left: `${row.rect.left}px`, top: `${row.rect.top}px`, width: `${row.rect.width}px`, height: `${row.rect.height}px` });
      preview!.hidden = false;
      preview!.dataset.zone = "center";
      return;
    }
    const whole = cells.size > 0 && o.area ? o.area.getBoundingClientRect() : null;
    if (whole && m.clientX >= whole.left && m.clientX < whole.right && m.clientY >= whole.top && m.clientY < whole.bottom) {
      const d: [Zone, number][] = [["left", m.clientX - whole.left], ["right", whole.right - m.clientX - 1], ["top", m.clientY - whole.top], ["bottom", whole.bottom - m.clientY - 1]];
      d.sort((a, b) => a[1] - b[1]);
      if (d[0][1] < RIM) {
        target = { session: null, zone: d[0][0], outer: true };
        const p = previewRect(whole, target.zone);
        Object.assign(preview!.style, { left: `${p.l}px`, top: `${p.t}px`, width: `${p.w}px`, height: `${p.h}px` });
        preview!.hidden = false;
        preview!.dataset.zone = target.zone;
        return;
      }
    }
    for (const [session, r] of cells) {
      if (m.clientX >= r.left && m.clientX < r.right && m.clientY >= r.top && m.clientY < r.bottom) {
        target = { session, zone: zoneAt(r, m.clientX, m.clientY) };
        const p = previewRect(r, target.zone);
        Object.assign(preview!.style, { left: `${p.l}px`, top: `${p.t}px`, width: `${p.w}px`, height: `${p.h}px` });
        break;
      }
    }
    if (!target && cells.size === 0 && o.area) {
      const r = o.area.getBoundingClientRect();
      if (m.clientX >= r.left && m.clientX < r.right && m.clientY >= r.top && m.clientY < r.bottom) {
        target = { session: null, zone: "center" };
        Object.assign(preview!.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      }
    }
    preview!.hidden = !target;
    preview!.dataset.zone = target?.zone ?? "";
  };

  let cancelled = false;

  /** Remove the ghost and preview. Listeners stay until the button comes up. */
  const clear = () => {
    ghost?.remove();
    preview?.remove();
    ghost = preview = null;
    document.body.classList.remove("dragging-session");
    for (const [el, t] of titled) if (el.isConnected) el.setAttribute("title", t);
    titled.length = 0;
  };

  // The click that follows a drag's mouseup must not reach the row under it.
  const swallow = (c: Event) => c.stopPropagation();

  const up = (u: MouseEvent) => {
    window.removeEventListener("mousemove", move, true);
    window.removeEventListener("mouseup", up, true);
    window.removeEventListener("keydown", key, true);
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

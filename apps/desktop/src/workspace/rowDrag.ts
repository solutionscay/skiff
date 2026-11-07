/**
 * The mouse gesture of a row drag in the side panel (treeDrag.ts, skipperDrag.ts).
 * A press that does not move stays a click. Esc cancels. The caller decides where
 * a drag can land and what the release does.
 */
import { treeDrag } from "./tree";

interface RowDrag<D> {
  /** The element at the pointer, made when the drag starts. */
  ghost: () => HTMLElement;
  /** Where a drop at this point lands. Null: nowhere. */
  dropAt: (x: number, y: number) => D | null;
  /** Fills the ghost for this target, before it takes its place. */
  show?: (drop: D | null, ghost: HTMLElement) => void;
  /** Marks the target, and clears the last mark. */
  mark: (drop: D | null) => void;
  /** The mouse came up, after the cleanup. `dragged` is false for a plain click. */
  release: (drop: D | null, dragged: boolean) => void;
  /** Esc, after the cleanup. */
  cancel: () => void;
}

/** Starts listening at the press `down`. `source` is what the drag moves. */
export function rowDrag<D>(down: MouseEvent, source: HTMLElement, o: RowDrag<D>) {
  const sy = down.clientY;
  let drop: D | null = null;
  let ghost: HTMLElement | null = null;
  let active = false;

  const move = (m: MouseEvent) => {
    if (!active) {
      if (Math.abs(m.clientY - sy) < 5) return;
      active = treeDrag.active = true;
      source.classList.add("dragging");
      document.body.classList.add("dragging-tree");
      ghost = o.ghost();
      document.body.appendChild(ghost);
    }
    drop = o.dropAt(m.clientX, m.clientY);
    o.show?.(drop, ghost!);
    ghost!.style.left = `${m.clientX + 12}px`;
    ghost!.style.top = `${m.clientY - ghost!.offsetHeight / 2}px`;
    o.mark(drop);
  };
  const end = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    window.removeEventListener("keydown", esc, true);
    if (!active) return null;
    // The click that follows the release would act on the row.
    const swallow = (c: Event) => c.stopPropagation();
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true), 0);
    document.body.classList.remove("dragging-tree");
    source.classList.remove("dragging");
    ghost?.remove();
    const d = drop;
    drop = null;
    o.mark(null);
    active = treeDrag.active = false;
    return d;
  };
  const up = () => {
    const dragged = active;
    o.release(end(), dragged);
  };
  const esc = (k: KeyboardEvent) => {
    if (k.key !== "Escape") return;
    k.preventDefault();
    k.stopPropagation();
    end();
    o.cancel();
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  window.addEventListener("keydown", esc, true);
}

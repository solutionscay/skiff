/**
 * Drag a fleet or project Skipper's row in the Skippers list, with the mouse
 * logic of treeDrag.ts. A press that does not move stays a click. Over a row, the
 * top half drops the drag before it and the bottom half after it. The Skiff agent's
 * row stays first: over it, the drag lands right under it. Below the last row, the
 * drag lands last. Esc cancels.
 */
import { render } from "../app/renderRequest";
import { hostCan } from "../app/hostCaps";
import { h } from "../ui/dom";
import { moveSkipper } from "./skipperOrder";
import { treeDrag } from "./tree";

const MARKS = ["drop-before", "drop-after"];

type Drop = { anchor: string; side: "before" | "after"; el: HTMLElement; mark: string };

export function dragSkipper(down: MouseEvent, source: HTMLElement, scope: string, name: string) {
  if (down.button !== 0 || !hostCan("skipper_config")) return;
  const list = source.parentElement!;
  const rows = () => [...list.querySelectorAll<HTMLElement>(".sk-row")];
  const movable = () => rows().filter((r) => r.dataset.scope);
  if (movable().length < 2) return;
  const sy = down.clientY;
  let drop: Drop | null = null;
  let ghost: HTMLElement | null = null;
  let active = false;

  function dropAt(x: number, y: number): Drop | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("#sidebar-scroll .sk-row");
    if (!el) {
      // The empty part of the list, below the last row.
      const last = movable().pop()!;
      const box = list.getBoundingClientRect();
      if (last === source || x < box.left || x > box.right || y < last.getBoundingClientRect().bottom || y > box.bottom) return null;
      return { anchor: last.dataset.scope!, side: "after", el: last, mark: "drop-after" };
    }
    if (el === source) return null;
    const anchor = el.dataset.scope;
    if (!anchor) {
      const first = movable()[0];
      if (first === source) return null;
      return { anchor: first.dataset.scope!, side: "before", el, mark: "drop-after" };
    }
    const box = el.getBoundingClientRect();
    const side = y < box.top + box.height / 2 ? "before" : "after";
    return { anchor, side, el, mark: `drop-${side}` };
  }

  function mark() {
    for (const el of list.querySelectorAll(".drop-before, .drop-after")) el.classList.remove(...MARKS);
    if (drop) drop.el.classList.add(drop.mark);
  }

  const move = (m: MouseEvent) => {
    if (!active) {
      if (Math.abs(m.clientY - sy) < 5) return;
      active = treeDrag.active = true;
      source.classList.add("dragging");
      document.body.classList.add("dragging-tree");
      ghost = h("div", "tree-ghost");
      ghost.append(h("span", "tree-ghost-name", name));
      document.body.appendChild(ghost);
    }
    drop = dropAt(m.clientX, m.clientY);
    ghost!.style.left = `${m.clientX + 12}px`;
    ghost!.style.top = `${m.clientY - ghost!.offsetHeight / 2}px`;
    mark();
  };
  const end = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    window.removeEventListener("keydown", esc, true);
    if (!active) return null;
    // The click that follows the release would open the row's pane.
    const swallow = (c: Event) => c.stopPropagation();
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true), 0);
    document.body.classList.remove("dragging-tree");
    source.classList.remove("dragging");
    ghost?.remove();
    const d = drop;
    drop = null;
    mark();
    active = treeDrag.active = false;
    return d;
  };
  const up = () => {
    const wasActive = active;
    const d = end();
    if (d) {
      // A drop next to the row's own place changes nothing, so nothing is stored.
      const order = movable();
      const at = order.indexOf(source);
      const to = order.findIndex((r) => r.dataset.scope === d.anchor) + (d.side === "after" ? 1 : 0);
      if (to !== at && to !== at + 1) return void moveSkipper(scope, d.anchor, d.side);
    }
    if (wasActive) render("skippers-order");
  };
  const esc = (k: KeyboardEvent) => {
    if (k.key !== "Escape") return;
    k.preventDefault();
    k.stopPropagation();
    end();
    render("skippers-order");
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  window.addEventListener("keydown", esc, true);
}

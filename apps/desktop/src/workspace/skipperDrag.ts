/**
 * Drag a fleet or project Skipper's row in the Skippers list, with the gesture of
 * rowDrag.ts. Over a row, the top half drops the drag before it and the bottom half
 * after it. The Skiff agent's row stays first: over it, the drag lands right under
 * it. Below the last row, the drag lands last.
 */
import { render } from "../app/renderRequest";
import { hostCan } from "../app/hostCaps";
import { h } from "../ui/dom";
import { rowDrag } from "./rowDrag";
import { moveSkipper } from "./skipperOrder";

const MARKS = ["drop-before", "drop-after"];

type Drop = { anchor: string; side: "before" | "after"; el: HTMLElement; mark: string };

export function dragSkipper(down: MouseEvent, source: HTMLElement, scope: string, name: string) {
  if (down.button !== 0 || !hostCan("skipper_config")) return;
  const list = source.parentElement!;
  const rows = () => [...list.querySelectorAll<HTMLElement>(".sk-row")];
  const movable = () => rows().filter((r) => r.dataset.scope);
  if (movable().length < 2) return;

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

  rowDrag<Drop>(down, source, {
    ghost() {
      const ghost = h("div", "tree-ghost");
      ghost.append(h("span", "tree-ghost-name", name));
      return ghost;
    },
    dropAt,
    mark(drop) {
      for (const el of list.querySelectorAll(".drop-before, .drop-after")) el.classList.remove(...MARKS);
      if (drop) drop.el.classList.add(drop.mark);
    },
    release(drop, dragged) {
      if (drop) {
        // A drop next to the row's own place changes nothing, so nothing is stored.
        const order = movable();
        const at = order.indexOf(source);
        const to = order.findIndex((r) => r.dataset.scope === drop.anchor) + (drop.side === "after" ? 1 : 0);
        if (to !== at && to !== at + 1) return void moveSkipper(scope, drop.anchor, drop.side);
      }
      if (dragged) render("skippers-order");
    },
    cancel: () => render("skippers-order"),
  });
}

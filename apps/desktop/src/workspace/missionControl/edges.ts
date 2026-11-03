/**
 * The edge ticks: the amber marks at the board edge for a waiting card out of view.
 * One button per waiting card, keyed by card id. A pan or zoom only moves or hides a
 * button; the set of buttons changes only when the set of waiting cards changes.
 */
import { h } from "../../ui/dom";

/** A waiting card: its id, its title, and where its tick sits (null: in view). */
export type Edge = { id: string; title: string; at: { x: number; y: number } | null };

const ticks = new Map<string, HTMLButtonElement>();
let layer: HTMLElement | null = null;

export function mountEdges(el: HTMLElement) {
  layer = el;
}

export function drawEdges(edges: Edge[]) {
  if (!layer) return;
  const live = new Set(edges.map((e) => e.id));
  for (const [id, b] of ticks)
    if (!live.has(id)) {
      b.remove();
      ticks.delete(id);
    }
  for (const e of edges) {
    let b = ticks.get(e.id);
    if (!b) {
      b = h("button", "mission-tick");
      b.type = "button";
      b.tabIndex = -1;
      b.dataset.id = e.id;
      ticks.set(e.id, b);
      layer.appendChild(b);
    }
    if (b.title !== e.title) b.title = e.title;
    b.hidden = !e.at;
    if (e.at) {
      b.style.left = `${e.at.x}px`;
      b.style.top = `${e.at.y}px`;
    }
  }
}

export function clearEdges() {
  for (const b of ticks.values()) b.remove();
  ticks.clear();
}

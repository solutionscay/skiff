/** Movement of ledger rows after a redraw. Transform and opacity only. */
import { fade, move } from "../ui/motion";

const moving = new WeakMap<HTMLElement, Animation>();

/**
 * Moves the rows of `list` from their places in `before`: a row that stays
 * slides from its old place, a new row fades in, a row that left fades out.
 */
export function animate(list: HTMLElement, before: Map<string, { top?: number; el: HTMLElement }>) {
  const after = new Set<string>();
  const rows = [...list.querySelectorAll<HTMLElement>(".lg-row[data-key]")];
  // Finish every layout read before an animation starts.
  const tops = new Map(rows.filter((r) => before.get(r.dataset.key!)?.top !== undefined).map((r) => [r, r.offsetTop]));
  for (const r of rows) {
    const k = r.dataset.key!;
    after.add(k);
    const b = before.get(k);
    if (!b) {
      r.animate([{ opacity: 0, offset: 0 }], fade());
      continue;
    }
    const dy = b.top === undefined ? 0 : b.top - tops.get(r)!;
    if (!dy) continue;
    moving.get(r)?.cancel();
    moving.set(r, r.animate([{ transform: `translateY(${dy}px)` }, { transform: "none" }], move()));
  }
  for (const [k, b] of before) {
    if (after.has(k) || b.top === undefined) continue;
    const g = b.el.cloneNode(true) as HTMLElement;
    g.classList.remove("flash");
    g.classList.add("ghost");
    g.removeAttribute("data-key");
    g.style.top = `${b.top}px`;
    list.append(g);
    g.animate([{ opacity: 0, offset: 1 }], fade()).finished.finally(() => g.remove());
  }
}

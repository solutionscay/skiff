/** Movement of ledger rows after a redraw. */
const motions = new WeakMap<HTMLElement, object>();

export const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Moves the rows of `list` from their places in `before`: a row that stays
 * slides from its old place, a row with a new look flashes, a new row fades
 * in, a row that left fades up and out. A row that moved into Finished
 * slides slower and does not flash.
 */
export function animate(list: HTMLElement, before: Map<string, { top?: number; el: HTMLElement }>) {
  const after = new Set<string>();
  const rows = [...list.querySelectorAll<HTMLElement>(".lg-row[data-key]")];
  // Finish every layout read before an animation class or transform is written.
  const tops = new Map(rows.filter((r) => before.get(r.dataset.key!)?.top !== undefined).map((r) => [r, r.offsetTop]));
  for (const r of rows) {
    const k = r.dataset.key!;
    after.add(k);
    const b = before.get(k);
    if (!b) {
      r.classList.add("enter");
      continue;
    }
    const intoFin = r.classList.contains("fin") && !b.el.classList.contains("fin");
    if (b.el.dataset.look !== r.dataset.look && !intoFin) r.classList.add("flash");
    const dy = b.top === undefined ? 0 : b.top - tops.get(r)!;
    if (!dy) continue;
    const motion = {};
    motions.set(r, motion);
    r.classList.remove("moving", "long");
    r.style.transform = `translateY(${dy}px)`;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (!r.isConnected || motions.get(r) !== motion) return;
        r.classList.add("moving");
        if (intoFin) r.classList.add("long");
        r.style.transform = "";
      }),
    );
  }
  for (const [k, b] of before) {
    if (after.has(k) || b.top === undefined) continue;
    const g = b.el.cloneNode(true) as HTMLElement;
    g.classList.remove("moving", "long", "enter", "flash");
    g.classList.add("ghost");
    g.removeAttribute("data-key");
    g.style.top = `${b.top}px`;
    g.style.transform = "";
    list.append(g);
    requestAnimationFrame(() => requestAnimationFrame(() => g.classList.add("out")));
    setTimeout(() => g.remove(), 420);
  }
}

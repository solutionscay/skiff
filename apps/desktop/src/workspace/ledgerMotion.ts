/** Movement of ledger rows after a redraw. */
export const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Moves the rows of `list` from their places in `before`: a row that stays
 * slides from its old place, a row with a new look flashes, a new row fades
 * in, a row that left fades up and out. A row that moved into Finished
 * slides slower and does not flash.
 */
export function animate(list: HTMLElement, before: Map<string, { top: number; el: HTMLElement }>) {
  const after = new Set<string>();
  for (const r of list.querySelectorAll<HTMLElement>(".lg-row[data-key]")) {
    const k = r.dataset.key!;
    after.add(k);
    const b = before.get(k);
    if (!b) {
      r.classList.add("enter");
      continue;
    }
    const intoFin = r.classList.contains("fin") && !b.el.classList.contains("fin");
    if (b.el.dataset.look !== r.dataset.look && !intoFin) r.classList.add("flash");
    const dy = b.top - r.offsetTop;
    if (!dy) continue;
    r.style.transform = `translateY(${dy}px)`;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        r.classList.add("moving");
        if (intoFin) r.classList.add("long");
        r.style.transform = "";
      }),
    );
  }
  for (const [k, b] of before) {
    if (after.has(k)) continue;
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

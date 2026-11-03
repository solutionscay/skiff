/**
 * Mission control's connectors on the board: one solid path per link, keyed
 * by `from>to`, so a draw that changes nothing leaves them be. Three layers
 * in the strings' SVG: idle and done under working, working under waiting, so
 * where connectors share a run the strongest state shows. grid.ts gives the
 * paths.
 */
const SVG = "http://www.w3.org/2000/svg";

export interface Link {
  key: string;
  d: string;
  state: "working" | "waiting" | "idle" | "done";
  dim: boolean;
}

const layer = () => document.createElementNS(SVG, "g");
const layers = { idle: layer(), working: layer(), waiting: layer() };
const paths = new Map<string, SVGPathElement>();

/** Puts the layers in the strings' SVG, once. */
export function mountConnectors(svg: SVGSVGElement) {
  svg.append(layers.idle, layers.working, layers.waiting);
}

/** Draws `links`: new ones added, changed ones updated, gone ones removed. */
export function drawConnectors(links: Link[]) {
  const seen = new Set<string>();
  for (const l of links) {
    seen.add(l.key);
    let p = paths.get(l.key);
    if (!p) {
      p = document.createElementNS(SVG, "path");
      paths.set(l.key, p);
    }
    const into = l.state === "waiting" || l.state === "working" ? layers[l.state] : layers.idle;
    if (p.parentNode !== into) into.appendChild(p);
    const cls = `${l.state}${l.dim ? " dim" : ""}`;
    if (p.getAttribute("class") !== cls) p.setAttribute("class", cls);
    if (p.getAttribute("d") !== l.d) p.setAttribute("d", l.d);
  }
  for (const [key, p] of paths) {
    if (seen.has(key)) continue;
    p.remove();
    paths.delete(key);
  }
}

export function clearConnectors() {
  for (const p of paths.values()) p.remove();
  paths.clear();
}

import type { Layout } from "../platform/types";
import { rects } from "./layout";



/** Small outline of the layout for the group list. */
export function glyph(l: Layout, w = 16, h = 12): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("width", String(w));
  svg.setAttribute("height", String(h));
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("glyph");
  for (const r of rects(l, 0, 0, w, h)) {
    const e = document.createElementNS(ns, "rect");
    e.setAttribute("x", String(r.x + 0.5));
    e.setAttribute("y", String(r.y + 0.5));
    e.setAttribute("width", String(Math.max(0, r.w - 1)));
    e.setAttribute("height", String(Math.max(0, r.h - 1)));
    svg.appendChild(e);
  }
  return svg;
}

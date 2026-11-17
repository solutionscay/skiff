/**
 * The usage ring (#276): a 13px circle, stroke 2.5, square ends. The track is
 * `--rule-strong`; the arc is the share used, in its level's color.
 */
import { svg } from "./icons";

const R = 4.75;
const C = 2 * Math.PI * R;

export function ring(percent: number, level: "ok" | "near" | "limit", size = 13): SVGSVGElement {
  const arc = (Math.min(Math.max(percent, 0), 100) / 100) * C;
  const color = level === "limit" ? "var(--error)" : level === "near" ? "var(--state-waiting)" : "var(--text-3)";
  return svg(
    `<svg width="${size}" height="${size}" viewBox="0 0 13 13" aria-hidden="true">` +
      `<circle cx="6.5" cy="6.5" r="${R}" fill="none" stroke="var(--rule-strong)" stroke-width="2.5"/>` +
      `<circle cx="6.5" cy="6.5" r="${R}" fill="none" stroke="${color}" stroke-width="2.5" stroke-linecap="butt" stroke-dasharray="${arc.toFixed(2)} ${C.toFixed(2)}" transform="rotate(-90 6.5 6.5)"/>` +
      `</svg>`,
  );
}

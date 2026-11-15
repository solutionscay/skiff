/**
 * Mission control's connectors on the board: one path per link, keyed
 * by `from>to`, so a draw that changes nothing leaves them be. Three layers
 * in the strings' SVG: idle and done under working, working under waiting, so
 * where connectors share a run the strongest state shows. grid.ts gives the
 * paths.
 *
 * A line is solid while work runs on the child's side, and dotted while the
 * parent waits or holds a result it has not reported. When a result arrives,
 * one dot runs once along the line from the child to the parent. Reduced
 * motion: no run.
 */
import { move, still } from "../../ui/motion";

const SVG = "http://www.w3.org/2000/svg";

export interface Link {
  key: string;
  d: string;
  state: "working" | "waiting" | "idle" | "done";
  /** `solid`: work runs on the child's side. `wait`: the parent waits. `result`: a result the parent has not reported. */
  line: "solid" | "wait" | "result";
  dim: boolean;
}

const layer = () => document.createElementNS(SVG, "g");
const layers = { idle: layer(), working: layer(), waiting: layer(), pulse: layer() };
const paths = new Map<string, SVGPathElement>();
/** Each link's line at the last draw, to see a result arrive. */
const lines = new Map<string, Link["line"]>();
/** The pulse's time: a few moves, so the dot reads on a long line. */
const PULSE_MOVES = 5;
/** The points the dot passes on its way, child to parent. */
const PULSE_STEPS = 24;

/** Puts the layers in the strings' SVG, once. */
export function mountConnectors(svg: SVGSVGElement) {
  svg.append(layers.idle, layers.working, layers.waiting, layers.pulse);
}

/** One dot runs once along `p` from its end, the child, to its start, the parent, and goes. */
function pulse(p: SVGPathElement, state: Link["state"]) {
  if (still() || !p.isConnected) return;
  const len = p.getTotalLength();
  if (!len) return;
  const dot = document.createElementNS(SVG, "circle");
  dot.setAttribute("r", "3.5");
  dot.setAttribute("class", `pulse ${state}`);
  layers.pulse.appendChild(dot);
  const frames: Keyframe[] = [];
  for (let i = 0; i <= PULSE_STEPS; i++) {
    const at = p.getPointAtLength(len * (1 - i / PULSE_STEPS));
    frames.push({ transform: `translate(${at.x}px, ${at.y}px)`, opacity: i === PULSE_STEPS ? 0 : 1 });
  }
  const timing = move();
  const run = dot.animate(frames, { ...timing, duration: (timing.duration as number) * PULSE_MOVES, easing: "linear" });
  const end = () => dot.remove();
  run.onfinish = end;
  run.oncancel = end;
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
    const cls = `${l.state}${l.line === "solid" ? "" : " dotted"}${l.dim ? " dim" : ""}`;
    if (p.getAttribute("class") !== cls) p.setAttribute("class", cls);
    if (p.getAttribute("d") !== l.d) p.setAttribute("d", l.d);
    // A result arrives: the line was there, and not yet a result. A board that opens on a result does not pulse.
    const was = lines.get(l.key);
    lines.set(l.key, l.line);
    if (l.line === "result" && was && was !== "result" && !l.dim) pulse(p, l.state);
  }
  for (const [key, p] of paths) {
    if (seen.has(key)) continue;
    p.remove();
    paths.delete(key);
    lines.delete(key);
  }
}

export function clearConnectors() {
  for (const p of paths.values()) p.remove();
  paths.clear();
  lines.clear();
  layers.pulse.replaceChildren();
}

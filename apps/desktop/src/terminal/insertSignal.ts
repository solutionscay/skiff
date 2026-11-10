/**
 * The signal of a typed image path: the cursor row gets a green tint for 1 s.
 * Skiff does not read the agent's input. The tint belongs to the buffer row,
 * so an agent that redraws "[Image #N]" in place keeps it, and a clear or a
 * trim of the row takes it away at once.
 */
import type { Terminal } from "@xterm/xterm";

/** `color` at `alpha` over `bg`, as #rrggbb: xterm takes no other format. */
function mix(color: string, alpha: number, bg: string): string {
  const c = document.createElement("canvas");
  c.width = c.height = 1;
  const g = c.getContext("2d", { willReadFrequently: true });
  if (!g) return bg;
  g.fillStyle = bg;
  g.fillRect(0, 0, 1, 1);
  g.globalAlpha = alpha;
  g.fillStyle = color;
  g.fillRect(0, 0, 1, 1);
  const [r, gr, b] = g.getImageData(0, 0, 1, 1).data;
  return "#" + [r, gr, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}

/** One row of `--diff-add` at `alpha` under the text, on `marker`'s row. */
function tint(term: Terminal, marker: ReturnType<Terminal["registerMarker"]>, alpha: number, el: HTMLElement) {
  if (!marker || marker.isDisposed) return undefined;
  const add = getComputedStyle(el).getPropertyValue("--diff-add").trim() || "#7fd08a";
  const bg = term.options.theme?.background ?? getComputedStyle(el).backgroundColor;
  return term.registerDecoration({ marker, width: term.cols, layer: "bottom", backgroundColor: mix(add, alpha, bg) });
}

/**
 * Marks the cursor row before the paste: 18% for 1 s. Then the tint and the
 * marker go at once: a terminal row has no fade.
 */
export function signalLine(term: Terminal, el: HTMLElement) {
  const marker = term.registerMarker(0);
  const full = tint(term, marker, 0.18, el);
  if (!marker || !full) return marker?.dispose();
  window.setTimeout(() => {
    full.dispose();
    marker.dispose();
  }, 1000);
}

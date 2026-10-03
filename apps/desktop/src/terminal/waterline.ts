/**
 * The waterline: where the user last read in a pane.
 *
 * When a pane loses the keys, its screen rows are kept as hashes. When the
 * user comes back, the rows are found again by their text, once, and one
 * line goes above the first new row. After that the line follows its row and
 * never moves on its own. Input to the pane removes it.
 *
 * Rules that keep it honest:
 * - One search per arrival. If the terminal resets and the row is gone, the
 *   line goes too. It is never searched for again.
 * - No line on the alternate screen. A full-screen program redraws its view
 *   on every frame, so no row there stays put.
 * - A line needs at least MIN_NEW rows of text that the old screen did not
 *   show. A spinner tick or a status line change is not news.
 * - No match means no line: a wrong line is worse than none.
 */
import type { IBuffer, IMarker } from "@xterm/xterm";

import { sessions } from "../app/state";
import { h } from "../ui/dom";
import { type Pane, panes } from "./terminalState";

interface Mark {
  /** One hash per screen row when the pane lost the keys. 0 is a blank row. */
  rows: number[];
  /** Unix ms when the pane lost the keys. */
  at: number;
}

interface Line {
  el: HTMLElement;
  label: HTMLElement;
  /** The first new row. It follows the row as history scrolls. */
  marker?: IMarker;
  /** The pane was parked or is new: its rows are old until the snapshot is written. */
  wait: boolean;
  frame: number;
}

const KEY = "skiff.waterline";
/** Fewer matching rows than this could be a chance match. */
const MIN_ROWS = 2;
/** Fewer new rows of text than this is a redraw, not output. */
const MIN_NEW = 2;

const marks = new Map<string, Mark>(load());
/** Lines on screen, by session. A session has one while its mark waits for input. */
const lines = new Map<string, Line>();

function load(): [string, Mark][] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (!v || typeof v !== "object") return [];
    return Object.entries(v as Record<string, Mark & { alt?: boolean }>)
      .filter(([, m]) => Array.isArray(m?.rows) && typeof m.at === "number" && !m.alt)
      .map(([id, m]) => [id, { rows: m.rows, at: m.at }]);
  } catch {
    return [];
  }
}

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(Object.fromEntries([...marks].filter(([id]) => sessions.has(id)))));
  } catch {
    /* the marks last for this run */
  }
}

/** FNV-1a over the row's text, without the blanks at its end. Hashes, not text, go to storage. */
function rowHash(buffer: IBuffer, y: number): number {
  const text = buffer.getLine(y)?.translateToString(true) ?? "";
  if (!text.trim()) return 0;
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) hash = Math.imul(hash ^ text.charCodeAt(i), 0x01000193);
  return hash >>> 0 || 1;
}

function hashes(buffer: IBuffer, from: number, count: number): number[] {
  const out = new Array<number>(count);
  for (let i = 0; i < count; i++) out[i] = rowHash(buffer, from + i);
  return out;
}

/** How many rows of `now`, from `p`, equal the rows of `then`, from `s`. The second value counts those with text. */
function run(now: number[], p: number, then: number[], s: number): [number, number] {
  let n = 0;
  let text = 0;
  while (p + n < now.length && s + n < then.length && now[p + n] === then[s + n]) {
    if (now[p + n]) text++;
    n++;
  }
  return [n, text];
}

/**
 * The normal buffer keeps every row at its place. The old screen is found
 * from its first row, and the line goes where the buffer stops matching it.
 * Returns the buffer row of the first new row, or -1 when nothing is new.
 */
export function firstNewRow(now: number[], then: number[]): number {
  const anchor = then.findIndex((x) => x !== 0);
  if (anchor < 0) return -1;
  let best = -1;
  let bestText = MIN_ROWS - 1;
  // From the bottom: of two equal matches, the later one is where the user was.
  for (let p = now.length - 1; p >= anchor; p--) {
    if (now[p] !== then[anchor]) continue;
    const [n, text] = run(now, p - anchor, then, 0);
    if (n > anchor && text > bestText) {
      bestText = text;
      best = p - anchor + n;
    }
  }
  if (best < 0 || best >= now.length) return -1;
  // Rows the old screen already showed, such as an agent's input box that
  // moved down, are not new. Only text the user has not seen counts.
  const seen = new Set(then);
  let fresh = 0;
  for (let i = best; i < now.length && fresh < MIN_NEW; i++) if (now[i] && !seen.has(now[i])) fresh++;
  return fresh >= MIN_NEW ? best : -1;
}

/** The pane lost the keys: remember what its screen showed. */
export function leave(id: string) {
  const pane = panes.get(id);
  remove(id);
  if (!pane || pane.parked) return;
  const buffer = pane.term.buffer.active;
  const rows = buffer.type === "alternate" ? [] : hashes(buffer, buffer.baseY, pane.term.rows);
  if (rows.filter(Boolean).length < MIN_ROWS) {
    marks.delete(id);
  } else {
    marks.set(id, { rows, at: Date.now() });
  }
  save();
}

/** The pane is on screen again: show where the user stopped reading. */
export function arrive(id: string) {
  if (!marks.has(id) || lines.has(id)) return;
  const pane = panes.get(id);
  if (!pane) return;
  const el = h("div", "waterline");
  const label = h("span", "waterline-label");
  el.appendChild(label);
  el.hidden = true;
  pane.el.appendChild(el);
  lines.set(id, { el, label, wait: pane.parked || pane.wrote !== pane.stream, frame: 0 });
  update(id);
}

/** Output of the pane's current stream was written. The first write of a stream is the snapshot. */
export function ready(id: string) {
  const line = lines.get(id);
  if (!line?.wait) return;
  line.wait = false;
  update(id);
}

/** Input to the pane: the user reads from here now. */
export function clear(id: string) {
  if (!marks.delete(id) && !lines.has(id)) return;
  remove(id);
  save();
}

/** The session is gone. */
export function forget(id: string) {
  clear(id);
}

function remove(id: string) {
  const line = lines.get(id);
  if (!line) return;
  cancelAnimationFrame(line.frame);
  line.marker?.dispose();
  line.el.remove();
  lines.delete(id);
}

/** The terminal drew or scrolled: place the line again, once per frame. */
export function update(id: string) {
  const line = lines.get(id);
  if (!line || line.frame) return;
  line.frame = requestAnimationFrame(() => {
    line.frame = 0;
    const pane = panes.get(id);
    const mark = marks.get(id);
    if (!pane || !mark || lines.get(id) !== line) return;
    if (!place(pane, mark, line)) clear(id);
  });
}

function age(at: number): string {
  const min = Math.floor((Date.now() - at) / 60_000);
  if (min < 1) return "now";
  if (min < 60) return `${min}m`;
  const hours = Math.floor(min / 60);
  return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}

/** Draws the line, or hides it while its row is out of view. False when the line has nothing to show. */
function place(pane: Pane, mark: Mark, line: Line): boolean {
  const term = pane.term;
  const buffer = term.buffer.active;
  const screen = pane.el.querySelector<HTMLElement>(".xterm-screen");
  line.el.hidden = true;
  if (!screen || pane.parked || line.wait) return true;
  if (!line.marker) {
    if (buffer.type === "alternate") return false;
    const at = firstNewRow(hashes(buffer, 0, buffer.length), mark.rows);
    if (at < 0) return false;
    line.marker = term.registerMarker(at - (buffer.baseY + buffer.cursorY));
  }
  // A reset or a cleared history took the row away. The line goes with it.
  if (line.marker.isDisposed || buffer.type === "alternate") return false;
  const row = line.marker.line - buffer.viewportY;
  // The row is above or below the view.
  if (row <= 0 || row >= term.rows) return true;
  const box = pane.el.getBoundingClientRect();
  const view = screen.getBoundingClientRect();
  const y = view.top - box.top + (view.height / term.rows) * row;
  line.el.hidden = false;
  line.el.style.setProperty("--wl-bg", term.options.theme?.background ?? "");
  line.el.style.top = `${y}px`;
  line.label.textContent = `since you left · ${age(mark.at)}`;
  return true;
}

/**
 * The waterline: where the user last read in a pane.
 *
 * When a pane loses the keys, its screen rows are kept as hashes. When the
 * user comes back, the rows are found again by their text, and one line goes
 * above the first row that changed. The rows above it are dim. The first key
 * in the pane removes the line.
 *
 * The match is by text, not by row number. An agent in full-screen mode owns
 * its scrolling, a parked pane redraws from a snapshot, and both move rows.
 * No match means no line: a wrong line is worse than none.
 */
import type { IBuffer, IMarker } from "@xterm/xterm";

import { sessions } from "../app/state";
import { h } from "../ui/dom";
import { type Pane, panes } from "./terminalState";

interface Mark {
  /** One hash per screen row when the pane lost the keys. 0 is a blank row. */
  rows: number[];
  /** The program drew on the alternate screen: it owns the scrolling. */
  alt: boolean;
  /** Unix ms when the pane lost the keys. */
  at: number;
}

interface Line {
  el: HTMLElement;
  dim: HTMLElement;
  rule: HTMLElement;
  label: HTMLElement;
  /** The first new row in the normal buffer. It follows the row as history scrolls. */
  marker?: IMarker;
  /** The pane was parked or is new: its rows are old until the snapshot is written. */
  wait: boolean;
  /** The rows were compared with the mark once. Text that comes after that is not "since you left". */
  searched: boolean;
  frame: number;
}

const KEY = "skiff.waterline";
/** Fewer matching rows than this could be a chance match. */
const MIN_ROWS = 2;

const marks = new Map<string, Mark>(load());
/** Lines on screen, by session. A session has one while its mark waits for a key. */
const lines = new Map<string, Line>();

function load(): [string, Mark][] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (!v || typeof v !== "object") return [];
    return Object.entries(v as Record<string, Mark>).filter(([, m]) => Array.isArray(m?.rows) && typeof m.at === "number");
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
 * Returns the buffer row of the first new row, or -1.
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
  // The whole old screen is still there, with nothing after it: nothing is new.
  return best >= 0 && best < now.length ? best : -1;
}

/**
 * The alternate screen is one view that the program scrolls. The old rows
 * can be anywhere in it, or gone. The line goes under the highest run of old
 * rows that ends inside the view. Returns the view row of the first new row, or -1.
 */
export function firstNewRowInView(now: number[], then: number[]): number {
  for (let p = 0; p < now.length; p++) {
    if (!now[p]) continue;
    let end = -1;
    for (let s = 0; s < then.length; s++) {
      if (then[s] !== now[p]) continue;
      const [n, text] = run(now, p, then, s);
      // A run to the bottom of the view is the program's own frame, or shows no edge.
      if (text >= MIN_ROWS && p + n < now.length) end = Math.max(end, p + n);
    }
    if (end >= 0) return end;
  }
  return -1;
}

/** The pane lost the keys: remember what its screen showed. */
export function leave(id: string) {
  const pane = panes.get(id);
  remove(id);
  if (!pane || pane.parked) return;
  const buffer = pane.term.buffer.active;
  const rows = hashes(buffer, buffer.baseY, pane.term.rows);
  if (rows.filter(Boolean).length < MIN_ROWS) {
    marks.delete(id);
  } else {
    marks.set(id, { rows, alt: buffer.type === "alternate", at: Date.now() });
  }
  save();
}

/** The pane has the keys again: show where the user stopped reading. */
export function arrive(id: string) {
  if (!marks.has(id) || lines.has(id)) return;
  const pane = panes.get(id);
  if (!pane) return;
  const el = h("div", "waterline");
  const dim = h("div", "waterline-dim");
  const rule = h("div", "waterline-rule");
  const label = h("span", "waterline-label");
  rule.appendChild(label);
  el.append(dim, rule);
  el.hidden = true;
  pane.el.appendChild(el);
  lines.set(id, { el, dim, rule, label, wait: pane.parked || pane.wrote !== pane.stream, searched: false, frame: 0 });
  update(id);
}

/** Output of the pane's current stream was written. The first write of a stream is the snapshot. */
export function ready(id: string) {
  const line = lines.get(id);
  if (!line?.wait) return;
  line.wait = false;
  update(id);
}

/** A key in the pane: the user reads from here now. */
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
    // Nothing changed while the user was away: there is no line to draw.
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

/** Draws the line, or hides it while its row is out of view. False when the mark has nothing to show. */
function place(pane: Pane, mark: Mark, line: Line): boolean {
  const term = pane.term;
  const buffer = term.buffer.active;
  const screen = pane.el.querySelector<HTMLElement>(".xterm-screen");
  line.el.hidden = true;
  if (!screen || pane.parked || line.wait) return true;
  if ((buffer.type === "alternate") !== mark.alt) return false;
  /** The view row of the first new row. Above the view: nothing to dim. Under it: all rows are old. */
  let row: number;
  if (mark.alt) {
    // The program scrolls its own view, so the old rows are looked for on every draw.
    const now = hashes(buffer, 0, term.rows);
    if (!line.searched) {
      line.searched = true;
      if (now.length === mark.rows.length && now.every((x, i) => x === mark.rows[i])) return false;
    }
    row = firstNewRowInView(now, mark.rows);
  } else {
    if (!line.searched) {
      line.searched = true;
      const at = firstNewRow(hashes(buffer, 0, buffer.length), mark.rows);
      if (at < 0) return false;
      const marker = term.registerMarker(at - (buffer.baseY + buffer.cursorY));
      line.marker = marker;
      // A new snapshot resets the terminal and its markers. The rows are found again by their text.
      marker.onDispose(() => {
        if (line.marker === marker) line.searched = false;
      });
    }
    if (!line.marker || line.marker.isDisposed) return true;
    row = line.marker.line - buffer.viewportY;
    // The user scrolled up into rows they have read: all of the view is old.
    if (row > term.rows) row = term.rows;
  }
  if (row <= 0) return true;
  const box = pane.el.getBoundingClientRect();
  const view = screen.getBoundingClientRect();
  const y = view.top - box.top + (view.height / term.rows) * row;
  line.el.hidden = false;
  line.el.style.setProperty("--wl-bg", term.options.theme?.background ?? "");
  line.dim.style.height = `${y}px`;
  line.rule.style.top = `${y}px`;
  line.rule.hidden = row >= term.rows;
  line.label.textContent = `since you left · ${age(mark.at)}`;
  return true;
}

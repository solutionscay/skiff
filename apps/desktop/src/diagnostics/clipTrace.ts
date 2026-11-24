/**
 * Clipboard trace (#280). While the latency trace runs (SKIFF_TRACE, or
 * Help → Start latency trace), each copy, paste and cleared selection
 * appends a `clip` line to the same file. A line holds lengths and short
 * hashes seeded per launch, never the text. When the trace is off, each hook returns at once.
 *
 *   {"clip":"copy","route":"menu","sid":…,"remote":true,"len":12,"hash":"…",
 *    "mouse":"none","path":"gtk","failed":[],"ok":true}
 *   {"clip":"readback","sid":…,"after":0,"len":12,"hash":"…","same":true}
 *   {"clip":"paste","route":"key","sid":…,"remote":true,"len":12,"path":"gtk","ok":true}
 *   {"clip":"cleared","sid":…,"reason":"snapshot","ago":40}
 *
 * `route`: key (a key the page saw), menu (the menu bar or its accelerator:
 * on Linux the copy key arrives this way), osc52 (a program in the pane),
 * peek (a preview terminal), other (the palette, or no route noted).
 */
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import type { ClipTry } from "../platform/clipboard";
import { isRemote } from "../app/hosts";
import { tracing, traceLine } from "./latency";

type ClipRoute = "key" | "menu" | "osc52" | "peek" | "other";

/** The route of the copy or paste that runs next, noted by the key or menu handler. */
let routeNote: { route: ClipRoute; at: number } | null = null;

export function noteRoute(route: ClipRoute) {
  if (tracing()) routeNote = { route, at: performance.now() };
}

function takeRoute(): ClipRoute {
  const n = routeNote;
  routeNote = null;
  // A note older than this belongs to some other action.
  return n && performance.now() - n.at < 500 ? n.route : "other";
}

/** A random seed for this launch, kept in memory only: a hash in the file
 *  compares texts within one run, but cannot be used to guess a short text. */
const SEED = crypto.getRandomValues(new Uint32Array(1))[0];

/** FNV-1a, 32 bits, seeded, in hex: enough to tell two texts apart in a trace. */
function shortHash(s: string): string {
  let h = (0x811c9dc5 ^ SEED) >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}

const chars = (s: string) => [...s].length;
const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** A copy is about to run. Returns the try record and a callback for its end, or nothing when the trace is off. */
export function traceCopy(
  sid: string | null,
  term: Terminal | undefined,
  text: string,
  route?: ClipRoute,
): { tr: ClipTry; end: (e?: unknown) => void } | undefined {
  if (!tracing()) return;
  const tr: ClipTry = { failed: [] };
  const base = {
    clip: "copy",
    route: route ?? takeRoute(),
    sid,
    remote: isRemote(),
    len: chars(text),
    hash: text ? shortHash(text) : null,
    mouse: term?.modes.mouseTrackingMode ?? null,
    alt: term ? term.buffer.active.type === "alternate" : null,
  };
  return {
    tr,
    end(e?: unknown) {
      traceLine({ ...base, path: tr.path ?? null, failed: tr.failed, ok: e === undefined, error: e === undefined ? null : errText(e) });
      // Read the GTK clipboard back: now, and once more later, to see the write hold.
      if (e === undefined && tr.path === "gtk") {
        readBack(sid, text, 0);
        window.setTimeout(() => readBack(sid, text, 2000), 2000);
      }
    },
  };
}

function readBack(sid: string | null, wrote: string, after: number) {
  invoke<string>("clipboard_read").then(
    (got) => traceLine({ clip: "readback", sid, after, len: chars(got), hash: got ? shortHash(got) : null, same: got === wrote }),
    (e) => traceLine({ clip: "readback", sid, after, error: errText(e) }),
  );
}

/** A copy that did not run because the selection was empty. */
export function traceEmptyCopy(sid: string | null, term: Terminal | undefined) {
  traceCopy(sid, term, "")?.end("empty selection");
}

/** A paste is about to run. Returns the try record and a callback for its end. */
export function tracePaste(sid: string | null, route?: ClipRoute): { tr: ClipTry; end: (text: string, e?: unknown) => void } | undefined {
  if (!tracing()) return;
  const tr: ClipTry = { failed: [] };
  const r = route ?? takeRoute();
  return {
    tr,
    end(text: string, e?: unknown) {
      traceLine({ clip: "paste", route: r, sid, remote: isRemote(), len: chars(text), path: tr.path ?? null, failed: tr.failed, ok: e === undefined, error: e === undefined ? null : errText(e) });
    },
  };
}

/** An OSC 52 write: refused (Host not allowed), or the copy it ran. */
export function traceOsc52Refused(sid: string | null, text: string) {
  if (tracing()) traceLine({ clip: "copy", route: "osc52", sid, remote: isRemote(), len: chars(text), path: null, ok: false, error: "clipboard writes not allowed from this Host" });
}

/** Per terminal: whether it had a selection, and the last event that can clear one. */
const watched = new WeakMap<Terminal, { had: boolean; cause?: { why: string; at: number } }>();

/** An event that can clear the selection, for the `cleared` line's reason. */
export function markClear(term: Terminal, why: string) {
  if (!tracing()) return;
  const w = watched.get(term);
  if (w) w.cause = { why, at: performance.now() };
}

/** Logs a selection that goes empty, with the event just before it. */
export function watchSelection(sid: string, term: Terminal) {
  watched.set(term, { had: false });
  term.onSelectionChange(() => {
    if (!tracing()) return;
    const w = watched.get(term);
    if (!w) return;
    const has = term.hasSelection();
    if (w.had && !has) {
      const c = w.cause;
      const ago = c ? Math.round(performance.now() - c.at) : null;
      // An event more than a second before is not the cause.
      traceLine({ clip: "cleared", sid, reason: c && ago! < 1000 ? c.why : "unknown", ago, mouse: term.modes.mouseTrackingMode });
    }
    w.had = has;
  });
  term.buffer.onBufferChange((b) => markClear(term, b.type === "alternate" ? "alt-screen-on" : "alt-screen-off"));
  term.onResize(() => markClear(term, "resize"));
  term.onData(() => markClear(term, "input"));
  // Call after term.open: the element is there from then on.
  term.element?.addEventListener("mousedown", () => markClear(term, "click"), true);
}

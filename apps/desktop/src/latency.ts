/**
 * Keystroke latency trace. Off unless the app starts with SKIFF_TRACE=<file>;
 * then each keystroke appends one JSON line to that file. All times are ms on
 * the page clock (performance.now), so the stages add up:
 *
 *   queue   keydown created → the page handles it (main thread busy)
 *   ipc     pty_write sent → Rust took it
 *   echo    pty_write sent → first output back for that session
 *   parse   output arrived → xterm parsed it
 *   render  parsed → xterm drew the frame
 *   total   keydown → xterm drew the frame
 *
 * `busy` is the bytes the session printed in the 250 ms before the key: a
 * streaming program makes `echo` include other output. scripts/latency.py
 * summarizes the file.
 */
import { invoke } from "@tauri-apps/api/core";

interface Entry {
  sid: string;
  key: number | null;
  sent: number;
  ipc?: number;
  echo?: number;
  bytes?: number;
  parsed?: number;
  drawn?: number;
  busy: number;
}

let on = false;
const lastKey = new Map<string, number>();
const pending = new Map<string, Entry[]>();
const recent = new Map<string, { t: number; n: number }[]>();
let lines: string[] = [];

export async function initTrace() {
  on = await invoke<boolean>("trace_enabled").catch(() => false);
  if (!on) return;
  console.info("skiff: keystroke latency trace on");
  setInterval(flush, 2000);
  // A key with no echo (a shortcut, a program that does not echo) ends here.
  setInterval(() => {
    const now = performance.now();
    for (const list of pending.values())
      while (list.length && now - list[0].sent > 1000) done(list.shift()!);
  }, 500);
}

/** keydown in a terminal: `timeStamp` is when the event was created. */
export function traceKey(sid: string, e: KeyboardEvent) {
  if (on && e.type === "keydown") lastKey.set(sid, e.timeStamp);
}

/** onData: the terminal is about to send input. Returns a callback for the send's reply. */
export function traceSend(sid: string): (() => void) | undefined {
  if (!on) return;
  const now = performance.now();
  const k = lastKey.get(sid);
  lastKey.delete(sid);
  const busy = (recent.get(sid) ?? []).filter((r) => now - r.t < 250).reduce((a, r) => a + r.n, 0);
  const entry: Entry = { sid, key: k !== undefined && now - k < 100 ? k : null, sent: now, busy };
  (pending.get(sid) ?? pending.set(sid, []).get(sid)!).push(entry);
  return () => (entry.ipc = performance.now());
}

/** Output reached the page. Returns a callback for when xterm has parsed it. */
export function traceOutput(sid: string, bytes: number): (() => void) | undefined {
  if (!on) return;
  const now = performance.now();
  const r = recent.get(sid) ?? recent.set(sid, []).get(sid)!;
  r.push({ t: now, n: bytes });
  while (r.length && now - r[0].t > 250) r.shift();
  const waiting = (pending.get(sid) ?? []).filter((e) => e.echo === undefined);
  if (!waiting.length) return;
  for (const e of waiting) {
    e.echo = now;
    e.bytes = bytes;
  }
  return () => {
    const t = performance.now();
    for (const e of waiting) e.parsed = t;
  };
}

/** xterm drew a frame for this session. */
export function traceRender(sid: string) {
  if (!on) return;
  const list = pending.get(sid);
  if (!list) return;
  const t = performance.now();
  while (list.length && list[0].parsed !== undefined) {
    const e = list.shift()!;
    e.drawn = t;
    done(e);
  }
}

function done(e: Entry) {
  const d = (a?: number | null, b?: number | null) => (a == null || b == null ? null : Math.round((b - a) * 100) / 100);
  lines.push(
    JSON.stringify({
      at: Date.now(),
      sid: e.sid,
      queue: d(e.key, e.sent),
      ipc: d(e.sent, e.ipc),
      echo: d(e.sent, e.echo),
      parse: d(e.echo, e.parsed),
      render: d(e.parsed, e.drawn),
      total: d(e.key ?? e.sent, e.drawn),
      bytes: e.bytes ?? null,
      busy: e.busy,
    }),
  );
}

function flush() {
  if (!lines.length) return;
  const out = lines;
  lines = [];
  invoke("trace_write", { lines: out }).catch(console.error);
}

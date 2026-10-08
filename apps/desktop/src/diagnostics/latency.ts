/**
 * Keystroke latency trace. Help → Start latency trace turns it on; SKIFF_TRACE
 * turns it on at launch. While on, a corner of the focused pane shows the
 * numbers, and each keystroke appends one JSON line to latency.jsonl in the
 * app's log folder (or the SKIFF_TRACE file). All times are ms on the page
 * clock (performance.now), so the stages add up:
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
 * summarizes the file; Help → Copy latency report summarizes this run.
 */
import { invoke } from "@tauri-apps/api/core";
import { copyText } from "../platform/clipboard";
import { button, h } from "../ui/dom";
import { showError } from "../ui/alerts";
import { scheduleRender } from "../app/render";
import { S } from "../app/state";

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

type Stage = "queue" | "ipc" | "echo" | "parse" | "render" | "total";
type Row = Record<Stage, number | null> & { at: number; sid: string; bytes: number | null; busy: number };

const STAGES: Stage[] = ["queue", "ipc", "echo", "parse", "render", "total"];
/** Two frames at 60 Hz. A key slower than this shows red. */
const SLOW_MS = 33;
const MAX_ROWS = 20_000;

let on = false;
let file = "";
let timers: number[] = [];
let startedAt = 0;
let rows: Row[] = [];
let lines: string[] = [];
const lastKey = new Map<string, number>();
const pending = new Map<string, Entry[]>();
const recent = new Map<string, { t: number; n: number }[]>();

export const tracing = () => on;
export const hasReport = () => rows.length > 0;

/** Starts the trace at launch when SKIFF_TRACE is set. */
export async function initTrace() {
  const [path, fromEnv] = await invoke<[string, boolean]>("trace_info").catch(() => ["", false] as [string, boolean]);
  file = path;
  if (fromEnv) startTrace();
}

export function startTrace() {
  if (on) return;
  on = true;
  startedAt = Date.now();
  rows = [];
  timers = [
    window.setInterval(flush, 2000),
    // A key with no echo (a shortcut, a program that does not echo) ends here.
    window.setInterval(() => {
      const now = performance.now();
      for (const list of pending.values())
        while (list.length && now - list[0].sent > 1000) done(list.shift()!);
    }, 500),
  ];
  indicator();
  hud();
  probeGl();
}

/** One bare context, so the `gl` lines can be read against what a context alone costs. */
function probeGl() {
  const t = performance.now();
  const g = document.createElement("canvas").getContext("webgl2", { preserveDrawingBuffer: true });
  lines.push(JSON.stringify({ at: Date.now(), glprobe: performance.now() - t, ok: !!g }));
  g?.getExtension("WEBGL_lose_context")?.loseContext();
}

/** A WebGL renderer attached to a pane: the addon's construction, and `loadAddon` (context, shaders, atlas, first draw), in ms. `warm`: in an idle frame, not for a shown pane. */
export function traceGl(sid: string, ctor: number, load: number, warm: boolean) {
  if (on) lines.push(JSON.stringify({ at: Date.now(), sid, gl: { ctor, load, warm } }));
}

export function stopTrace() {
  if (!on) return;
  on = false;
  timers.forEach(clearInterval);
  timers = [];
  lastKey.clear();
  pending.clear();
  recent.clear();
  flush();
  hudEl?.remove();
  indicatorEl?.remove();
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
  const row: Row = {
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
  };
  rows.push(row);
  if (rows.length > MAX_ROWS) rows.splice(0, rows.length - MAX_ROWS);
  lines.push(JSON.stringify(row));
  hud();
}

function flush() {
  if (!lines.length) return;
  const out = lines;
  lines = [];
  invoke("trace_write", { lines: out }).catch(console.error);
}

function pct(xs: number[], p: number): number {
  return xs[Math.min(xs.length - 1, Math.floor((p / 100) * xs.length))];
}

function values(rs: Row[], s: Stage): number[] {
  return rs.map((r) => r[s]).filter((v): v is number => v !== null).sort((a, b) => a - b);
}

// The overlay redraws at most 4 times a second, so it costs little itself.
let hudEl: HTMLElement | null = null;
let hudTimer: number | undefined;

// While the trace runs the status bar says so, with the key count; a click stops it.
let indicatorEl: HTMLButtonElement | null = null;

function indicator() {
  indicatorEl ??= button("status-item trace-indicator", "", () => {
    stopTrace();
    scheduleRender();
  });
  indicatorEl.title = "Stop latency trace";
  indicatorEl.replaceChildren(h("span", "dot"), h("span", "", `Latency trace · ${rows.length} keys`));
  if (!indicatorEl.isConnected) document.querySelector("#statusbar .spacer")?.before(indicatorEl);
}

function hud() {
  if (hudTimer !== undefined) return;
  hudTimer = window.setTimeout(() => {
    hudTimer = undefined;
    if (!on) return;
    indicator();
    const cell = [...document.querySelectorAll<HTMLElement>(".cell")].find((c) => c.dataset.session === S.focused);
    if (!cell) return hudEl?.remove();
    hudEl ??= h("div", "latency-hud");
    const mine = rows.filter((r) => r.sid === S.focused && r.total !== null);
    const last = mine.at(-1)?.total ?? null;
    const t = values(mine, "total");
    hudEl.classList.toggle("slow", last !== null && last > SLOW_MS);
    hudEl.replaceChildren(
      h("b", "", last === null ? "type to measure" : `${last.toFixed(1)} ms`),
      h("span", "", t.length ? `p50 ${pct(t, 50).toFixed(1)} · p99 ${pct(t, 99).toFixed(1)} · ${t.length} keys` : "latency trace on"),
    );
    if (hudEl.parentElement !== cell) cell.append(hudEl);
  }, 250);
}

/** Per-stage percentiles for this run, split by quiet and printing sessions. */
function report(): string {
  const out = [
    `Skiff keystroke latency, ${new Date(startedAt).toISOString()} to ${new Date().toISOString()}`,
    `${navigator.userAgent}`,
    `Trace file: ${file}`,
  ];
  for (const [title, rs] of [
    ["Quiet session", rows.filter((r) => !r.busy)],
    ["Session printing", rows.filter((r) => r.busy)],
  ] as const) {
    out.push("", `${title}: ${rs.length} keys`, `  ${"stage".padEnd(7)} ${["n", "p50", "p90", "p99", "max"].map((c) => c.padStart(7)).join(" ")}`);
    for (const s of STAGES) {
      const xs = values(rs, s);
      out.push(
        `  ${s.padEnd(7)} ${String(xs.length).padStart(7)} ` +
          (xs.length ? [50, 90, 99].map((p) => pct(xs, p).toFixed(2).padStart(7)).join(" ") + " " + xs[xs.length - 1].toFixed(2).padStart(7) : ""),
      );
    }
    const noEcho = rs.filter((r) => r.echo === null).length;
    if (noEcho) out.push(`  no echo within 1 s: ${noEcho}`);
  }
  return out.join("\n");
}

export async function copyReport() {
  await copyText(report()).catch(showError);
}

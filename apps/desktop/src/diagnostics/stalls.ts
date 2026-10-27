/**
 * Main-thread stalls, for the latency trace. While the trace runs, a frame
 * loop measures the gap between frames; a gap over STALL_MS is a stall. The
 * steps that ran in it (render.sidebar, render.layout, ...) name the work.
 * `step` times a piece of UI work; `work` reports async work, such as a
 * project refresh, that took longer than STALL_MS from start to end.
 */

/** About three frames at 60 Hz. */
export const STALL_MS = 50;
/** Steps older than this cannot belong to a stall that is still open. */
const KEEP_MS = 5000;

export interface StepTime {
  name: string;
  ms: number;
}

interface Hooks {
  stall(ms: number, steps: StepTime[]): void;
  work(name: string, ms: number): void;
}

let hooks: Hooks | null = null;
let last = 0;
let steps: { name: string; start: number; end: number }[] = [];

/** Runs `fn` as a named step. Costs two clock reads while the trace is off. */
export function step<T>(name: string, fn: () => T): T {
  const start = performance.now();
  try {
    return fn();
  } finally {
    if (hooks) {
      const end = performance.now();
      steps.push({ name, start, end });
      const t = stepTimes.get(name);
      if (t) {
        t[0]++;
        t[1] += end - start;
      } else stepTimes.set(name, [1, end - start]);
    }
  }
}

/**
 * Counts an event by name while the trace runs: each render by its cause,
 * and what the sidebar did with it. The trace writes the counts every 2 s.
 */
export function tally(name: string) {
  if (hooks) tallies.set(name, (tallies.get(name) ?? 0) + 1);
}

let tallies = new Map<string, number>();
/** Each step's runs and total ms, stall or not. */
let stepTimes = new Map<string, [number, number]>();

/** The counts and step times since the last call, and a fresh start. */
export function takeTallies(): { tally: Map<string, number>; steps: Map<string, [number, number]> } {
  const t = { tally: tallies, steps: stepTimes };
  tallies = new Map();
  stepTimes = new Map();
  return t;
}

/** Async work that ran `ms` from start to end. */
export function work(name: string, ms: number) {
  if (hooks && ms > STALL_MS) hooks.work(name, ms);
}

export function startStalls(h: Hooks) {
  if (hooks) return;
  hooks = h;
  last = 0;
  steps = [];
  tallies = new Map();
  stepTimes = new Map();
  document.addEventListener("visibilitychange", reset);
  requestAnimationFrame(frame);
}

export function stopStalls() {
  hooks = null;
  steps = [];
  document.removeEventListener("visibilitychange", reset);
}

// A hidden window gets no frames: that gap is no stall.
function reset() {
  last = 0;
}

function frame() {
  if (!hooks) return;
  const now = performance.now();
  if (last && now - last > STALL_MS && document.visibilityState === "visible") {
    const from = last;
    const ran = steps
      .filter((s) => s.end > from && s.start < now)
      .map((s) => ({ name: s.name, ms: Math.round((s.end - s.start) * 10) / 10 }))
      .sort((a, b) => b.ms - a.ms)
      .slice(0, 4);
    hooks.stall(Math.round((now - from) * 10) / 10, ran);
  }
  last = now;
  steps = steps.filter((s) => now - s.end < KEEP_MS);
  requestAnimationFrame(frame);
}

// Dev builds: `skiffStall(200)` in the inspector blocks the page in a step,
// to check that the trace reports it.
if (import.meta.env.DEV)
  (window as unknown as { skiffStall: (ms: number) => void }).skiffStall = (ms) =>
    step("forced", () => {
      const t = performance.now();
      while (performance.now() - t < ms);
    });

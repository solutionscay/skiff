/**
 * The shown Host's agent usage, for Settings › Usage and the top bar cell.
 * The Host reads; the app only asks for the last values. It asks once a
 * minute while the window shows, every second while a read runs, and never
 * while the window is hidden. A Host without `agent_usage@1` gives nothing.
 */
import { hostCan, hostInvoke } from "./hostCaps";
import type { AgentUsage } from "./usageModel";

type Refresh = "none" | "stale" | "all";

/** Each agent with a reader, in Settings › Agents order. Empty until the first answer. */
export let usage: AgentUsage[] = [];
/** The last ask failed: its error. */
export let usageError: string | null = null;

const listeners = new Set<() => void>();
/** `fn` runs after each answer, and each minute for a reset time that passes. */
export function onUsage(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
const tell = () => listeners.forEach((fn) => fn());

let timer: ReturnType<typeof setTimeout> | null = null;
/** Settings › Usage shows: it asks more often. */
let pageOpen = false;

/** One ask. `refresh`: page open ("stale") or Read now ("all") start reads on the Host. */
export async function readUsage(refresh: Refresh = "none"): Promise<void> {
  if (!hostCan("agent_usage")) {
    usage = [];
    usageError = null;
    tell();
    return;
  }
  try {
    usage = await hostInvoke<AgentUsage[]>("agent_usage", "usage_read", { refresh });
    usageError = null;
  } catch (e) {
    usageError = String(e);
  }
  tell();
  schedule();
}

/** The next ask: soon while a read runs, else in a minute (10 s with the page open). */
function schedule() {
  if (timer) clearTimeout(timer);
  timer = null;
  if (document.hidden) return;
  const wait = usage.some((a) => a.reading) ? 1000 : pageOpen ? 10_000 : 60_000;
  timer = setTimeout(() => void readUsage(), wait);
}

/** Settings › Usage opened or closed. Open reads the agents whose last read is older than 60 s. */
export function usagePage(open: boolean) {
  pageOpen = open;
  if (open) void readUsage("stale");
}

/** Starts the asks. The window going hidden stops them; showing again asks at once. */
export function startUsage() {
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      if (timer) clearTimeout(timer);
      timer = null;
    } else void readUsage();
  });
  // A reset time passes between answers: the cell and the rows redraw.
  setInterval(() => {
    if (!document.hidden) tell();
  }, 30_000);
  void readUsage();
}

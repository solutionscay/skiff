/**
 * The selected Host's schedules (#262, `schedules@2`), each with its newest
 * runs. Read with the ledger: a run, an edit and a pause each change the
 * ledger's stamp. Nothing here draws.
 */
import { hostCan, hostInvoke } from "../app/hostCaps";
import { render } from "../app/renderRequest";
import { dataEpoch } from "../app/dataPlane";
import type { Entry } from "./ledgerStore";

/** A run: an ordinary ledger row, its status and the agent step under it. */
export interface Run {
  row: Entry & { ref?: string | null; outcome?: string | null };
  status: "pending" | "running" | "failed" | "succeeded";
  /** The start, in the Host's local time. */
  at: string;
  kids: Entry[];
}

export interface Schedule {
  uid: string;
  name: string;
  cron: string;
  /** The When sentence in words. */
  when: string;
  /** No sentence shows the cron. */
  custom: boolean;
  command: string | null;
  timeout: number | null;
  gate: "always" | "on-fail" | "on-output" | null;
  target: "type" | "pane" | "skipper" | null;
  project: string | null;
  type: string | null;
  pane: string | null;
  owner: string | null;
  /** The prompt file in the Host's Skipper folder: `schedules/<uid>.md`. */
  prompt_file: string;
  /** The prompt's size in bytes. Null: no prompt. */
  prompt: number | null;
  paused_at: string | null;
  /** The next run in the Host's local time, `Nov 11 01:30`. Null while paused. */
  next: string | null;
  last: "succeeded" | "failed" | "running" | "pending" | "skipped" | null;
  /** A run is open: Run now is off. */
  open: boolean;
  runs: Run[];
}

export let schedules: Schedule[] = [];
export let scheduleGeneration = 0;
export let scheduleError: string | null = null;
let data = "";
let asked = 0;

/** Reads the schedules again. A Host without `schedules@2` has none. */
export async function loadSchedules() {
  if (!hostCan("schedules")) {
    if (schedules.length) {
      schedules = [];
      data = "";
      scheduleGeneration++;
      render("ledger");
    }
    return;
  }
  const mine = ++asked;
  const at = dataEpoch();
  let next: Schedule[] = [];
  let err: string | null = null;
  try {
    next = await hostInvoke<Schedule[]>("schedules", "schedule_list");
  } catch (e) {
    err = String(e);
  }
  if (mine !== asked || at !== dataEpoch()) return;
  const now = JSON.stringify([next, err]);
  if (now === data) return;
  data = now;
  schedules = next;
  scheduleError = err;
  scheduleGeneration++;
  render("ledger");
}

/** `skiff schedule ARGS` on the Host, then a new read. Throws the CLI's line. */
export async function scheduleCall(args: string[]): Promise<void> {
  try {
    await hostInvoke("schedules", "schedule_call", { args });
  } finally {
    await loadSchedules();
  }
}

/** The target in words: `type fix`, `pane brief`, `Skipper`, `command`. The project shows apart, as its letters. */
export function targetWords(s: Schedule, words: { skipper: string; command: string; nothing: string }): string {
  if (s.target === "type") return `type ${s.type ?? ""}`;
  if (s.target === "pane") return `pane ${s.pane ?? ""}`;
  if (s.target === "skipper") return words.skipper;
  return s.command ? words.command : words.nothing;
}

/** The project whose letters lead the target: a project Skipper's, else the schedule's. */
export const targetProject = (s: Schedule): string | null => (s.target === "skipper" ? (s.owner && s.owner !== "grand" ? s.owner : null) : s.project);

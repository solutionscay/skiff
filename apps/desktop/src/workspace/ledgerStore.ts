/**
 * The Ledger's rows as the selected Host gives them, and their index. Nothing
 * here draws. `ledger.ts` keeps the public names.
 */
import { render } from "../app/renderRequest";
import { hostInvoke } from "../app/hostCaps";
import { dataEpoch } from "../app/dataPlane";

/** A ledger row as `ledger_rows` returns it. */
export interface Entry {
  id: number;
  uid: string;
  kind: "task" | "plan" | "handoff";
  /** The coordinator whose row it is: "grand", or a project Skipper's project. */
  owner: string;
  parent: string | null;
  pane_id: string | null;
  pane_gen: number | null;
  /** The project the work is for. Empty: no project, shown in every workspace. */
  project: string;
  task: string;
  sent_at: string;
  /** The short title Skipper gave it. */
  title: string | null;
  state: "sent" | "pending" | "blocked" | "done";
  updated_at: string;
  /** When the person, or Skiff once every task was done, and reported or flushed, closed the work. Only a work head has it. */
  closed_at: string | null;
  /** The work's bucket, as `skiff ledger ls` gives it. Every row of one work has the same one.
   *  Missing from a Host that predates it: its rows file as not finished. */
  bucket?: "working" | "open" | "finished";
  /** When skiff set the task done because its pane was gone. */
  gone_at: string | null;
  /** The pane is on another daemon. */
  elsewhere?: boolean;
  /** The Host can show the result. */
  result_exists: boolean;
  /** When Settings › Disk use flushed the row's work folder. */
  flushed_at?: string | null;
  /** The task type. */
  type?: string | null;
  /** Free text; a schedule's run says how its command ended: `exit 0`. */
  ref?: string | null;
  outcome?: string | null;
  /** A schedule's run: the schedule's uid. */
  schedule?: string | null;
}

export let rows: Entry[] = [];
export let generation = 0;
let rowData = "";
let indexedGeneration = -1;
export const byUid = new Map<string, Entry>();
export const children = new Map<string, Entry[]>();
export const paneIds = new Set<string>();
const workTitles = new Map<string, string>();

/** Index the Host's data once per changed reply, including the fallback hand-offs. */
export function indexRows() {
  if (indexedGeneration === generation) return;
  indexedGeneration = generation;
  byUid.clear();
  children.clear();
  paneIds.clear();
  workTitles.clear();
  const fallback = new Map<string, string>();
  for (const r of rows) {
    byUid.set(r.uid, r);
    if (r.parent) {
      const kids = children.get(r.parent) ?? [];
      kids.push(r);
      children.set(r.parent, kids);
    }
    if (r.pane_id) paneIds.add(r.pane_id);
    if (r.kind === "handoff" && r.state !== "done") fallback.set(r.project, titleOf(r));
    if (r.bucket === "finished" || r.state === "done" || r.parent) continue;
    workTitles.set(r.owner, titleOf(r));
    if (r.kind === "handoff") workTitles.set(r.project, titleOf(r));
  }
  for (const [scope, title] of fallback) if (!workTitles.has(scope)) workTitles.set(scope, title);
}

export let error: string | null = null;
export let loaded = false;
/** The last read asked for. Only its reply shows. */
let asked = 0;

/** Reads the rows again. A reply shows only when no read was asked since and
 *  the data connection it came on is still the one on screen. */
export async function loadLedger() {
  const mine = ++asked;
  const at = dataEpoch();
  let next: Entry[] = [];
  let err: string | null = null;
  try {
    next = await hostInvoke<Entry[]>("ledger", "ledger_rows");
  } catch (e) {
    err = String(e);
  }
  if (mine !== asked || at !== dataEpoch()) return;
  const data = JSON.stringify([next, err]);
  // Keep both the generation and row objects for repeated, unchanged replies.
  if (loaded && data === rowData) return;
  rowData = data;
  rows = next;
  error = err;
  loaded = true;
  generation++;
  render("ledger");
}

/**
 * The short title: the first sentence of the task text. It ends at the first
 * ". ", "; " or line break outside parentheses, and is cut at a space before
 * 80 characters. The row's ellipsis cuts the rest.
 */
function shortTitle(text: string): string {
  const t = text.trim();
  let depth = 0;
  let end = t.length;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (c === "(") depth++;
    else if (c === ")") depth = Math.max(0, depth - 1);
    else if (c === "\n" || (depth === 0 && (c === "." || c === ";") && (i + 1 === t.length || t[i + 1] === " "))) {
      end = i;
      break;
    }
  }
  const s = t.slice(0, end).trim();
  if (s.length <= 80) return s;
  const cut = s.lastIndexOf(" ", 80);
  return s.slice(0, cut > 0 ? cut : 80);
}

/** The title Skipper gave the row, else the one the task text gives. */
export const titleOf = (r: Entry) => r.title?.trim() || shortTitle(r.task);

/** The rows as the panel holds them, for a surface that draws them its own way: mission control's strings. */
export const ledgerRows = (): readonly Entry[] => rows;
export const ledgerTitle = titleOf;

/** The title of a project Skipper's newest open work: a hand-off it was given, or its own. */
export function ledgerWorkTitle(project: string): string | null {
  indexRows();
  return workTitles.get(project) ?? null;
}

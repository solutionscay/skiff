/**
 * The right column of a Mission control window: where Admiral or a Skipper
 * stands, in three lines, from what the board already holds (the session and
 * the ledger rows). No read of its own.
 *
 * 1. The state word, in its state color, with a detail: the time in the state
 *    for working ("working · 4m"), why it waits ("waiting · approval"), the
 *    clock time it ended ("done · 10:25 PM"); idle stands alone. A time shows
 *    only when the app saw the change; else the word stands alone.
 * 2. Its current task: the open hand-off it got, else the first open task it
 *    owns, else "no task".
 * 3. Its counts: Admiral's open hand-offs and tasks; a Skipper's open tasks.
 *    Then how many of their panes wait, when any do.
 */
import type { SessionInfo } from "../../platform/types";
import { hostCan } from "../../app/hostCaps";
import { sessions } from "../../app/state";
import { h } from "../../ui/dom";
import { type MessageKey, locale, t, tn } from "../../i18n";
import { type Entry, ledgerTitle } from "../ledger";
import { sinceAt, sinceText } from "./model";

/** waiting_cause (#226) in plain words. A cause this page does not know shows no detail. */
const CAUSE: Record<string, MessageKey> = {
  bell: "mission.win.cause.bell",
  prompt: "mission.win.cause.prompt",
  title: "mission.win.cause.title",
  report: "mission.win.cause.report",
};

const STATE: Record<SessionInfo["state"], MessageKey> = { working: "state.working", waiting: "state.waiting", idle: "state.idle", done: "state.done" };

/** The state word's detail: time in state, the cause of a wait, or when it ended. */
function detail(s: SessionInfo, now: number): string {
  if (s.state === "working") return sinceText(s, now);
  if (s.state === "waiting") return s.waiting_cause && CAUSE[s.waiting_cause] ? t(CAUSE[s.waiting_cause]) : "";
  const at = sinceAt(s);
  if (s.state === "done" && at !== undefined) return new Date(at).toLocaleTimeString(locale(), { hour: "numeric", minute: "2-digit" });
  return "";
}

/** The open ledger rows, indexed once per draw: tasks by owner, hand-offs by pane, and every open hand-off. */
export interface LedgerIndex {
  tasks: Map<string, Entry[]>;
  handoffs: Map<string, Entry[]>;
  open: Entry[];
}

/** Indexes `rows`: what is not finished and not done. */
export function indexLedger(rows: readonly Entry[]): LedgerIndex {
  const idx: LedgerIndex = { tasks: new Map(), handoffs: new Map(), open: [] };
  const put = (map: Map<string, Entry[]>, key: string, r: Entry) => {
    const list = map.get(key);
    if (list) list.push(r);
    else map.set(key, [r]);
  };
  for (const r of rows) {
    if (r.bucket === "finished" || r.state === "done") continue;
    if (r.kind === "task") put(idx.tasks, r.owner, r);
    else if (r.kind === "handoff") {
      idx.open.push(r);
      if (r.pane_id) put(idx.handoffs, r.pane_id, r);
    }
  }
  return idx;
}

/** The three lines, for `refill`. `idx`: the board's ledger index for this draw. */
export function stateParts(s: SessionInfo, now: number, idx: LedgerIndex): HTMLElement[] {
  const line = (cls: string, text: string) => {
    const el = h("span", cls, text);
    el.title = text;
    return el;
  };
  const d = detail(s, now);
  const word = line(`st ${s.state}`, d ? `${t(STATE[s.state])} · ${d}` : t(STATE[s.state]));
  if (!hostCan("ledger")) return [word, line("task none", t("mission.win.noLedger"))];
  const tasks = idx.tasks.get(s.coordinator ? "grand" : (s.skipper ?? "")) ?? [];
  const handoffs = s.coordinator ? idx.open : (idx.handoffs.get(s.id) ?? []);
  const current = (s.coordinator ? null : handoffs[0]) ?? tasks[0];
  const task = line(`task${current ? "" : " none"}`, current ? ledgerTitle(current) : t("mission.win.noTask"));
  const waits = tasks.filter((r) => r.pane_id && sessions.get(r.pane_id)?.state === "waiting").length;
  const counts = [...(s.coordinator ? [tn("mission.win.handoffs", handoffs.length)] : []), tn("mission.win.tasks", tasks.length), ...(waits ? [tn("mission.win.waiting", waits)] : [])];
  return [word, task, line("counts", counts.join(" · "))];
}

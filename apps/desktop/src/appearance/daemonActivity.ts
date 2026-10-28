import { invoke } from "@tauri-apps/api/core";
import { confirmAction } from "../ui/confirm";
import { h } from "../ui/dom";
import { S } from "../app/state";
import { place } from "../app/stateQueries";
import { projectSkipperOf, skiffAgentSession } from "../app/skiffAgent";
import { branchName, locate, scopeFleet, scopeName, taskTitle } from "../workspace/model";
import type { DaemonMetrics, DaemonStatus, SessionInfo } from "../platform/types";

export interface ActivityState { list: SessionInfo[]; metrics: DaemonMetrics | null; version: string | null; error: string | null; timer: number | null; }
export const activityState = (): ActivityState => ({ list: [], metrics: null, version: null, error: null, timer: null });
const bytes = (n: number) => n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
const folder = (path: string) => {
  const parts = path.split("/").filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : path;
};
const age = (at: number) => {
  if (!at) return "—";
  const seconds = Math.max(0, Math.floor((Date.now() - at) / 1000));
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
};

export async function refreshActivity(s: ActivityState, redraw: () => void) {
  try {
    const [list, metrics, status] = await Promise.all([
      invoke<SessionInfo[]>("list_sessions"),
      // An older skiffd has no metrics request: the table still lists its sessions.
      invoke<DaemonMetrics>("daemon_metrics").catch(() => null),
      invoke<DaemonStatus>("daemon_status").catch(() => null),
    ]);
    s.list = list.filter((x) => !x.peek); s.metrics = metrics; s.version = status?.version ?? null; s.error = null;
  } catch (e) { s.error = String(e); }
  redraw();
}
export function stopActivity(s: ActivityState) { if (s.timer !== null) window.clearInterval(s.timer); s.timer = null; }
export function startActivity(s: ActivityState, redraw: () => void) {
  stopActivity(s); void refreshActivity(s, redraw);
  s.timer = window.setInterval(() => void refreshActivity(s, redraw), 5000);
}

function workspace(s: SessionInfo) {
  const open = place(s);
  if (open) return { text: `${open.project.name} / ${branchName(open.worktree)}`, out: false };
  const closed = locate(S.closedProjects, s.cwd);
  // Use the sidebar's ownership lookups. A row shown there is in the workspace.
  if (s.coordinator && skiffAgentSession()?.id === s.id) return { text: "Skiff agent", out: false };
  const fleet = scopeFleet(s.skipper);
  const shown = fleet ? S.projects.some((p) => p.fleet === fleet) : S.projects.some((p) => p.name === s.skipper);
  if (s.skipper && shown && projectSkipperOf(s.skipper)?.id === s.id) return { text: scopeName(s.skipper), out: false };
  if (s.skipper && !s.coordinator) return { text: "owner missing", out: true };
  return closed ? { text: `closed: ${closed.project.name}`, out: true } : { text: "no project", out: true };
}
function stateRank(s: SessionInfo) { return { waiting: 0, working: 1, idle: 2, done: 3 }[s.state]; }
const program = (s: SessionInfo) => s.foreground_program || [s.command, ...s.args].join(" ");

/** The column heads, in table order. `key` is null for a head that does not sort. */
type SortKey = "state" | "session" | "program" | "workspace" | "folder" | "out" | "age";
const COLS: { label: string; key: SortKey | null; cls: string }[] = [
  { label: "STATE", key: "state", cls: "" }, { label: "SESSION", key: "session", cls: "" }, { label: "PROGRAM", key: "program", cls: "" },
  { label: "WORKSPACE", key: "workspace", cls: "" }, { label: "FOLDER", key: "folder", cls: "" },
  { label: "OUT", key: "out", cls: "c-num" }, { label: "AGE", key: "age", cls: "c-num" }, { label: "ACTIONS", key: null, cls: "" },
];
/** The chosen sort. It holds while the app runs and comes back to the default on restart. */
let sort: { key: SortKey; dir: 1 | -1 } | null = null;
/** A session's value under a head. null is a missing value (shown as a dash); it sorts last both ways. */
function sortValue(s: SessionInfo, key: SortKey): string | number | null {
  switch (key) {
    case "state": return stateRank(s);
    case "session": return taskTitle(s);
    case "program": return program(s) || null;
    case "workspace": return workspace(s).text;
    case "folder": return s.cwd || null;
    case "out": return s.output_bytes || null;
    case "age": return s.started_at ? Date.now() - s.started_at : null;
  }
}
/** Default order: out of the workspace first, then state, then start time. */
const byDefault = (a: SessionInfo, b: SessionInfo) => Number(workspace(b).out) - Number(workspace(a).out) || stateRank(a) - stateRank(b) || a.started_at - b.started_at || a.id.localeCompare(b.id);
function compare(a: SessionInfo, b: SessionInfo): number {
  if (!sort) return byDefault(a, b);
  const x = sortValue(a, sort.key), y = sortValue(b, sort.key);
  if (x === null || y === null) return x === y ? byDefault(a, b) : x === null ? 1 : -1;
  const c = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y), undefined, { sensitivity: "base", numeric: true });
  return c * sort.dir || byDefault(a, b);
}

export function daemonActivitySection(s: ActivityState, redraw: () => void, reveal: (id: string, closed: boolean) => void): HTMLElement[] {
  const head = h("div", "set-head"), title = h("div", "set-h", "skiffd activity"); title.id = "set-h-activity";
  head.append(title, h("div", "set-sub mono", "This computer"));
  const band = h("div", "set-band set-daemon-band");
  const m = s.metrics;
  const version = m?.version ?? s.version;
  // A value the daemon did not report shows a dash, never a zero.
  const up = m?.started_at ? age(m.started_at).replace(/^now$/, "0m") : "—";
  band.append(h("span", "", version ? `skiffd ${version}` : "skiffd"), h("span", "", `Up ${up}`), h("span", "", `${s.list.length} sessions`), h("span", "", `Memory ${m?.memory_bytes ? bytes(m.memory_bytes) : "—"}`));
  const table = h("div", "set-daemon");
  table.setAttribute("role", "table");
  table.setAttribute("aria-labelledby", title.id);
  const cols = h("div", "set-row set-cols");
  cols.setAttribute("role", "row");
  for (const c of COLS) {
    // A head that sorts is a button: a click, Enter or Space sorts by it, and again reverses.
    const el = h(c.key ? "button" : "span", c.key ? `set-sort ${c.cls}` : c.cls, c.label);
    el.setAttribute("role", "columnheader");
    if (c.key) {
      const key = c.key; (el as HTMLButtonElement).type = "button"; el.dataset.key = key;
      const on = sort?.key === key;
      el.setAttribute("aria-sort", on ? (sort!.dir > 0 ? "ascending" : "descending") : "none");
      if (on) el.append(h("span", "set-sort-dir", sort!.dir > 0 ? "\u2191" : "\u2193"));
      el.addEventListener("click", () => { sort = { key, dir: on ? (sort!.dir > 0 ? -1 : 1) : 1 }; redraw(); });
    }
    cols.append(el);
  }
  table.append(cols);
  const rows = [...s.list].sort(compare);
  if (!rows.length) { const row = h("div", "set-row set-daemon-row"); row.append(h("span", "c-span", "No sessions")); table.append(row); }
  for (const item of rows) {
    const w = workspace(item), row = h("div", `set-row set-daemon-row${w.out ? " out" : ""}`);
    row.setAttribute("role", "row");
    const cell = (text: string, cls = "", title = text) => { const el = h("span", cls, text); el.title = title; return el; };
    // The sidebar's state mark, in its state color.
    const state = h("span", "c-state"); state.append(h("span", `dot ${item.state}`), h("span", "", item.state));
    // The name alone, as the sidebar shows it. Hover shows the session id.
    row.append(state, cell(taskTitle(item), "c-name", `${taskTitle(item)} · ${item.id}`), cell(program(item)), cell(w.text, w.out ? "bad" : ""), cell(folder(item.cwd), "", item.cwd), cell(item.output_bytes ? bytes(item.output_bytes) : "—", "c-num"), cell(age(item.started_at), "c-num", item.started_at ? new Date(item.started_at).toLocaleString() : ""));
    const acts = h("span", "c-acts");
    const show = h("button", "set-rm", "Reveal"); show.type = "button"; show.disabled = item.skipper != null && !item.coordinator && w.out; show.addEventListener("click", () => reveal(item.id, !!locate(S.closedProjects, item.cwd))); acts.append(show);
    const end = h("button", `set-rm${item.state === "done" ? "" : " c-remove"}`, item.state === "done" ? "Remove" : "End…"); end.type = "button";
    end.addEventListener("click", async () => { if (item.state !== "done" && !(await confirmAction({ title: `End ${taskTitle(item)}?`, body: "Ends this session. Unsaved work in the terminal is lost.", action: "End session" }))) return; await invoke("kill_session", { session: item.id }); void refreshActivity(s, redraw); }); acts.append(end); row.append(acts); table.append(row);
  }
  const out = [head, band, table]; if (s.error) out.push(h("div", "set-error", s.error)); return out;
}

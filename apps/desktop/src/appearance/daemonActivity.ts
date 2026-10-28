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
const short = (id: string) => id.slice(0, 6);
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

export function daemonActivitySection(s: ActivityState, redraw: () => void, reveal: (id: string, closed: boolean) => void): HTMLElement[] {
  const head = h("div", "set-head"); head.append(h("div", "set-h", "skiffd activity"), h("div", "set-sub mono", "This computer"));
  const band = h("div", "set-band set-daemon-band");
  const m = s.metrics;
  const version = m?.version ?? s.version;
  band.append(h("span", "", version ? `skiffd ${version}` : "skiffd"), h("span", "", m ? `Up ${Math.floor((Date.now() - m.started_at) / 60000)}m` : ""), h("span", "", m ? `${m.session_count} sessions` : ""), h("span", "", m?.memory_bytes == null ? "" : `Memory ${bytes(m.memory_bytes)}`));
  const table = h("div", "set-daemon");
  const cols = h("div", "set-row set-cols");
  for (const x of ["STATE", "SESSION", "WORKSPACE", "PROGRAM", "FOLDER", "AGE", "OUT", "ACTIONS"]) cols.append(h("span", "", x));
  table.append(cols);
  const rows = [...s.list].sort((a, b) => Number(workspace(b).out) - Number(workspace(a).out) || stateRank(a) - stateRank(b) || a.started_at - b.started_at || a.id.localeCompare(b.id));
  if (!rows.length) { const row = h("div", "set-row set-daemon-row"); row.append(h("span", "", "No sessions")); table.append(row); }
  for (const item of rows) {
    const w = workspace(item), row = h("div", `set-row set-daemon-row${w.out ? " out" : ""}`);
    const cell = (text: string, cls = "") => { const el = h("span", cls, text); el.title = text; return el; };
    const when = item.started_at ? new Date(item.started_at).toLocaleString() : "";
    row.append(cell(item.state, `c-state ${item.state}`), cell(`${taskTitle(item)} · ${short(item.id)}`), cell(w.text, w.out ? "bad" : ""), cell(item.foreground_program || [item.command, ...item.args].join(" ")), cell(folder(item.cwd)), cell(age(item.started_at)), cell(item.output_bytes == null ? "—" : bytes(item.output_bytes)));
    row.children[4].setAttribute("title", item.cwd);
    row.children[5].setAttribute("title", when);
    const acts = h("span", "c-acts");
    const show = h("button", "set-rm", "Reveal"); show.type = "button"; show.disabled = item.skipper != null && !item.coordinator && !place(item); show.addEventListener("click", () => reveal(item.id, !!locate(S.closedProjects, item.cwd))); acts.append(show);
    const end = h("button", `set-rm${item.state === "done" ? "" : " c-remove"}`, item.state === "done" ? "Remove" : "End…"); end.type = "button";
    end.addEventListener("click", async () => { if (item.state !== "done" && !(await confirmAction({ title: `End ${taskTitle(item)}?`, body: "Ends this session. Unsaved work in the terminal is lost.", action: "End session" }))) return; await invoke("kill_session", { session: item.id }); void refreshActivity(s, redraw); }); acts.append(end); row.append(acts); table.append(row);
  }
  const out = [head, band, table]; if (s.error) out.push(h("div", "set-error", s.error)); return out;
}

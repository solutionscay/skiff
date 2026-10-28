import { hostCan, hostInvoke } from "../app/hostCaps";
import { confirmAction } from "../ui/confirm";
import { h } from "../ui/dom";
import { S, sessions } from "../app/state";
import { groupOf, place } from "../app/stateQueries";
import { branchName, locate, taskTitle } from "../workspace/model";
import type { DaemonMetrics, SessionInfo } from "../platform/types";

export interface ActivityState { list: SessionInfo[]; metrics: DaemonMetrics | null; error: string | null; timer: number | null; }
export const activityState = (): ActivityState => ({ list: [], metrics: null, error: null, timer: null });
const bytes = (n: number) => n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
const short = (id: string) => id.slice(0, 6);

export async function refreshActivity(s: ActivityState, redraw: () => void) {
  try {
    const [list, metrics] = await Promise.all([
      hostInvoke<SessionInfo[]>("session_inventory", "list_sessions"),
      hostCan("daemon_metrics") ? hostInvoke<DaemonMetrics>("daemon_metrics", "daemon_metrics") : Promise.resolve(null),
    ]);
    s.list = list.filter((x) => !x.peek); s.metrics = metrics; s.error = null;
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
  if (s.skipper && !s.coordinator) return { text: "Not in workspace: owner missing", out: true };
  return closed ? { text: `Not in workspace: project closed (${closed.project.name})`, out: true } : { text: "Not in workspace: no open project", out: true };
}
function stateRank(s: SessionInfo) { return { waiting: 0, working: 1, idle: 2, done: 3 }[s.state]; }

export function daemonActivitySection(s: ActivityState, redraw: () => void, reveal: (id: string, closed: boolean) => void): HTMLElement[] {
  const head = h("div", "set-head"); head.append(h("div", "set-h", "skiffd activity"), h("div", "set-sub mono", "This computer"));
  const band = h("div", "set-band set-daemon-band");
  const m = s.metrics;
  band.append(h("span", "", m ? `skiffd ${m.version}` : "skiffd"), h("span", "", m ? `Up ${Math.floor((Date.now() - m.started_at) / 60000)}m` : ""), h("span", "", m ? `${m.session_count} sessions` : ""), h("span", "", m?.memory_bytes ? `Memory ${bytes(m.memory_bytes)}` : ""));
  const table = h("div", "set-daemon");
  const cols = h("div", "set-row set-cols");
  for (const x of ["STATE", "SESSION", "WORKSPACE", "PROGRAM", "FOLDER", "PID", "STARTED", "OUTPUT", "ACTIONS"]) cols.append(h("span", "", x));
  table.append(cols);
  const rows = [...s.list].sort((a, b) => Number(workspace(b).out) - Number(workspace(a).out) || stateRank(a) - stateRank(b) || a.started_at - b.started_at || a.id.localeCompare(b.id));
  if (!rows.length) { const row = h("div", "set-row set-daemon-row"); row.append(h("span", "", "No sessions")); table.append(row); }
  for (const item of rows) {
    const w = workspace(item), group = groupOf(item.id), row = h("div", `set-row set-daemon-row${w.out ? " out" : ""}`);
    const cell = (text: string, cls = "") => { const el = h("span", cls, text); el.title = text; return el; };
    const when = item.started_at ? new Date(item.started_at).toLocaleString() : "";
    row.append(cell(item.state, `c-state ${item.state}`), cell(`${taskTitle(item)} · ${short(item.id)}`), cell(`${w.text}${group ? ` · in group ${group.name}` : ""}`, w.out ? "bad" : ""), cell(item.foreground_program || [item.command, ...item.args].join(" ")), cell(item.cwd), cell(item.pid ? String(item.pid) : "not running"), cell(when), cell(item.output_bytes === undefined ? "" : bytes(item.output_bytes)));
    const acts = h("span", "c-acts");
    const show = h("button", "set-rm", "Reveal"); show.type = "button"; show.disabled = item.skipper != null && !item.coordinator && !place(item); show.addEventListener("click", () => reveal(item.id, !!locate(S.closedProjects, item.cwd))); acts.append(show);
    const end = h("button", `set-rm${item.state === "done" ? "" : " c-remove"}`, item.state === "done" ? "Remove" : "End…"); end.type = "button";
    end.addEventListener("click", async () => { if (item.state !== "done" && !(await confirmAction({ title: `End ${taskTitle(item)}?`, body: "Ends this session. Unsaved work in the terminal is lost.", action: "End session" }))) return; await hostInvoke("session_lifecycle", "kill_session", { session: item.id }); void refreshActivity(s, redraw); }); acts.append(end); row.append(acts); table.append(row);
  }
  const out = [head, band, table]; if (s.error) out.push(h("div", "set-error", s.error)); return out;
}

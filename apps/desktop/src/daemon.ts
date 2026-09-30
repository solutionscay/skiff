/** skiffd: sessions, projects, events. */
import { filledOf, isSlot, placeInSlot, slot } from "./canvas";
import { agentCallsign } from "./agentNames";
import { removePane, replacePane, sessionsOf } from "./layout";
import type { AgentInfo, DaemonEvent, DaemonStatus, Project, SessionInfo, SplitDir } from "./types";
import { invoke } from "@tauri-apps/api/core";
import { $, button, h } from "./dom";
import { reloadChanges } from "./changes";
import { loadFilesSetting } from "./files";
import { deleteGroup, loadGroups, syncTemplateName } from "./groups";
import { render, scheduleRender } from "./render";
import { activeGroupObj, born, gone, OTHER, panes, place, removeErrors, S, sessions, shownIds, upsert } from "./state";
import { focusPane, showSingle, splitWith, unfocus } from "./view";

export function setDaemon(status: DaemonStatus) {
  const dot = $("daemon").querySelector(".dot") as HTMLElement;
  dot.className = "dot " + (status.connected ? "connected" : "error");
  $("daemon-label").textContent = status.connected
    ? `skiffd ${status.version ?? ""}${status.spawned ? " (started)" : ""}`
    : "skiffd unreachable";
  $("socket").textContent = status.socket;
  document.getElementById("daemon-alert")?.remove();
  if (status.warning) {
    const alert = h("div", "");
    alert.id = "daemon-alert";
    alert.setAttribute("role", "alert");
    const restart = button("alert-btn", "Restart skiffd", async () => {
      restart.disabled = true;
      restart.textContent = "Restarting…";
      try {
        await invoke("restart_daemon");
      } finally {
        location.reload();
      }
    });
    alert.append(h("span", "alert-text", status.warning), restart);
    $("body").before(alert);
  }
}

/** A new session is the single view, unless it came from a pane's Split menu. */
export async function newSession(
  cwd: string | null,
  agent: string | null,
  label: string,
  role: "agent" | "shell",
  where?: { target: string; dir: SplitDir } | { slot: string },
) {
  // Projects list agents as command lines; the first word is the program.
  const words = (agent ?? "").trim().split(/\s+/).filter(Boolean);
  const visible = S.focused ? panes.get(S.focused) : undefined;
  // Agent IDs are useful in the launch menu, but call signs make a new crew
  // easier (and more fun) to tell apart once it is running.
  const sessionLabel = role === "agent" ? agentCallsign([...sessions.values()].map((s) => s.label)) : label;
  const info = await invoke<SessionInfo>("create_session", {
    spec: {
      label: sessionLabel,
      name: role === "agent" ? sessionLabel : null,
      role,
      cwd,
      command: words[0] ?? null,
      args: words.slice(1),
      cols: visible?.term.cols ?? 120,
      rows: visible?.term.rows ?? 40,
    },
  });
  for (const b of born) b.add(info.id);
  upsert(info);
  if (where && "slot" in where) placeInSlot(where.slot, info.id);
  else if (where) splitWith(where.target, where.dir, info.id);
  else showSingle(info.id);
}

let projectsSeq = 0;

export async function loadProjects() {
  const seq = ++projectsSeq;
  if (!S.agents.length) S.agents = await invoke<AgentInfo[]>("list_agents").catch(() => []);
  let list: Project[] | null = null;
  let err: string | null = null;
  try {
    [list] = await Promise.all([invoke<Project[]>("list_projects"), loadFilesSetting()]);
  } catch (e) {
    err = String(e);
  }
  // A later call started while this one waited: its answer is newer.
  if (seq !== projectsSeq) return;
  if (list) S.projects = list;
  S.projectsError = err;
  if (S.selectedProject !== OTHER && !S.projects.some((p) => p.name === S.selectedProject)) {
    const f = S.focused ? sessions.get(S.focused) : undefined;
    S.selectedProject = (f && place(f)?.project.name) || S.projects[0]?.name || null;
  }
  for (const [path] of removeErrors) {
    if (!S.projects.some((p) => p.worktrees.some((w) => w.path === path))) removeErrors.delete(path);
  }
  render();
}

let staleTimer: number | undefined;

/**
 * Reads worktrees and changed files again, once a burst of calls settles.
 * Both change outside Skiff: an agent's `git worktree add`, an editor's save.
 */
export function reposStale() {
  window.clearTimeout(staleTimer);
  staleTimer = window.setTimeout(() => {
    void loadProjects();
    reloadChanges();
  }, 400);
}

// Back from a terminal, an editor or a diff tool.
window.addEventListener("focus", reposStale);

/** Pulls `last_output_at` and anything missed, so relative times stay true. Drops what the daemon no longer has. */
export async function refreshSessions() {
  const fresh = new Set<string>();
  born.add(fresh);
  let list: SessionInfo[];
  try {
    list = await invoke<SessionInfo[]>("list_sessions");
  } finally {
    born.delete(fresh);
  }
  const listed = new Set(list.map((s) => s.id));
  for (const s of list) if (!gone.has(s.id)) upsert(s);
  for (const id of [...sessions.keys()]) if (!listed.has(id) && !fresh.has(id)) dropSession(id);
}

export function onEvent(e: DaemonEvent) {
  switch (e.event) {
    case "title": {
      const s = sessions.get(e.session);
      if (s) s.title = e.title;
      break;
    }
    case "state": {
      const s = sessions.get(e.session);
      if (s) {
        s.state = e.state;
        if (e.state === "working") s.last_output_at = Date.now();
        // An agent that stops working may have written files or added a worktree.
        else reposStale();
      }
      break;
    }
    case "exit": {
      const s = sessions.get(e.session);
      if (s) {
        s.state = "done";
        s.exit_code = e.code;
        // A shell that exits (Ctrl+D, `exit`) has nothing left to show: it closes.
        if (s.role === "shell") invoke("kill_session", { session: s.id }).catch(console.error);
      }
      break;
    }
    case "session_created":
      if (gone.has(e.session.id)) return;
      for (const b of born) b.add(e.session.id);
      upsert(e.session);
      break;
    case "session_updated": {
      if (sessions.has(e.session.id)) upsert(e.session);
      break;
    }
    case "session_removed": {
      dropSession(e.session);
      return;
    }
    case "projects_changed":
      void loadProjects();
      return;
    case "groups_changed":
      void loadGroups();
      return;
  }
  scheduleRender();
}

/** The daemon pruned the session from its groups; groups_changed brings them. Here the view drops it at once. */
function dropSession(id: string) {
  gone.add(id);
  if (!sessions.has(id)) return;
  sessions.delete(id);
  panes.get(id)?.term.dispose();
  panes.get(id)?.el.remove();
  panes.delete(id);
  S.selection = S.selection.filter((x) => x !== id);
  if (S.focused === id) S.focused = null;
  for (const g of [...S.groups]) {
    if (!sessionsOf(g.layout).includes(id)) continue;
    let next = removePane(g.layout, id);
    if (filledOf(next).length === 0) {
      // The last session ended. A group on screen stays, with an empty pane in
      // its place. One off screen has nothing left to show: it goes.
      if (g.id !== S.activeGroup) {
        deleteGroup(g);
        continue;
      }
      next = replacePane(g.layout, id, slot);
    }
    if (next) {
      g.layout = next;
      syncTemplateName(g);
    }
    if (g.focus === id) g.focus = filledOf(next)[0] ?? null;
  }
  if (S.single === id) S.single = null;
  const ids = shownIds();
  if (S.focused && !ids.includes(S.focused)) S.focused = null;
  if (!ids.length) return unfocus();
  if (!S.focused) {
    const next = activeGroupObj()?.focus ?? ids.find((x) => !isSlot(x));
    if (next) return focusPane(next);
  }
  render();
}

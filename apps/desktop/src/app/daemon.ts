/** skiffd: sessions, projects, events. */
import { placeInSlot } from "../workspace/canvas";
import { filledOf, isSlot, slot } from "../workspace/layoutSlots";
import { agentCallsign } from "../appearance/agentNames";
import { removePane, replacePane, sessionsOf } from "../workspace/layout";

import type { AgentInfo, DaemonEvent, DaemonStatus, Project, SessionInfo, SplitDir } from "../platform/types";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { $, h } from "../ui/dom";
import { rune } from "../ui/runes";
import { showError } from "../ui/alerts";
import { daemonBadge, restartIfIdle } from "./daemonRestart";
import { loadChangesSetting, reloadChanges } from "../workspace/changes";
import { loadProjectThemes } from "../appearance/themes";
import { loadFilesSetting } from "../workspace/files";
import { deleteGroup, loadGroups, syncTemplateName } from "./groups";
import { render, scheduleRender } from "./render";
import { born, gone, OTHER, removeErrors, S, sessions, upsert } from "./state";
import { hasKeys, markSeen } from "./seen";
import * as waterline from "../terminal/waterline";
import { activeGroupObj, place, shownIds } from "./stateQueries";
import { panes } from "../terminal/terminalState";
import { focusPane, paneNear, revealSession, rowNear, showSingle, splitWith, unfocus } from "../workspace/view";

export function setDaemon(status: DaemonStatus) {
  const dot = $("daemon").querySelector(".dot") as HTMLElement;
  dot.className = "dot " + (status.connected ? "connected" : "error");
  $("daemon-label").textContent = status.connected
    ? `skiffd ${status.version ?? ""}${status.replaced ? ` (updated from ${status.replaced})` : status.spawned ? " (started)" : ""}`
    : status.warning?.kind === "hung"
      ? "skiffd not responding"
      : "skiffd unreachable";
  const socket = $("socket");
  if (status.pid) socket.replaceChildren(`${status.socket} · pid ${status.pid}`, pidCopy(status.pid));
  else socket.textContent = status.socket;
  daemonBadge(status.warning);
}

let copiedTimer: number | undefined;

/** A small button after the pid that copies the number. */
function pidCopy(pid: number): HTMLButtonElement {
  const b = h("button", "status-copy");
  b.type = "button";
  b.title = "Copy pid";
  b.setAttribute("aria-label", `Copy pid ${pid}`);
  b.appendChild(rune("code-copy", 12));
  b.addEventListener("click", () => {
    navigator.clipboard.writeText(String(pid)).then(() => {
      b.classList.add("copied");
      b.title = "Copied";
      clearTimeout(copiedTimer);
      copiedTimer = window.setTimeout(() => {
        b.classList.remove("copied");
        b.title = "Copy pid";
      }, 1200);
    }, showError);
  });
  return b;
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
    [list] = await Promise.all([invoke<Project[]>("list_projects"), loadFilesSetting(), loadChangesSetting(), loadProjectThemes()]);
  } catch (e) {
    err = String(e);
  }
  // A later call started while this one waited: its answer is newer.
  if (seq !== projectsSeq) return;
  if (list) {
    S.projects = list.filter((p) => !p.closed);
    S.closedProjects = list.filter((p) => p.closed);
  }
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

// The app edited projects.toml or a worktree.
void listen("skiff:projects-changed", () => void loadProjects());
// Back from a terminal, an editor or a diff tool.
window.addEventListener("focus", reposStale);
// The focused pane is in front of the user again: its result is seen.
window.addEventListener("focus", () => {
  if (!S.focused) return;
  waterline.arrive(S.focused);
  scheduleRender();
});
// The user went to another app: what the focused pane shows now is what they read.
window.addEventListener("blur", () => {
  if (S.focused) waterline.leave(S.focused);
});

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
    case "away": {
      const s = sessions.get(e.session);
      if (s) s.away = e.away;
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
      restartIfIdle();
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
      if (!sessions.has(e.session.id)) break;
      upsert(e.session);
      // A turn that ends in the pane with the keys leaves nothing to read.
      if (hasKeys(e.session.id)) markSeen(e.session.id);
      break;
    }
    case "session_removed": {
      dropSession(e.session);
      restartIfIdle();
      return;
    }
    case "groups_changed":
      void loadGroups();
      return;
  }
  scheduleRender();
}

/** Groups that must survive their last session ending off screen: "Kill all sessions" keeps the group. */
const kept = new Set<string>();
export const keepGroup = (id: string) => void kept.add(id);
export const releaseGroup = (id: string) => void kept.delete(id);

/** The daemon pruned the session from its groups; groups_changed brings them. Here the view drops it at once. */
function dropSession(id: string) {
  gone.add(id);
  if (!sessions.has(id)) return;
  // The keys go to a neighbor, picked while the pane and its row are still there:
  // the pane beside it in a group, else the next session row in the list.
  const near = S.focused !== id ? null : shownIds().length > 1 ? paneNear(id) : S.single === id ? rowNear(id) : null;
  sessions.delete(id);
  waterline.forget(id);
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
      if (g.id !== S.activeGroup && !kept.has(g.id)) {
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
  if (near && ids.includes(near)) return focusPane(near);
  if (!ids.length) return near && sessions.has(near) ? revealSession(near) : unfocus();
  if (!S.focused) {
    const next = activeGroupObj()?.focus ?? ids.find((x) => !isSlot(x));
    if (next) return focusPane(next);
  }
  render();
}

import { DEFAULT_ACCENT, MAX_PANES, S, sessions, selectedWorktree } from "./state";

import { ink } from "./appTheme";
import { leaf, sessionsOf } from "./layout";
import { byStart, locate, type Place } from "./model";
import type { AgentInfo, Group, Layout, Project, SessionInfo, Worktree } from "./types";

/** What the + menu offers, in order. */
export function enabledAgents(): AgentInfo[] {
  return S.agents.filter((a) => a.enabled);
}

export const accent = (p: Project | null | undefined) => (p?.color ? ink(p.color) : DEFAULT_ACCENT);

/** locate() per cwd, cached until the project list changes. Renders call it for every row. */
let placeFor: Project[] | null = null;

const placeMemo = new Map<string, Place | null>();

export function place(s: SessionInfo): Place | null {
  if (placeFor !== S.projects) {
    placeMemo.clear();
    placeFor = S.projects;
  }
  let at = placeMemo.get(s.cwd);
  if (at === undefined) {
    at = locate(S.projects, s.cwd);
    placeMemo.set(s.cwd, at);
  }
  return at;
}

export const currentProject = () => S.projects.find((p) => p.name === S.selectedProject) ?? null;

/** Sessions that sit under a group in the sidebar, not under their worktree. */
export function groupedIds(): Set<string> {
  return new Set(S.groups.flatMap((g) => sessionsOf(g.layout)));
}

export function worktreeSessions(w: Worktree): SessionInfo[] {
  return [...sessions.values()]
    .filter((s) => place(s)?.worktree === w)
    .sort(byStart);
}

export const activeGroupObj = () => S.groups.find((g) => g.id === S.activeGroup) ?? null;

/** The saved view: the active group's tree, else the single session. */
export function currentLayout(): Layout | null {
  const g = activeGroupObj();
  if (g) return g.layout;
  return S.single ? leaf(S.single) : null;
}

export const shownIds = () => sessionsOf(currentLayout());

/** What the terminal area draws: the saved view, or its one maximized pane. */
export function viewLayout(): Layout | null {
  const l = currentLayout();
  return S.maximized && sessionsOf(l).includes(S.maximized) ? leaf(S.maximized) : l;
}

export const splitFull = () => shownIds().length >= MAX_PANES;

export function groupOf(id: string): Group | null {
  return S.groups.find((g) => sessionsOf(g.layout).includes(id)) ?? null;
}

/** The worktree new sessions go to: the focused session's, else the selected one. */
export function currentWorktree(): { p: Project; w: Worktree } | null {
  const s = S.focused ? sessions.get(S.focused) : undefined;
  const at = s ? place(s) : null;
  if (at) return { p: at.project, w: at.worktree };
  const p = currentProject();
  if (!p) return null;
  const path = selectedWorktree.get(p.name);
  const w = p.worktrees.find((x) => x.path === path) ?? p.worktrees.find((x) => x.is_main) ?? p.worktrees[0];
  return w ? { p, w } : null;
}

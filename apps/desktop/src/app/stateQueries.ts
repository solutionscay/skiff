import { DEFAULT_ACCENT, MAX_PANES, S, sessions, selectedWorktree } from "./state";

import { ink } from "../appearance/appTheme";
import { leaf, sessionsOf } from "../workspace/layout";
import { byStart, locate, samePath, type Place } from "../workspace/model";
import type { AgentInfo, Group, Layout, Project, SessionInfo, Worktree } from "../platform/types";

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

/** The worktrees the session works in other than its own, as the project list knows them. */
export function awayPlaces(s: SessionInfo): Place[] {
  const home = place(s)?.worktree;
  const out: Place[] = [];
  for (const a of s.away ?? []) {
    const at = locate(S.projects, a.path);
    if (at && at.worktree !== home && samePath(at.worktree.path, a.path) && !out.some((o) => o.worktree === at.worktree)) out.push(at);
  }
  return out;
}

/** Sessions from other worktrees that work in `w`. */
export function visitors(w: Worktree): SessionInfo[] {
  return [...sessions.values()].filter((s) => awayPlaces(s).some((a) => a.worktree === w)).sort(byStart);
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

/** True when `id` is a pane of the group on screen. Only such a pane has a group to leave. */
export const inShownGroup = (id: string) => !!activeGroupObj() && shownIds().includes(id);

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

import { sessionsOf } from "./layout";

import { byStart } from "./model";
import type { Group, SessionInfo, Worktree } from "../platform/types";

import { S, sessions } from "../app/state";
import { awayPlaces, place } from "../app/stateQueries";

import { groupHome } from "./view";
import { fleetSkipperOf, projectSkipperOf, projectSkippers, skiffAgentSession } from "../app/skiffAgent";
import { treeSections } from "./tree";
/** One line in a session list: a session, a group's header, or a pointer to a session from another worktree that works here. */
export type Line =
  | { session: SessionInfo; branch?: boolean; group?: undefined; pointer?: undefined; in?: Group; groupRail?: boolean }
  | { group: Group; hasPrevious: boolean; hasNext: boolean; pointer?: undefined }
  | { pointer: SessionInfo; group?: undefined };

export interface LineIndex {
  groups: Map<Worktree | null, Group[]>;
  sessions: Map<Worktree | null, SessionInfo[]>;
  visitors: Map<Worktree, SessionInfo[]>;
  grouped: Set<string>;
  skippers: Set<string>;
  agent: string | undefined;
}

/** Gather the rows once for a sidebar render. */
export function lineIndex(): LineIndex {
  const index: LineIndex = { groups: new Map(), sessions: new Map(), visitors: new Map(), grouped: new Set(), skippers: new Set(projectSkippers().map((s) => s.id)), agent: skiffAgentSession()?.id };
  for (const g of S.groups) {
    const home = groupHome(g);
    const groups = index.groups.get(home) ?? [];
    groups.push(g);
    index.groups.set(home, groups);
    for (const id of sessionsOf(g.layout)) index.grouped.add(id);
  }
  for (const s of sessions.values()) {
    const home = place(s)?.worktree ?? null;
    const own = index.sessions.get(home) ?? [];
    own.push(s);
    index.sessions.set(home, own);
    for (const at of awayPlaces(s)) {
      const visitors = index.visitors.get(at.worktree) ?? [];
      visitors.push(s);
      index.visitors.set(at.worktree, visitors);
    }
  }
  for (const rows of index.sessions.values()) rows.sort(byStart);
  for (const rows of index.visitors.values()) rows.sort(byStart);
  return index;
}

/**
 * A worktree's lines: its groups (a group lives in the worktree of its first
 * session) with their members, then its sessions in no group.
 */
export function worktreeLines(w: Worktree | null, index: LineIndex = lineIndex()): Line[] {
  const lines: Line[] = [];
  const groups = index.groups.get(w) ?? [];
  for (const [groupIndex, g] of groups.entries()) {
    const members: Array<{ session: SessionInfo; branch: boolean }> = [];
    for (const id of sessionsOf(g.layout)) {
      const m = sessions.get(id);
      if (m) members.push({ session: m, branch: (place(m)?.worktree ?? null) !== w });
    }
    lines.push({ group: g, hasPrevious: groupIndex > 0, hasNext: groupIndex < groups.length - 1 });
    members.forEach(({ session, branch }) => {
      lines.push({ session, in: g, branch, groupRail: groupIndex < groups.length - 1 });
    });
  }
  // The Skiff agent has its own row above the projects. In a group it stays
  // there too. A live project Skipper has the first row of its project.
  const loose = index.sessions.get(w) ?? [];
  for (const m of loose) if (!index.grouped.has(m.id) && m.id !== index.agent && !index.skippers.has(m.id)) lines.push({ session: m });
  if (w) for (const m of index.visitors.get(w) ?? []) lines.push({ pointer: m });
  return lines;
}

/**
 * Every live session in tree order, folds aside: the Skiff agent, then each
 * section's fleet Skipper and its projects (the project Skipper, then each
 * worktree's lines), then Other. An open workspace leaves out the projects
 * it does not show. Each session once.
 */
export function treeOrder(index: LineIndex = lineIndex()): string[] {
  const out: string[] = [];
  const add = (id: string | undefined) => {
    if (id && sessions.has(id) && !out.includes(id)) out.push(id);
  };
  const lines = (w: Worktree | null) => {
    for (const l of worktreeLines(w, index)) if ("session" in l) add(l.session.id);
  };
  add(index.agent);
  for (const sec of treeSections()) {
    if (sec.fleet) add(fleetSkipperOf(sec.fleet)?.id);
    for (const p of sec.projects) {
      add(projectSkipperOf(p.name)?.id);
      for (const w of p.worktrees ?? []) lines(w);
    }
  }
  lines(null);
  return out;
}

import type { Project, SessionInfo, Worktree } from "./types";

export interface Place {
  project: Project;
  worktree: Worktree;
}

function trimSlash(p: string): string {
  return p.length > 1 ? p.replace(/\/+$/, "") : p;
}

function within(cwd: string, root: string): boolean {
  const c = trimSlash(cwd);
  const r = trimSlash(root);
  return c === r || c.startsWith(r === "/" ? r : r + "/");
}

/** The worktree whose path is the longest prefix of `cwd`, across all projects. */
export function locate(projects: Project[], cwd: string): Place | null {
  let best: Place | null = null;
  let len = -1;
  for (const project of projects) {
    for (const worktree of project.worktrees) {
      const n = trimSlash(worktree.path).length;
      if (n > len && within(cwd, worktree.path)) {
        best = { project, worktree };
        len = n;
      }
    }
  }
  return best;
}

export function samePath(a: string, b: string): boolean {
  return trimSlash(a) === trimSlash(b);
}

export function basename(p: string): string {
  const t = trimSlash(p);
  return t.slice(t.lastIndexOf("/") + 1) || t;
}

export function agentName(s: SessionInfo): string {
  return s.command ? basename(s.command) : s.role;
}

/** A user or Skiff name wins. An agent label wins over its terminal title. */
export function taskTitle(s: SessionInfo): string {
  return s.name || (s.role === "agent" ? s.label || s.title : s.title || s.label) || agentName(s);
}

export function branchName(w: Worktree): string {
  return w.branch ?? "(detached)";
}

/** Finished out of sight: a result waits to be read. */
export function isUnread(s: SessionInfo): boolean {
  return s.state === "idle" && !!s.unread;
}

export function rank(s: SessionInfo): number {
  if (isUnread(s)) return 1;
  return { waiting: 0, working: 2, idle: 3, done: 4 }[s.state];
}

/** Display order: oldest first. It never changes with state. */
export function byStart(a: SessionInfo, b: SessionInfo): number {
  return a.started_at - b.started_at || a.id.localeCompare(b.id);
}

/** Which session to focus: waiting first, then most recent output. Not for display. */
export function bySessionPriority(a: SessionInfo, b: SessionInfo): number {
  return rank(a) - rank(b) || b.last_output_at - a.last_output_at || a.id.localeCompare(b.id);
}

/**
 * Subsequence match per space-separated token. Returns -1 when a token does not
 * match. Higher is better: consecutive characters and word starts score more.
 */
export function fuzzy(query: string, text: string): number {
  const hay = text.toLowerCase();
  let total = 0;
  for (const token of query.toLowerCase().split(/\s+/).filter(Boolean)) {
    let score = 0;
    let at = -1;
    let prev = -2;
    for (const ch of token) {
      at = hay.indexOf(ch, at + 1);
      if (at < 0) return -1;
      score += 1;
      if (at === prev + 1) score += 2;
      if (at === 0 || /[\s/·._-]/.test(hay[at - 1])) score += 3;
      prev = at;
    }
    total += score;
  }
  return total;
}

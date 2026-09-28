import type { Project, SessionInfo, SessionState, Worktree } from "./types";

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

/** The user's name wins; then the program's terminal title; then a label; then the agent. */
export function taskTitle(s: SessionInfo): string {
  return s.name || s.title || s.label || agentName(s);
}

export function branchName(w: Worktree): string {
  return w.branch ?? "(detached)";
}

export function rank(state: SessionState): number {
  return { waiting: 0, working: 1, idle: 2, done: 3 }[state];
}

/** Display order: oldest first. It never changes with state. */
export function byStart(a: SessionInfo, b: SessionInfo): number {
  return a.started_at - b.started_at || a.id.localeCompare(b.id);
}

/** Which session to focus: waiting first, then most recent output. Not for display. */
export function bySessionPriority(a: SessionInfo, b: SessionInfo): number {
  return rank(a.state) - rank(b.state) || b.last_output_at - a.last_output_at || a.id.localeCompare(b.id);
}

export function relTime(ms: number, now = Date.now()): string {
  if (!ms) return "";
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
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

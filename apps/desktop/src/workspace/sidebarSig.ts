/** What the tree draws, as signatures: when a block rebuilds. */
import { worktreeLines, type LineIndex } from "./sidebarLines";
import { filledOf } from "./layoutSlots";
import { agentKind } from "../appearance/agentIcon";
import { ink } from "../appearance/appTheme";
import { appliedTheme, ownTheme, signatureColor } from "../appearance/themes";
import { agentName, taskTitle } from "./model";
import { faceSig } from "./skipperFace";
import type { Group, Project } from "../platform/types";
import { changesSig, hasChanges, showsChanges } from "./changes";
import { filesSig, showsFiles } from "./files";
import { collapsed, removeErrors, removing, S, selectedWorktree, sessions } from "../app/state";
import { accent, awayPlaces, place } from "../app/stateQueries";
import { groupHome } from "./view";
import { bareWorktree, folded, hiddenSplit, isOpen, outsideSessions, projectKey, projectsHidden, treeSections } from "./tree";
import { shows } from "./profile";
import { ledgerNeeds } from "./ledger";
import { fleetScope, projectSkipperOf, skiffAgentChoice, skiffAgentFolder, skiffAgentName, skiffAgentSession, skiffWatch } from "../app/skiffAgent";
import { groupColor, look } from "./sessionRows";
import { skipperSig } from "./skipperRows";

let lastProjects: Project[] | null = null;
let lastGroups: Group[] | null = null;
let listGen = 0;

export const AGENT_BLOCK = "agent";
export const OTHER_BLOCK = "other";
export const OUTSIDE_BLOCK = "outside";
export const projectBlockKey = (name: string) => `project:${name}`;

/**
 * Everything the tree draws, as one string, and the part of it each block
 * draws. Renders run while agents work. A rebuilt row loses its hover,
 * restarts its icons and costs a style pass, so a block is rebuilt only when
 * what it shows changes. Expanding one project rebuilds that project.
 */
export function sidebarSig(index: LineIndex): { all: string; parts: Map<string, string>; looks: Map<string, string[]> } {
  // Rows close over project, worktree and group objects. A new list brings new
  // objects, and some callers compare them by identity: the rows are rebuilt.
  if (S.projects !== lastProjects || S.groups !== lastGroups) {
    lastProjects = S.projects;
    lastGroups = S.groups;
    listGen++;
  }
  // The session rows each project draws: its own, its visitors' and its groups' members.
  const under = new Map<string, unknown[]>();
  const add = (name: string | null, row: unknown) => {
    const k = name === null ? OTHER_BLOCK : projectBlockKey(name);
    (under.get(k) ?? under.set(k, []).get(k)!).push(row);
  };
  // State and title changes rebuild no block: applyLook writes them into kept rows.
  // A folded row counts the states under it, so its block keeps them.
  const looksUnder = new Map<string, string[]>();
  const addLook = (name: string | null, l: string) => {
    const k = name === null ? OTHER_BLOCK : projectBlockKey(name);
    (looksUnder.get(k) ?? looksUnder.set(k, []).get(k)!).push(l);
  };
  const rowOf = new Map<string, unknown>();
  const rows = [...sessions.values()].map((s) => {
    const t = ownTheme(s);
    const at = place(s);
    const away = awayPlaces(s);
    const row = [
      s.id, s.started_at,
      agentName(s), agentKind(s), t ? ink(signatureColor(t)) : null,
      at?.worktree.path ?? null, away.map((a) => a.worktree.path),
    ];
    rowOf.set(s.id, row);
    add(at?.project.name ?? null, row);
    addLook(at?.project.name ?? null, look(s));
    for (const a of away) add(a.project.name, row);
    return [row, look(s)];
  });
  const groups = S.groups.map((g) => {
    const lead = sessions.get(g.focus ?? filledOf(g.layout)[0]);
    const row = [g.id, g.name, g.layout, g.cwd, groupColor(g), accent(lead ? place(lead)?.project : null)];
    const home = groupHome(g);
    const owner = home ? S.projects.find((p) => p.worktrees.includes(home))?.name ?? null : null;
    add(owner, [row, filledOf(g.layout).map((id) => rowOf.get(id) ?? null)]);
    return row;
  });
  const projects = S.projects.map((p) => [
    p.name, p.short, p.icon, p.fleet ?? null, p.error, p.default_branch, accent(p), selectedWorktree.get(p.name),
    p.worktrees.map((w) => [
      w.path, w.branch, w.is_main, w.locked, w.prunable, collapsed.has(w.path), removing.has(w.path), removeErrors.get(w.path),
      showsChanges(p) ? changesSig(w) : null, showsFiles(p) ? filesSig(w) : null,
    ]),
  ]);
  // The agent icons are inked against the applied app theme: a new theme re-inks them.
  // The watcher state changes with no session event when the grace ends.
  const sa = skiffAgentSession();
  const agent = skiffAgentFolder() ? [skiffAgentName(), sa?.id ?? null, skiffAgentChoice()?.id ?? null, sa ? skiffWatch(sa) : null, sa?.watch?.follows ?? null, ledgerNeeds(), sa ? [rowOf.get(sa.id), look(sa)] : null, faceSig(null)] : null;
  const base = [listGen, appliedTheme(), sa?.id ?? null];
  const parts = new Map<string, string>();
  parts.set(AGENT_BLOCK, JSON.stringify([base, agent, sa ? S.selection.includes(sa.id) : false]));
  parts.set(OTHER_BLOCK, JSON.stringify([base, under.get(OTHER_BLOCK) ?? null, S.selection, S.renamingSession, S.renaming?.id]));
  S.projects.forEach((p, i) => {
    const k = projectBlockKey(p.name);
    const open = isOpen(projectKey(p.name));
    // An open project's Skipper row is rebuilt alone (syncSkipperRow).
    const skipper = open ? !!projectSkipperOf(p.name) : skipperSig(p.name);
    const bare = bareWorktree(p);
    const lines = bare ? worktreeLines(bare, index) : [];
    const bareEmpty = bare ? !lines.length && !skipper && !showsFiles(p) && !(showsChanges(p) && hasChanges(bare)) : false;
    parts.set(k, JSON.stringify([base, projects[i].slice(0, -1), p.worktrees.map((w) => w.path), bare?.path, bareEmpty, open, p.name === S.selectedProject, skipper, open ? null : looksUnder.get(k) ?? null]));
    for (const w of p.worktrees) {
      const wtLines = worktreeLines(w, index);
      const ids = wtLines.flatMap((l) => l.group ? [l.group.id] : "session" in l ? [l.session.id] : [l.pointer.id]);
      parts.set(`worktree:${p.name}:${w.path}`, JSON.stringify([base, (projects[i][projects[i].length - 1] as unknown[][])[p.worktrees.indexOf(w)], bare === w, w.path, w.path === selectedWorktree.get(p.name), wtLines.map((l) => l.group ? [groups[S.groups.indexOf(l.group)], filledOf(l.group.layout).map((id) => rowOf.get(id))] : l.pointer ? [rowOf.get(l.pointer.id), taskTitle(l.pointer)] : rowOf.get(l.session.id)), S.selection.filter((id) => ids.includes(id)), ids.includes(S.renamingSession ?? "") ? S.renamingSession : null, ids.includes(S.renaming?.id ?? "") ? S.renaming?.id : null]));
    }
  });
  const all = JSON.stringify([
    listGen, appliedTheme(), S.selectedProject, folded.keys(), S.projects.map(shows), hiddenSig(), outsideSig(),
    S.selection, S.renamingSession, S.renaming?.id ?? null, rows, groups, projects, agent,
    S.projects.map((p) => faceSig(p.name)), treeSections().map((sec) => sec.fleet ? faceSig(fleetScope(sec.fleet)) : null),
    S.dataState, S.dataNote,
  ]);
  return { all, parts, looks: looksUnder };
}

/** The sessions under Not in this workspace, and what their rows draw. */
export function outsideSig() {
  return outsideSessions().map(({ session: x, where }) => [x.id, where, taskTitle(x), agentName(x), agentKind(x)]);
}

/** What each fleet that hides its projects shows: its head's count and the rows still under it. */
function hiddenSig() {
  return treeSections().flatMap((sec) => {
    if (!sec.fleet || !projectsHidden(sec.fleet)) return [];
    const { shown, hidden } = hiddenSplit(sec.projects);
    return [[sec.fleet, hidden, shown.map((x) => x.id)]];
  });
}

/** The list generation: it grows when a new project or group list arrives. */
export const listGeneration = () => listGen;

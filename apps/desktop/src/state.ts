/** Shared app state and the queries over it. `S` holds what several modules reassign. */
import { ink } from "./appTheme";
import { leaf, sessionsOf } from "./layout";
import { byStart, locate, type Place } from "./model";
import type { AgentInfo, Group, Layout, Project, SessionInfo, TerminalTheme, Worktree } from "./types";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import type { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";

export interface Pane {
  el: HTMLDivElement;
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  /** The WebGL renderer, while this pane keeps one. */
  webgl?: WebglAddon;
  /** The theme id this pane last drew with. */
  theme?: string;
  /** Out of the layout: no output stream, so it costs nothing while hidden. */
  parked: boolean;
  /** Counts subscriptions. A chunk from an older one is dropped. */
  stream: number;
  /** Bytes xterm parsed that Rust has not heard about yet. */
  unacked: number;
  /** An ack_output call is in flight. */
  acking: boolean;
  /** Subscribe and unsubscribe calls, one after another, so they land in order. */
  sub: Promise<void>;
}

/** Accent for sessions outside every project, and projects without `color`. */
export const DEFAULT_ACCENT = "var(--project-default)";

export const sessions = new Map<string, SessionInfo>();
export const panes = new Map<string, Pane>();

/** Opens in flight, so concurrent focus calls share one terminal and one subscription. */
export const opening = new Map<string, Promise<Pane>>();

/** Selected worktree path per project name. */
export const selectedWorktree = new Map<string, string>();
export const collapsed = new Set<string>();

/** What the + menu offers, in order. */
export function enabledAgents(): AgentInfo[] {
  return S.agents.filter((a) => a.enabled);
}

export const removeErrors = new Map<string, string>();

/** Worktrees with a remove in flight. */
export const removing = new Set<string>();

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

/** Merge a daemon copy into the known session, so menus and closures holding it see the change. */
export function upsert(info: SessionInfo) {
  const known = sessions.get(info.id);
  if (known) Object.assign(known, info);
  else sessions.set(info.id, info);
}

/** The rail's "Other" entry: sessions outside every project (a removed worktree, a folder not added). */
export const OTHER = "\u0000other";

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

/** A group holds at most this many panes. */
export const MAX_PANES = 4;

export const splitFull = () => shownIds().length >= MAX_PANES;

export const FULL_HINT = `group is full (${MAX_PANES})`;

export function groupOf(id: string): Group | null {
  return S.groups.find((g) => sessionsOf(g.layout).includes(id)) ?? null;
}

/** Sessions the daemon removed. A list taken before the removal must not bring them back. */
export const gone = new Set<string>();

/** One set per list_sessions in flight: sessions created meanwhile, which that list cannot hold. */
export const born = new Set<Set<string>>();

export const FONT_DEFAULT = 13;

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

function savedFontSize(): number {
  try {
    return Number(localStorage.getItem("skiff.fontSize")) || FONT_DEFAULT;
  } catch {
    return FONT_DEFAULT;
  }
}

/** State that several modules reassign. */
export const S = {
  /** The pane that gets the keys. Always one of the shown sessions, or null. */
  focused: null as string | null,
  /** The group whose row is the sidebar selection. Its terminals are shown but none is picked. */
  groupPicked: null as string | null,
  // What the terminal area shows: the active group's layout, else one session.
  groups: [] as Group[],
  activeGroup: null as string | null,
  single: null as string | null,
  /** The group whose name is an input, and what is typed in it. */
  renaming: null as { id: string; name: string } | null,
  /** The session whose title is an input for renaming. */
  renamingSession: null as string | null,

  /** Open projects, in rail order. */
  projects: [] as Project[],
  /** Closed projects: kept with their settings, out of the rail. */
  closedProjects: [] as Project[],
  projectsError: null as string | null,
  selectedProject: null as string | null,
  /** Known agents with installed and enabled state, from list_agents. */
  agents: [] as AgentInfo[],
  /** The project just added: the terminal area offers to start its first agent. */
  justAdded: null as string | null,

  /** Sessions picked with Ctrl or Shift+click, in click order. */
  selection: [] as string[],
  /** The list item the session list's single Tab stop lands on. */
  roveKey: null as string | null,
  /** The keys are on the rail (Ctrl+Shift+Left): Ctrl+Shift+Up/Down step through
   *  projects, and no sidebar row is highlighted. Ctrl+Shift+Right goes back. */
  atRail: false,
  /** A pane asked for the keys before its terminal opened. It takes them when it opens. */
  grab: null as string | null,
  /** The session focused before the current one, for Back. */
  previous: null as string | null,

  /** Numbers the groups not saved yet. */
  tmpSeq: 0,
  themes: [] as TerminalTheme[],
  /** `[appearance] theme`: a theme id, or null for Harbor. It colors the app and every terminal without its own. */
  appTheme: null as string | null,
  fontSize: savedFontSize(),
};

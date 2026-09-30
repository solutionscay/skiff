// Wire types from skiff-core, as the Tauri commands return them.

export type SessionState = "working" | "waiting" | "idle" | "done";
export type Role = "agent" | "task" | "server" | "shell";

export interface SessionInfo {
  id: string;
  label: string;
  role: Role;
  cwd: string;
  command: string;
  args: string[];
  state: SessionState;
  cols: number;
  rows: number;
  pid: number | null;
  exit_code: number | null;
  /** This terminal's own theme id; wins over the project's. */
  theme: string | null;
  /** A name Skiff or the user gave; wins over `title`. */
  name: string | null;
  /** Terminal title the program set, if any. */
  title: string | null;
  /** Unix ms. */
  started_at: number;
  /** Unix ms. */
  last_output_at: number;
  /** Client only. It finished working while the user looked elsewhere. */
  unread?: boolean;
}

export interface Worktree {
  path: string;
  /** Short branch name. `null` when detached. */
  branch: string | null;
  head: string;
  is_main: boolean;
  locked: boolean;
  prunable: boolean;
}

export interface Project {
  name: string;
  short: string;
  path: string;
  color: string | null;
  agents: string[];
  /** Rail image as a data URL, configured or found in the repo. */
  icon: string | null;
  /** Absolute path of the background image behind the terminals, when set. */
  background: string | null;
  worktrees: Worktree[];
  /** Path missing, not a git repo, and so on. `worktrees` is empty then. */
  error: string | null;
  /** Out of the rail until it opens again. Its worktrees are not read. Absent from an older daemon. */
  closed?: boolean;
}

export type DaemonEvent =
  | { event: "state"; session: string; state: SessionState }
  | { event: "exit"; session: string; code: number | null }
  | { event: "session_created"; session: SessionInfo }
  | { event: "session_removed"; session: string }
  | { event: "session_updated"; session: SessionInfo }
  | { event: "projects_changed" }
  | { event: "groups_changed" }
  | { event: "title"; session: string; title: string | null };

export interface AgentInfo {
  id: string;
  /** What the + menu runs: the user's command, else the default. */
  command: string;
  default_command: string;
  installed: boolean;
  enabled: boolean;
}

/** `[appearance]` in the daemon config. `font_size` is absent from an older daemon. */
export interface Appearance {
  theme: string | null;
  font_size?: number | null;
}

export interface DaemonStatus {
  connected: boolean;
  version: string | null;
  socket: string;
  spawned: boolean;
  /** The daemon differs from this app and was not replaced. */
  warning: DaemonWarning | null;
  /** The version of an older daemon this app replaced at launch. */
  replaced: string | null;
}

export interface DaemonWarning {
  kind: "outdated" | "protocol" | "newer" | "hung";
  message: string;
  /** Live sessions a restart stops. `null` when the daemon did not say. */
  sessions: number | null;
}

export interface FolderInfo {
  /** Git top level. `null` when the folder is not in a repository. */
  root: string | null;
  name: string;
  short: string;
  color: string;
  branch: string | null;
  worktrees: number;
  /** The icon found in the repo, as a data URL, and its path in the repo. */
  icon: string | null;
  icon_path: string | null;
  /** Why the folder cannot be added. */
  error: string | null;
}

export type SplitDir = "row" | "col";

/** `row`: a left, b right. `col`: a top, b bottom. `ratio` is the share of `a`. */
export type Layout =
  | { type: "pane"; session: string }
  | { type: "split"; dir: SplitDir; ratio: number; a: Layout; b: Layout };

/** A named split kept by the daemon. `id` is empty until the daemon assigns one. */
export interface Group {
  id: string;
  name: string;
  layout: Layout;
  focus: string | null;
  /** Where the group's empty panes start their sessions. */
  cwd?: string | null;
}

export interface TerminalTheme {
  /** builtin:<slug> or file:<name>. */
  id: string;
  name: string;
  source: "built-in" | "file";
  foreground: string;
  background: string;
  cursor: string | null;
  selection: string | null;
  /** ANSI 0–15. */
  palette: string[];
  /** App colors from the theme file (Superset's ui block), when it has them. */
  ui?: Record<string, string>;
}

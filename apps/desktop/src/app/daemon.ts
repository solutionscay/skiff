/** skiffd: sessions, projects, events. */
import { newSession } from "./newSession";
import { work } from "../diagnostics/stalls";

import type { AgentInfo, Appearance, DaemonEvent, DaemonStatus, Project, Reloaded, SessionInfo } from "../platform/types";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { $, h } from "../ui/dom";
import { rune } from "../ui/runes";
import { showError } from "../ui/alerts";
import { daemonBadge, restartIfIdle } from "./daemonRestart";
import { reloadNote } from "./daemonNotice";
import { reloadChanges } from "../workspace/changes";
import { projectThemesFrom } from "../appearance/themes";
import { filesFrom } from "../workspace/files";
import { loadGroups } from "./groups";
import { render, scheduleRender } from "./render";
import { born, gone, OTHER, removeErrors, S, sessions, stateSince, upsert } from "./state";
import { hasKeys, markSeen, quietSeen } from "./seen";
import { peekExited, peekOwner, peekUpdated, strayPeek } from "../terminal/peek";
import { awayPlaces, currentWorktree, place } from "./stateQueries";
import { agentKind } from "../appearance/agentIcon";
import { loadSkiffAgent, noteDaemon, skipperCreated } from "./skiffAgent";
import { isFolder, taskTitle } from "../workspace/model";
import { loadSkipperAvatars } from "../workspace/skipperFace";
import { ptySized } from "../terminal/terminal";
import { reattachPanes } from "../terminal/terminalRuntime";
import { t } from "../i18n";
import { hostCan, hostInvoke, loadHostCaps } from "./hostCaps";
import { hostName, isRemote, shownHost } from "./hosts";
import { watchDataPlane } from "./dataPlane";
import { refreshHostDiff } from "../terminal/peek";
import { reloadFiles } from "../workspace/files";
import { reloadBackdrops } from "../appearance/backdrop";
import { loadLedger } from "../workspace/ledger";
import { includeNew } from "../workspace/profile";
import { muteInput } from "../terminal/terminalRuntime";
import { connectAttempt, connectDone, connectFailed, connectWait } from "./connectPanel";
import { dropSession } from "./sessionRemoval";
import { sessionGone, stateMoved } from "./notify";

export { newSession };

export function setDaemon(status: DaemonStatus) {
  const dot = $("daemon").querySelector(".dot") as HTMLElement;
  dot.className = "dot " + (status.connected ? "connected" : "error");
  const version = status.version ?? "";
  const where = isRemote() ? `${hostName()} · ` : "";
  $("daemon-label").textContent = where + (status.reload
    ? t("daemon.reloading")
    : status.connected
    ? status.replaced
      ? t("daemon.updatedFrom", { version, old: status.replaced })
      : t(status.spawned ? "daemon.started" : "daemon.version", { version })
    : status.warning?.kind === "hung"
      ? t("daemon.badgeHung")
      : isRemote()
        ? t("daemon.disconnected")
        : t("daemon.unreachable"));
  const socket = $("socket");
  if (status.pid) socket.replaceChildren(t("daemon.socketPid", { socket: status.socket, pid: status.pid }), pidCopy(status.pid));
  else socket.textContent = status.socket;
  daemonBadge(status.warning);
  noteDaemon(status);
  // The welcome page names the version gap the badge shows.
  if (isRemote()) scheduleRender();
}

/** Counts event subscriptions. An event from an older one is dropped. */
let eventStream = 0;
let dataEventStream = 0;

/** Streams daemon events to `onEvent`. Replaces an earlier subscription. */
export async function subscribeEvents() {
  const n = ++eventStream;
  const events = new Channel<DaemonEvent>();
  events.onmessage = (e) => {
    if (n === eventStream) onEvent(e);
  };
  await hostInvoke("session_updates", "subscribe_events", { onEvent: events });
}

/** Pushed Host data invalidations. A gap is intentionally a broad refresh. */
export async function subscribeDataEvents() {
  if (!hostCan("data_events")) return;
  const n = ++dataEventStream;
  const events = new Channel<{ projects: boolean; worktrees: string[]; tasks: boolean; gap: boolean }>();
  events.onmessage = (e) => {
    if (n !== dataEventStream) return;
    if (e.projects || e.gap) void loadProjects();
    if (e.tasks || e.gap) void loadLedger();
    if (e.gap) { reloadFiles(); reloadChanges(); }
    else { reloadChanges(e.worktrees); reloadFiles(); }
  };
  await invoke("subscribe_data_events", { onEvent: events });
}

/**
 * Moves an older skiffd onto the app's own, with its sessions. Rust holds
 * terminal input meanwhile and sends it on once the reload is proven. The
 * page, layout, focus and scroll stay; shown panes redraw from a snapshot.
 * The launch starts it, not the user: an update it leaves unfinished shows
 * in the badge and the status bar, with no dialog.
 */
export async function reloadDaemon(): Promise<DaemonStatus> {
  return runReload("reload_daemon");
}

/** Update skiffd, from an error bar: the same move, asked for by the
 *  person now, whatever the launch did. Rust refuses when the daemon is
 *  not older or cannot hand over; the refusal is the error shown. */
export async function updateDaemon(): Promise<DaemonStatus> {
  return runReload("update_daemon");
}

async function runReload(command: "reload_daemon" | "update_daemon"): Promise<DaemonStatus> {
  $("daemon-label").textContent = t("daemon.reloading");
  // Nothing the reconnect redraws counts as seen.
  quietSeen(true);
  try {
    const r = await invoke<Reloaded>(command);
    // Before boot subscribed anything, boot does it next.
    if (r.reconnected && r.status.connected && eventStream) {
      // The reloaded Host answers for itself. Events first, so nothing
      // between the list and the stream is missed.
      await loadHostCaps().catch(console.error);
      await subscribeEvents().catch(console.error);
      await refreshSessions().catch(console.error);
      await loadGroups().catch(console.error);
      reattachPanes();
    }
    setDaemon(r.status);
    const note = reloadNote(r);
    $("daemon").title = note ?? "";
    if (note) console.warn(`skiffd reload: ${note}`);
    render();
    return r.status;
  } finally {
    quietSeen(false);
  }
}

/** Asks for the selected Host until it answers: 1 s, then twice as long
 *  each time, up to 30 s. Each try runs SSH and the handshake again. */
export async function untilConnected(first = 0): Promise<DaemonStatus> {
  let wait = 1000;
  if (first) await connectWait(first);
  for (;;) {
    const remote = isRemote();
    if (remote) connectAttempt();
    const status = await invoke<DaemonStatus>("daemon_status").catch(() => null);
    if (status) setDaemon(status);
    if (status?.connected) {
      connectDone();
      return status;
    }
    if (remote) connectFailed(failureLine(status), wait);
    // Try again in the connecting panel cuts the wait short.
    await connectWait(wait);
    wait = Math.min(wait * 2, 30_000);
  }
}

/** The one plain line a failed connect shows: the cause Rust named. */
export function failureLine(status: DaemonStatus | null): string {
  return status?.warning?.message ?? t("daemon.hostSilent", { host: hostName() });
}

/**
 * The link to a remote Host dropped. Input is off until it is back; nothing
 * typed meanwhile is kept or sent later. Then the same steps as after a
 * reload: the Host's capabilities, events first, the list, groups, its
 * agents, and each shown pane from a fresh snapshot. The agents kept from
 * before go first: the Host may run other commands now, and a launch menu
 * opened meanwhile offers a shell only. A step that fails throws, so the caller
 * runs the whole recovery again; it never turns input back on itself (see
 * `watchHostLink`). A different Host at the same address never connects
 * (Rust refuses it), so this keeps waiting and the status says why.
 */
export async function reconnectRemote(): Promise<void> {
  muteInput(true);
  quietSeen(true);
  S.agents = [];
  try {
    const status = await untilConnected();
    await loadHostCaps();
    await subscribeEvents();
    await refreshSessions();
    if (hostCan("session_groups")) await loadGroups();
    if (hostCan("session_agents")) S.agents = await hostInvoke<AgentInfo[]>("session_agents", "list_agents");
    reattachPanes();
    setDaemon(status);
    render();
  } finally {
    quietSeen(false);
  }
}

/** A full recovery with no loss in it: input goes back on. */
export function hostOnline() {
  muteInput(false);
  render();
}

let copiedTimer: number | undefined;

/** A small button after the pid that copies the number. */
function pidCopy(pid: number): HTMLButtonElement {
  const b = h("button", "status-copy");
  b.type = "button";
  b.title = t("daemon.copyPid");
  b.setAttribute("aria-label", t("daemon.copyPidN", { pid }));
  b.appendChild(rune("code-copy", 12));
  b.addEventListener("click", () => {
    navigator.clipboard.writeText(String(pid)).then(() => {
      b.classList.add("copied");
      b.title = t("daemon.copied");
      clearTimeout(copiedTimer);
      copiedTimer = window.setTimeout(() => {
        b.classList.remove("copied");
        b.title = t("daemon.copyPid");
      }, 1200);
    }, showError);
  });
  return b;
}

let projectsSeq = 0;

/** The app theme from projects.toml. The `skiff` CLI can change it while the app runs. */
async function loadAppTheme() {
  const a = await invoke<Appearance>("get_appearance").catch(() => null);
  if (a) S.appTheme = a.theme;
}

export async function loadProjects() {
  const seq = ++projectsSeq;
  void loadSkipperAvatars();
  // The agents are the selected Host's own, from its account: a remote
  // Host's never come from this Mac.
  if (!S.agents.length) S.agents = await hostInvoke<AgentInfo[]>("session_agents", "list_agents").catch(() => []);
  // Projects come from the selected Host's data plane (skiff-host), on This
  // Mac too. A remote Host without it shows its sessions only, never this
  // Mac's projects under its name. This Mac's service starts on demand, so
  // its read is tried even before it is offered: a failure says why.
  const remote = isRemote();
  const host = shownHost() ?? "";
  if (remote && !hostCan("projects")) {
    // Skipper is the Host's own (`skipper@1`), with or without projects there.
    await loadSkiffAgent();
    if (seq !== projectsSeq) return;
    S.projects = [];
    S.closedProjects = [];
    S.projectsError = null;
    // The data plane check says why (dataPlane.ts), and reads again once it answers.
    S.dataState = "unavailable";
    if (S.selectedProject !== OTHER) S.selectedProject = null;
    render();
    return;
  }
  let list: Project[] | null = null;
  let err: string | null = null;
  const started = performance.now();
  try {
    const read = invoke<Project[]>("list_projects").finally(() => work("projects refresh (worktrees, branches)", performance.now() - started));
    // The app theme is this computer's: a remote Host does not get it. Skipper is the selected Host's.
    [list] = await Promise.all(remote ? [read, loadSkiffAgent()] : [read, loadAppTheme(), loadSkiffAgent()]);
  } catch (e) {
    err = String(e);
  }
  // A later call started while this one waited: its answer is newer.
  if (seq !== projectsSeq) return;
  if (list) {
    // Files, Changes and theme are the Host's project settings, read with its rows.
    filesFrom(list);
    projectThemesFrom(list);
    const open = list.filter((p) => !p.closed);
    // The same list keeps its objects: the tree and place() compare worktrees by identity.
    if (JSON.stringify(open) !== JSON.stringify(S.projects)) S.projects = open;
    S.closedProjects = list.filter((p) => p.closed);
    // A project added outside the app joins the open workspace (#229).
    includeNew(host, list);
  }
  S.projectsError = err;
  S.dataState = err ? "unavailable" : "ready";
  S.dataNote = err;
  if (S.selectedProject !== OTHER && !S.projects.some((p) => p.name === S.selectedProject)) {
    const f = S.focused ? sessions.get(S.focused) : undefined;
    S.selectedProject = (f && place(f)?.project.name) || S.projects[0]?.name || null;
  }
  for (const [path] of removeErrors) {
    if (!S.projects.some((p) => p.worktrees.some((w) => w.path === path))) removeErrors.delete(path);
  }
  render();
}

/**
 * Follows the selected Host's data plane from now on (dataPlane.ts). A new
 * data connection reads its capabilities, projects, Skipper's settings, the
 * ledger, Git changes and a remote diff or result on screen again; a lost
 * one says why. Sessions, panes and
 * terminal streams stay as they are.
 */
export function followDataPlane() {
  watchDataPlane({
    async reload() {
      const caps = await loadHostCaps().then(
        () => true,
        (e) => (console.error(e), false),
      );
      await subscribeDataEvents().catch(console.error);
      // The ledger too: its watcher sees no change in a ledger that did not change.
      await Promise.all([loadProjects(), loadLedger()]);
      reloadChanges();
      reloadFiles();
      reloadBackdrops();
      refreshHostDiff();
      // Either read failed: the next check on this connection reads again.
      return caps && S.projectsError === null;
    },
    down(message) {
      S.dataState = "unavailable";
      S.dataNote = message;
      render();
    },
    fresh: () => S.dataState === "ready" && hostCan("projects"),
  });
}

let staleTimer: number | undefined;

/** The worktrees to read changed files for when the burst settles. Null: every known one. */
let stale: Set<string> | null = new Set();

/**
 * Reads the worktrees of the projects that hold `paths` again, and the
 * changed files of `paths`, once a burst of calls settles. Both change
 * outside Skiff: an agent's `git worktree add`, an editor's save. Without
 * `paths`, every project and every known worktree is read.
 */
function reposStale(paths?: Iterable<string>) {
  if (!paths) stale = null;
  else if (stale) for (const p of paths) stale.add(p);
  window.clearTimeout(staleTimer);
  staleTimer = window.setTimeout(() => {
    const only = stale;
    stale = new Set();
    if (only) void reloadProjectsOf(only);
    else void loadProjects();
    reloadChanges(only ?? undefined);
  }, 400);
}

/**
 * Reads again only the projects that hold `paths`: an agent that stops in
 * one project runs git in that project alone. A project that reads the same
 * keeps its object, so place() keeps its cache.
 */
async function reloadProjectsOf(paths: Set<string>) {
  // A plain folder has no git to read after a turn. A full read still finds a new .git.
  const names = S.projects.filter((p) => !isFolder(p) && p.worktrees.some((w) => paths.has(w.path))).map((p) => p.name);
  if (!names.length) return;
  const seq = projectsSeq;
  const fresh = await Promise.all(names.map((name) => invoke<Project>("get_project", { name }).catch(() => null)));
  // A full read started meanwhile: its answer is newer.
  if (seq !== projectsSeq) return;
  let next = S.projects;
  for (const p of fresh) {
    const i = p ? next.findIndex((o) => o.name === p.name) : -1;
    if (!p || p.closed || i < 0 || JSON.stringify(next[i]) === JSON.stringify(p)) continue;
    if (next === S.projects) next = [...next];
    next[i] = p;
  }
  if (next === S.projects) return;
  S.projects = next;
  render();
}

/** Between two full reads asked for by sessions outside every project. */
export const UNPLACED_EVERY_MS = 10_000;
/** Folders outside every project whose session asked for a full read once. */
const unplacedAsked = new Set<string>();
/** A full read for such a session ran less than UNPLACED_EVERY_MS ago. */
let unplacedCooling = false;
/** A folder asked meanwhile: one more full read when the wait ends. */
let unplacedPending = false;

/**
 * A new session in a folder no known project holds. The folder may be a
 * project added outside the app (`skiff projects add`), which a read of the
 * known projects never finds (reloadProjectsOf): the projects are read in
 * full, once per folder and at most once per UNPLACED_EVERY_MS. A folder
 * asked within the wait is read when it ends. Still outside every project,
 * the session stays under Other.
 */
export function unplaced(s: SessionInfo) {
  if (S.dataState !== "ready" || place(s) || unplacedAsked.has(s.cwd)) return;
  unplacedAsked.add(s.cwd);
  if (unplacedCooling) unplacedPending = true;
  else unplacedRead();
}

function unplacedRead() {
  unplacedCooling = true;
  unplacedPending = false;
  void loadProjects();
  window.setTimeout(() => {
    unplacedCooling = false;
    if (unplacedPending) unplacedRead();
  }, UNPLACED_EVERY_MS);
}

/** The worktrees the session works in: its own, and the ones its processes ran in. */
function worktreesOf(s: SessionInfo): string[] {
  const at = place(s);
  return [...(at ? [at.worktree.path] : []), ...awayPlaces(s).map((a) => a.worktree.path)];
}

/** Worktrees with a session that worked while the window was not in front. */
const workedAway = new Set<string>();
let windowAway = !document.hasFocus();

// projects.toml or a worktree changed: an edit of the app's, or one from
// outside it that the projects watcher saw (`skiff projects add`, a hand edit).
void listen("skiff:projects-changed", () => void loadProjects());
window.addEventListener("blur", () => {
  windowAway = true;
  for (const s of sessions.values()) if (s.state === "working") for (const p of worktreesOf(s)) workedAway.add(p);
});
// Back from a terminal, an editor or a diff tool: the worktree in front may have
// changed, and so may the ones agents worked in meanwhile.
window.addEventListener("focus", () => {
  windowAway = false;
  const at = currentWorktree();
  if (at) workedAway.add(at.w.path);
  reposStale(workedAway);
  workedAway.clear();
});
// The focused pane is in front of the user again: its result is seen.
window.addEventListener("focus", () => {
  if (S.focused) scheduleRender("focus");
});

/**
 * What the tree and the headers show for the session: its title and its icon.
 * An agent's leading status glyph is left out, so a spinner frame is not a
 * change. A shell shows its raw title, so a shell keeps every change.
 */
function shownTitle(s: SessionInfo): string {
  const title = taskTitle(s);
  return `${s.role === "shell" ? title : title.replace(/^[^\p{L}\p{N}]+/u, "")}\0${agentKind(s)}`;
}

/**
 * A session as the tree and the headers show it: `last_output_at` moves with
 * every poll, and `input_gen` with every key typed. Neither shows.
 */
const shownSessions = () => JSON.stringify([...sessions.values()].map((s) => ({ ...s, last_output_at: 0, input_gen: 0 })));

/**
 * Pulls `last_output_at` and anything missed, so relative times stay true.
 * Drops what the daemon no longer has. True when something shown changed.
 */
export async function refreshSessions(): Promise<boolean> {
  const before = shownSessions();
  const fresh = new Set<string>();
  born.add(fresh);
  let list: SessionInfo[];
  try {
    list = await hostInvoke<SessionInfo[]>("session_inventory", "list_sessions");
  } finally {
    born.delete(fresh);
  }
  // Peek commands are not sessions to the user. One that is not the open
  // peek's was left behind, or is an app that runs on.
  if (list.some((s) => s.peek)) {
    const me = await peekOwner();
    for (const s of list) if (s.peek) strayPeek(s, me);
  }
  list = list.filter((s) => !s.peek);
  const listed = new Set(list.map((s) => s.id));
  for (const s of list) if (!gone.has(s.id)) upsert(s);
  for (const id of [...sessions.keys()]) if (!listed.has(id) && !fresh.has(id)) dropSession(id);
  return shownSessions() !== before;
}

export function onEvent(e: DaemonEvent) {
  switch (e.event) {
    case "title": {
      const s = sessions.get(e.session);
      if (!s) return;
      // A spinner frame changes the title string, not what is shown: no render.
      const before = shownTitle(s);
      s.title = e.title;
      if (shownTitle(s) === before) return;
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
        stateMoved(s.id, e.state);
        if (e.state === "working") {
          s.last_output_at = Date.now();
          if (windowAway) for (const p of worktreesOf(s)) workedAway.add(p);
        }
        // An agent that stops working may have written files or added a worktree: its worktrees are read again.
        else reposStale(worktreesOf(s));
      }
      restartIfIdle();
      break;
    }
    case "exit": {
      // A signal: the code says nothing about the program. No code means no status to report.
      if (peekExited(e.session, e.signaled ? null : e.code)) return;
      const s = sessions.get(e.session);
      if (s) {
        if (s.state !== "done") stateSince.set(s.id, Date.now());
        s.state = "done";
        s.exit_code = e.code;
        // A shell that exits (Ctrl+D, `exit`) has nothing left to show: it closes.
        if (s.role === "shell") hostInvoke("session_lifecycle", "kill_session", { session: s.id }).catch(console.error);
      }
      break;
    }
    case "session_created":
      if (gone.has(e.session.id) || e.session.peek) return;
      for (const b of born) b.add(e.session.id);
      upsert(e.session);
      // A restore brings the id back with the size the daemon saved.
      ptySized(e.session);
      skipperCreated(e.session);
      unplaced(e.session);
      break;
    case "session_updated": {
      if (e.session.peek) return peekUpdated(e.session);
      if (!sessions.has(e.session.id)) break;
      upsert(e.session);
      ptySized(e.session);
      // A turn that ends in the pane with the keys leaves nothing to read.
      if (hasKeys(e.session.id)) markSeen(e.session.id);
      break;
    }
    case "session_removed": {
      dropSession(e.session);
      sessionGone(e.session);
      restartIfIdle();
      return;
    }
    case "groups_changed":
      void loadGroups();
      return;
    // The state event before it brought the idle state, and session_updated the unread mark.
    case "turn_ended":
      return;
  }
  scheduleRender(`event.${e.event}`);
}

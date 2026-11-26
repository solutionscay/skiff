import { listen } from "@tauri-apps/api/event";
import {
  Channel,
  accepts,
  forgetRoute,
  invoke,
  invokeAt,
  pageRoute,
  registerRoute,
  routeOf,
  showRoute,
  withinHost,
  type HostEvent,
  type HostRoute,
} from "../platform/invoke";
import type { DaemonEvent, DaemonStatus, Group, Project, SessionInfo } from "../platform/types";
import { stateMoved, sessionGone } from "./notify";
import { S, sessions, upsert } from "./state";
import { reloadDaemon, onEvent, loadProjects, setDaemon, subscribeDataEvents } from "./daemon";
import { setHostConnected, hostLabel, knownHosts, loadHosts } from "./hosts";
import { render, scheduleRender } from "./render";
import { loadHostCaps } from "./hostCaps";
import { restoreWorkspace } from "../workspace/profile";
import { loadLedger } from "../workspace/ledgerStore";
import { view } from "../terminal/terminal";
import { muteInput, parkPane, reattachPanes } from "../terminal/terminalRuntime";
import { panes } from "../terminal/terminalState";
import { closePreview, reattachPeeks } from "../terminal/peek";
import { trackPlace } from "./stored";
import { filesFrom } from "../workspace/files";
import { type SkiffAgentSettings } from "./skiffAgent";
import { t } from "../i18n";

export interface Connection {
  entry: number;
  endpoint: string | null;
  phase: "connecting" | "up" | "failed";
  lines: string[];
  since: number;
  next: number;
  wait: number;
  attempt: number;
  timer?: number;
  status?: DaemonStatus;
  folded: boolean;
  visited: number;
  restored: boolean;
}

const connections = new Map<number, Connection>();
let visit = 0;
let restore: (id: string | null) => void = () => {};
export const connection = (entry: number) => connections.get(entry);
export const activeEntry = () => pageRoute()?.entry;
export function orderedConnections() {
  const ids = [null, ...knownHosts().map((h) => h.id)];
  return [...connections.values()].sort((a, b) => ids.indexOf(a.endpoint) - ids.indexOf(b.endpoint));
}
export const canDisconnect = (c: Connection) => [...connections.values()].some((h) => h !== c && h.phase === "up");
export const titleOfHost = (c: Connection) => hostLabel(c.endpoint);
const changed = () => {
  scheduleRender("hosts");
  window.dispatchEvent(new Event("skiff-host-panel"));
};

function add(route: HostRoute) {
  registerRoute(route);
  const found = connections.get(route.entry);
  if (found) return found;
  const c: Connection = {
    entry: route.entry,
    endpoint: route.endpoint,
    phase: "connecting",
    lines: [],
    since: Date.now(),
    next: 0,
    wait: 1000,
    attempt: 0,
    folded: false,
    visited: 0,
    restored: false,
  };
  connections.set(c.entry, c);
  return c;
}

export async function connectEndpoint(id: string | null) {
  const existing = [...connections.values()].find((c) => c.endpoint === id);
  if (existing) return;
  const route = await invoke<HostRoute>("connect_host", { id });
  const c = add(route);
  if (!c.attempt) void attempt(c);
  changed();
}

export async function disconnect(c: Connection) {
  if (!canDisconnect(c)) return;
  const route = routeOf(c.entry)!;
  await invokeAt(route, "disconnect_host");
  clearTimeout(c.timer);
  c.attempt++;
  const next = [...connections.values()].filter((h) => h !== c && h.phase === "up").sort((a, b) => b.visited - a.visited)[0];
  if (activeEntry() === c.entry && next) focusConnection(next);
  withinHost(route, () => {
    for (const p of panes.values()) {
      p.stream++;
      p.term.dispose();
      p.el.remove();
    }
  });
  connections.delete(c.entry);
  forgetRoute(c.entry);
  changed();
}

function rememberPlace(c: Connection) {
  if (c.restored) return;
  c.restored = true;
  restore(c.endpoint);
  trackPlace(c.endpoint, pageRoute()?.host ?? undefined);
}

export function focusConnection(c: Connection, restoreView = true) {
  const next = routeOf(c.entry);
  if (!next?.host || activeEntry() === c.entry) return;
  closePreview();
  for (const [id, p] of panes) parkPane(id, p);
  view.sync(null);
  showRoute(next);
  document.querySelector(".focused-host")?.classList.remove("focused-host");
  document.querySelector('[data-host-entry="' + c.entry + '"]')?.classList.add("focused-host");
  c.visited = ++visit;
  trackPlace(c.endpoint, next.host);
  muteInput(c.phase !== "up");
  setHostConnected(c.phase === "up");
  if (c.status) setDaemon(c.status);
  if (restoreView) rememberPlace(c);
  else c.restored = true;
  if (c.phase === "up")
    void refreshFocused(next).then(() => {
      if (accepts(next)) void restoreWorkspace();
    });
  void loadHosts().catch(console.error);
  scheduleRender("host-focus");
}

async function refreshFocused(route: HostRoute) {
  try {
    await loadHostCaps();
    if (!accepts(route)) return;
    await Promise.all([loadProjects(), loadLedger(), subscribeDataEvents().catch(console.error)]);
  } catch (e) {
    if (accepts(route)) console.error(e);
  }
}

// A state change or removal received during a snapshot wins over that snapshot.
const inventoryChanges = new WeakMap<Connection, Set<string>>();

async function inventory(c: Connection, route: HostRoute) {
  const generation = c.attempt;
  const touched = new Set<string>();
  inventoryChanges.set(c, touched);
  try {
    const list = await invokeAt<SessionInfo[]>(route, "list_sessions");
    const groups = await invokeAt<Group[]>(route, "list_groups").catch(() => null);
    if (!connections.has(c.entry) || generation !== c.attempt || routeOf(c.entry)?.generation !== route.generation) return;
    withinHost(route, () => {
      const live = new Set(list.filter(s => !s.peek).map(s => s.id));
      for (const id of sessions.keys()) if (!live.has(id) && !touched.has(id)) {
        sessions.delete(id);
        sessionGone(id);
      }
      for (const s of list) if (!s.peek && !touched.has(s.id)) upsert(s);
      if (groups) S.groups = groups.map(g => ({ ...g, hostId: route.host ?? undefined }));
    });
  } finally {
    if (inventoryChanges.get(c) === touched) inventoryChanges.delete(c);
  }
}

async function readBackground(c: Connection, route: HostRoute) {
  const [projects, agent] = await Promise.all([
    invokeAt<Project[]>(route, "list_projects").catch(() => null),
    invokeAt<SkiffAgentSettings>(route, "get_skiff_agent").catch(() => null),
  ]);
  if (!connections.has(c.entry) || routeOf(c.entry)?.generation !== route.generation || accepts(route)) return;
  withinHost(route, () => {
    if (projects) {
      S.dataState = "ready";
      S.projects = projects.filter((p) => !p.closed);
      S.closedProjects = projects.filter((p) => p.closed);
      filesFrom(projects);
    }
    if (agent) S.skiffAgent = agent;
  });
}

function poll(c: Connection) {
  clearTimeout(c.timer);
  c.timer = window.setTimeout(async () => {
    if (c.phase !== "up" || !connections.has(c.entry)) return;
    const route = routeOf(c.entry)!;
    if (!accepts(route)) {
      try {
        await inventory(c, route);
        await readBackground(c, route);
        changed();
      } catch {
        /* Loss is reported by the event stream. */
      }
    }
    if (c.phase === "up" && connections.has(c.entry)) poll(c);
  }, 10000);
}

async function attempt(c: Connection) {
  clearTimeout(c.timer);
  const n = ++c.attempt;
  const current = () => connections.has(c.entry) && c.attempt === n;
  c.phase = "connecting";
  c.since = Date.now();
  c.lines = [];
  c.next = 0;
  changed();
  try {
    const status = await invokeAt<DaemonStatus>(routeOf(c.entry)!, "daemon_status");
    if (!current()) return;
    c.status = status;
    if (!c.status.connected) throw new Error(c.status.warning?.message ?? t("daemon.hostSilent", { host: titleOfHost(c) }));
    if (c.status.reload && activeEntry() === c.entry) c.status = await reloadDaemon();
    const route = routeOf(c.entry)!;
    const events = new Channel<HostEvent<DaemonEvent>>();
    events.onmessage = ({ route: from, value }) => {
      if (!current() || from.generation !== routeOf(c.entry)?.generation) return;
      if (inventoryChanges.has(c) && "session" in value) {
        const id = typeof value.session === "string" ? value.session : value.session.id;
        if (value.event === "session_removed" || value.event === "session_created" || withinHost(from, () => sessions.has(id))) {
          inventoryChanges.get(c)?.add(id);
        }
      }
      if (accepts(from)) onEvent(value);
      else
        withinHost(from, () => {
          if (value.event === "session_created" || value.event === "session_updated") {
            if (!value.session.peek) upsert(value.session);
          } else if (value.event === "session_removed") {
            sessions.delete(value.session);
            sessionGone(value.session);
          } else if (value.event === "exit") {
            const s = sessions.get(value.session);
            if (s) s.state = "done";
          } else if (value.event === "state") {
            const s = sessions.get(value.session);
            if (s) {
              s.state = value.state;
              stateMoved(s.id,value.state);
            }
          } else if (value.event === "title") {
            const s = sessions.get(value.session);
            if (s) s.title = value.title;
          }
          scheduleRender("background-session");
        });
    };
    await invokeAt(route, "subscribe_events", { onEvent: events });
    await inventory(c, route);
    if (!current()) return;
    c.phase = "up";
    c.wait = 1000;
    if (accepts(route)) {
      S.agents = [];
      setDaemon(c.status);
      await refreshFocused(route);
      if (accepts(route) && current()) {
        setHostConnected(true);
        muteInput(false);
        rememberPlace(c);
        reattachPanes();
        reattachPeeks();
      }
    } else await readBackground(c, route);
    if (!current()) return;
    void invokeAt(route, "remember_hosts").catch(console.error);
    poll(c);
  } catch (error) {
    if (!current()) return;
    c.phase = "failed";
    if (activeEntry() === c.entry) {
      muteInput(true);
      setHostConnected(false);
      if (c.status) setDaemon({ ...c.status, connected: false });
    }
    c.lines.push(String(error).replace(/^Error: /, ""));
    c.next = Date.now() + c.wait;
    c.timer = window.setTimeout(() => void attempt(c), c.wait);
    c.wait = Math.min(30000, c.wait * 2);
  }
  changed();
}

export const retry = (c: Connection) => {
  if (c.phase === "failed") void attempt(c);
};

export async function startConnections(restoreView: typeof restore) {
  restore = restoreView;
  await listen<HostEvent<string>>("skiff:connect-line", ({ payload }) => {
    const c = connection(payload.route.entry);
    if (!c || c.phase !== "connecting" || routeOf(c.entry)?.generation !== payload.route.generation) return;
    c.lines.push(((Date.now() - c.since) / 1000).toFixed(1) + " s  " + payload.value);
    window.dispatchEvent(new Event("skiff-host-panel"));
  });
  await listen<HostRoute>("skiff:host-lost", ({ payload }) => {
    const c = connection(payload.entry);
    if (!c || payload.generation !== routeOf(c.entry)?.generation) return;
    if (c.status) c.status = { ...c.status, connected: false };
    if (accepts(payload)) {
      muteInput(true);
      setHostConnected(false);
      if (c.status) setDaemon(c.status);
    }
    void attempt(c);
  });
  const initial = (await invoke<HostRoute[]>("host_routes")).map(add);
  for (const c of initial) if (c.entry !== activeEntry()) void attempt(c);
  const first = initial.find((c) => c.entry === activeEntry());
  if (first) {
    first.visited = ++visit;
    await attempt(first);
  }
  render();
}

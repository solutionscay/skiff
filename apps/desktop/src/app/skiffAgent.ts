import { invoke } from "@tauri-apps/api/core";
import { showError } from "../ui/alerts";
import type { AgentInfo, DaemonStatus, SessionInfo } from "../platform/types";
import { setSkiffAgentName } from "./keys";
import { render } from "./renderRequest";
import { S, sessions, upsert } from "./state";
import { place } from "./stateQueries";
import { agentKind } from "../appearance/agentIcon";
import { keepRow } from "../workspace/selection";
import { revealSession } from "../workspace/view";
import { openPath } from "../terminal/peek";
import { actionDialog } from "../ui/dialogParts";
import { h } from "../ui/dom";
import { scopeName } from "../workspace/model";

/** The name until Settings has one. skiffd's setup keeps the same default. */
export const DEFAULT_NAME = "Skipper";

/** The row key while no Skiff agent session runs. */
export const SKIFF_AGENT_KEY = "skiff-agent";

/** `[skiff_agent]` in projects.toml. */
export interface SkiffAgentSettings {
  name: string | null;
  agent: string | null;
  /** The row shows and the folder is set up. */
  enabled: boolean;
  /** `~/skiff-agent`, or the override in projects.toml. Null when the override is refused. */
  folder: string | null;
  /** Why the override is refused. */
  folder_error: string | null;
  /** The folder projects.toml names, refused or not. */
  configured: string;
  /** ready: a Skiff folder. missing: Skiff could not make it. other: it holds other files. */
  folder_status: "ready" | "missing" | "other" | "refused";
  continue_on_restore: boolean;
  /** The project menu offers Start project Skipper. */
  project_skippers: boolean;
  /** The agent project Skippers start. Null: the Skipper's. */
  project_agent: string | null;
  /** `[skiff_agent.tasks]` in file order. */
  tasks: TaskType[];
}

/** One task type: Skipper starts a task of this type with this agent and model. */
export interface TaskType {
  type: string;
  agent: string;
  /** Null: the agent's default. Always null for an agent that takes its model in its command. */
  model: string | null;
  /** The command line skiffd builds. Null when the agent is gone. */
  command: string | null;
  /** Why the row does not start what it says. */
  error: string | null;
}

/** The folder while the Skiff agent is on and its folder is good. */
export function skiffAgentFolder(): string | null {
  return S.skiffAgent?.enabled ? S.skiffAgent.folder : null;
}

/** Reads `[skiff_agent]` again. loadProjects calls it, so an edit from Settings or the CLI shows at once. */
export async function loadSkiffAgent() {
  const a = await invoke<SkiffAgentSettings>("get_skiff_agent").catch(() => null);
  S.skiffAgent = a;
  setSkiffAgentName(skiffAgentName());
}

export function skiffAgentName(): string {
  return S.skiffAgent?.name?.trim() || DEFAULT_NAME;
}

/**
 * The agent the row starts: the one from Settings while it is installed and
 * on (a custom agent is on), else the first enabled one. Settings shows the same.
 */
export function skiffAgentChoice(agents: AgentInfo[] = S.agents, id = S.skiffAgent?.agent): AgentInfo | null {
  return agents.find((a) => a.id === id && a.installed && (a.enabled || a.custom)) ?? agents.find((a) => a.enabled) ?? null;
}

/**
 * The Skiff agent's session: the one skiffd marks as the coordinator. The
 * daemon keeps the mark across reloads and restores, and `skiff skipper`
 * finds the same session.
 */
export function skiffAgentSession(): SessionInfo | null {
  if (!skiffAgentFolder()) return null;
  return [...sessions.values()].find((s) => s.coordinator && s.state !== "done") ?? null;
}

/** True for the Skiff agent's session. It has no project, no splits and no rename: its name lives in Settings. */
export const isSkiffAgent = (id: string | null | undefined) => !!id && skiffAgentSession()?.id === id;

/**
 * The scope a live project or fleet Skipper coordinates, else null: a
 * project's name, or "fleet:<fleet>". skiffd marks it with its scope. A
 * restore brings one back with no mark, and an older skiffd starts one with
 * no mark (the CLI then says it is too old, but the agent runs): its name,
 * "<project> <Skiff agent's name>", still finds a project Skipper. A
 * retiring one is a plain session.
 */
function skipperProject(s: SessionInfo): string | null {
  if (s.coordinator || s.state === "done" || s.retiring) return null;
  if (s.skipper) return s.skipper;
  if (!skiffAgentFolder()) return null;
  const tail = ` ${skiffAgentName()}`;
  const project = s.name?.endsWith(tail) ? s.name.slice(0, -tail.length) : null;
  return project && S.projects.some((p) => p.name === project) ? project : null;
}

/** The live project Skippers, marked or found by name. A marked one counts
 *  while the Skiff agent's settings are off or not read yet: it runs, and its
 *  title already says so. */
export function projectSkippers(): SessionInfo[] {
  return [...sessions.values()].filter((s) => skipperProject(s) !== null);
}

/** The live Skipper of a scope: a project, or "fleet:<fleet>". A marked one comes first. */
export function projectSkipperOf(project: string): SessionInfo | null {
  const all = projectSkippers().filter((s) => skipperProject(s) === project);
  return all.find((s) => s.skipper === project) ?? all[0] ?? null;
}

/** The live fleet Skipper of `fleet`, if one runs. */
export const fleetSkipperOf = (fleet: string) => projectSkipperOf(fleetScope(fleet));

/** A fleet's scope, as its Skipper's mark and its rows' owner carry it. */
export const fleetScope = (fleet: string) => `fleet:${fleet}`;

/** The coordinator whose session `id` is: "grand" for the Skiff agent, a scope for a project or fleet Skipper, else null. */
export function coordinatorOf(id: string | null | undefined): string | null {
  if (!id) return null;
  if (isSkiffAgent(id)) return "grand";
  const s = projectSkippers().find((s) => s.id === id);
  return s ? skipperProject(s) : null;
}

/** Project menu › Start project Skipper. `skiff skipper --project` starts it in the project's folder and sends SKIPPER.md as its first message. */
export function startProjectSkipper(project: string) {
  const s = projectSkipperOf(project);
  if (s) return show(s.id);
  void starting(project, () => invoke<string>("start_project_skipper", { project })).catch(showError);
}

/** The scope of a Skipper the app is starting: its session shows as soon as skiffd has it. */
let awaited: string | null = null;

/**
 * Runs `start`, and shows the new Skipper of `scope` the moment skiffd adds
 * it. The start returns only after the agent is ready and has its first
 * message, which takes seconds. A pane that ends meanwhile then gives its
 * keys to the new Skipper, not to the next row.
 */
async function starting(scope: string, start: () => Promise<string>) {
  awaited = scope;
  try {
    const id = await start();
    // Shown already when skiffd added it: the person may have moved on since.
    if (id && awaited === scope) show(id);
  } finally {
    if (awaited === scope) awaited = null;
  }
}

/** skiffd added session `s`: the Skipper the app is starting shows at once. */
export function skipperCreated(s: SessionInfo) {
  if (!awaited || s.skipper !== awaited) return;
  awaited = null;
  show(s.id);
}

/** Project Skipper sessions that a takeover hands to a fleet Skipper, and the fleet. */
const handedTo = new Map<string, string>();

/**
 * Session `id` ends. When it has the keys, they go to the fleet Skipper that
 * took this retiring project Skipper over, else null. A takeover from the app
 * names its fleet. One from a terminal is found by the project's fleet.
 */
export function heirOf(id: string, focused: boolean): string | null {
  const s = sessions.get(id);
  const named = handedTo.get(id);
  handedTo.delete(id);
  if (!focused) return null;
  const fleet = named ?? (s?.retiring ? place(s)?.project.fleet : null);
  const heir = fleet ? fleetSkipperOf(fleet)?.id : null;
  return heir && heir !== id ? heir : null;
}

function handOver(fleet: string, taken: Taken[]) {
  for (const t of taken) handedTo.set(t.session, fleet);
}

/** One project Skipper a takeover stops, as `fleet_takeover_preview` gives it. */
export interface Taken {
  project: string;
  session: string;
  tasks: { uid: string; title: string }[];
  handoffs: string[];
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The confirm's body: one line per project Skipper, with its open tasks by title and its hand-offs. */
function takenLines(taken: Taken[]): HTMLElement[] {
  const body = h("div", "confirm-body");
  body.append(...taken.map((t) => {
    let line = `${t.project} Skipper: ${count(t.tasks.length, "open task", "open tasks")}`;
    if (t.tasks.length) line += ` (${t.tasks.map((x) => x.title).join(", ")})`;
    if (t.handoffs.length) line += `, ${count(t.handoffs.length, "hand-off", "hand-offs")}`;
    return h("div", "", `${line}.`);
  }));
  return [body];
}

/** One confirm for a takeover. Nothing to take over asks nothing. */
function confirmTakeover(title: string, action: string, taken: Taken[]): Promise<boolean> {
  if (!taken.length) return Promise.resolve(true);
  return new Promise((resolve) => {
    let ok = false;
    actionDialog({ title, content: takenLines(taken), action, tone: "go", submit: () => void (ok = true), onClose: () => resolve(ok) });
  });
}

/** Fleet menu › Start fleet Skipper, or Open fleet Skipper while it runs. Project Skippers in the fleet are taken over after one confirm. */
export async function startFleetSkipper(fleet: string) {
  const s = fleetSkipperOf(fleet);
  if (s) return show(s.id);
  try {
    const taken = await invoke<Taken[]>("fleet_takeover_preview", { fleet, join: null });
    if (!(await confirmTakeover(`Start the ${fleet} Skipper?`, "Start fleet Skipper", taken))) return;
    handOver(fleet, taken);
    await starting(fleetScope(fleet), () => invoke<string>("start_fleet_skipper", { fleet }));
  } catch (e) {
    showError(e);
  }
}

/**
 * Add to fleet…, Move to fleet… or a drop: when `fleet` has a running fleet
 * Skipper and `project` a project Skipper, one confirm, then the takeover,
 * which also sets the fleet. False: not a takeover, the caller sets the
 * fleet itself. Null: the person cancelled.
 */
export async function joinFleet(project: string, fleet: string): Promise<boolean | null> {
  if (!fleet || !fleetSkipperOf(fleet) || !projectSkipperOf(project)) return false;
  const taken = await invoke<Taken[]>("fleet_takeover_preview", { fleet, join: project });
  if (!taken.length) return false;
  if (!(await confirmTakeover(`Add ${project} to ${fleet}?`, "Add to fleet", taken))) return null;
  handOver(fleet, taken);
  await invoke("fleet_takeover", { fleet, join: project });
  const s = fleetSkipperOf(fleet);
  if (s) show(s.id);
  return true;
}

/** What `open_skiff_agent` answers. */
interface Opened {
  session: SessionInfo;
  created: boolean;
  typed: boolean;
  /** Why its watcher did not start. */
  watch_error: string | null;
}

/**
 * The row and the key: show the Skiff agent's pane, or start it. skiffd finds
 * or starts it in one guarded step, so two quick opens start one. A pane that
 * is a bare shell gets its agent again.
 */
export function openSkiffAgent() {
  if (!skiffAgentFolder()) return;
  const s = skiffAgentSession();
  if (s) show(s.id);
  if (s && agentKind(s) !== "shell") return;
  if (!s && !skiffAgentChoice()) return showError("No agent is on. Turn one on in Settings › Agents.");
  invoke<Opened | null>("open_skiff_agent", { restore: null })
    .then((o) => {
      if (!o) return;
      noteAsk(o.session.id, o.watch_error);
      upsert(o.session);
      if (o.session.id !== s?.id) show(o.session.id);
    })
    .catch(showError);
}

function show(id: string) {
  revealSession(id);
  requestAnimationFrame(() => keepRow(id));
}

/** When the app last asked for a watcher, so a render does not ask each time. */
let watchAsked = { id: "", at: 0 };

/** Why the last start of the watcher failed. `null`: it did not fail. */
let startError: string | null = null;

/** The daemon's protocol, and when the app last connected to it. */
let daemon = { protocol: null as number | null, at: 0 };

/** A watcher has this long to register after a start or a connect. */
const WATCH_GRACE = 5_000;

/** Re-renders once the grace ends, so a watcher that did not register shows. */
let graceTimer = 0;
function graceRender() {
  clearTimeout(graceTimer);
  graceTimer = window.setTimeout(render, WATCH_GRACE + 100);
}

/** setDaemon calls it on each status: the protocol, and a new grace after a connect. */
export function noteDaemon(status: DaemonStatus) {
  daemon.protocol = status.protocol ?? null;
  if (status.connected) daemon.at = Date.now();
  graceRender();
}

function noteAsk(id: string, error: string | null) {
  watchAsked = { id, at: Date.now() };
  startError = error;
  graceRender();
}

/**
 * Starts `skiff watch` for the Skiff agent's session: on start, and again after
 * an app restart or a daemon restart. The backend starts none while one runs,
 * so this asks again now and then. `now` skips the 30 s wait and shows a failure.
 */
function watch(id: string, now = false) {
  if (!now && watchAsked.id === id && Date.now() - watchAsked.at < 30_000) return;
  noteAsk(id, startError);
  invoke<boolean>("start_skiff_watch", { session: id })
    .then(() => {
      if (startError === null) return;
      startError = null;
      render();
    })
    .catch((e) => {
      startError = String(e);
      render();
      if (now) showError(e);
      else console.error(e);
    });
}

/** The row menu's Start watcher. */
export function startSkiffWatch() {
  const s = skiffAgentSession();
  if (s) watch(s.id, true);
}

/** The row menu's Open watch log, as Settings › Open with says. */
export function openWatchLog() {
  invoke<string>("skiff_watch_log").then(openPath).catch(showError);
}

/**
 * Whether `skiff watch` runs for the Skiff agent's session:
 * - runs: a watcher registered and reports no failure.
 * - failed: a registered watcher reports a failure.
 * - missing: none registered, past the grace.
 * - starting: none registered, within the grace after a connect or a start.
 * - unknown: skiffd is older than protocol 27, or the session is not live.
 */
export type WatchState = "runs" | "failed" | "missing" | "starting" | "unknown";

export function watchMark(s: SessionInfo | null, protocol: number | null, lastAsk: number, now: number): WatchState {
  if (!s || s.state === "done" || protocol === null || protocol < 27) return "unknown";
  if (s.watch) return s.watch.failure ? "failed" : "runs";
  return now - lastAsk < WATCH_GRACE ? "starting" : "missing";
}

/** Local `HH:MM`. */
const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });

/** The tooltip text for a state. Empty for starting and unknown. */
export function watchText(state: WatchState, s: SessionInfo, name: string, error: string | null): string {
  const f = s.watch?.failure;
  switch (state) {
    case "runs":
      return `skiff watch runs for ${name} since ${clock(s.watch!.since)}.`;
    case "missing":
      return `No watcher runs for ${name}. ${name} does not hear when its panes finish. Skiff tries to start it every 30 seconds.` +
        (error ? ` Last start failed: ${error}.` : "");
    case "failed":
      return f?.kind === "ledger"
        ? `The watcher cannot read the ledger: ${f.message}.`
        : `At ${clock(f?.at ?? 0)} a watcher line did not start a turn. It can be in ${name}'s input box.`;
    default:
      return "";
  }
}

/** The Skiff agent's watcher state now, and its tooltip text. */
export function skiffWatch(s: SessionInfo): { state: WatchState; text: string } {
  const now = Date.now();
  // A session not asked for yet is asked on this render.
  const asked = watchAsked.id === s.id ? watchAsked.at : now;
  const state = watchMark(s, daemon.protocol, Math.max(daemon.at, asked), now);
  return { state, text: watchText(state, s, skiffAgentName(), startError) };
}

/** A project Skipper's watcher state, from its own registration: the
 *  watcher registers on each Skipper it follows. The app starts no watcher
 *  for a project Skipper on its own (`skiff skipper --project` does, and the
 *  Skiff agent's watcher follows every Skipper), so none registered is
 *  unknown here, not missing. */
export function projectWatch(s: SessionInfo): { state: WatchState; text: string } {
  const state: WatchState = s.state === "done" || !s.watch ? "unknown" : s.watch.failure ? "failed" : "runs";
  return { state, text: watchText(state, s, projectSkipperName(s), null) };
}

/** A project or fleet Skipper's session name: its project or fleet, then the Skiff agent's name ("gesher Skipper", "Harbor Skipper"). */
export function projectSkipperName(s: SessionInfo): string {
  return `${scopeName(skipperProject(s) ?? s.skipper ?? "")} ${skiffAgentName()}`;
}

/** Restored panes Skiff already asked skiffd to continue. Once each. */
const continued = new Set<string>();

/**
 * Runs on each render. A restored Skiff agent pane continues the last
 * conversation in its folder when Settings says so. Only this pane: every
 * other pane restores as a shell and the user picks. A custom agent has no
 * continue line. A rename from Settings renames the running session too.
 */
export function syncSkiffAgent() {
  // Not read yet: the default name is not a rename.
  if (!S.skiffAgent) return;
  const s = skiffAgentSession();
  const name = skiffAgentName();
  // Only a change in Settings renames it, and each project Skipper with it
  // ("gesher Skipper"). A rename of the pane itself stays.
  if (shownName !== null && shownName !== name) {
    if (s && s.name !== name) rename(s.id, name);
    for (const p of projectSkippers()) {
      if (p.name !== projectSkipperName(p)) rename(p.id, projectSkipperName(p));
    }
  }
  shownName = name;
  if (!s) return;
  watch(s.id);
  if (!S.skiffAgent.continue_on_restore || !s.resume || continued.has(s.id) || agentKind(s) !== "shell") return;
  continued.add(s.id);
  // skiffd types the continue line, the same as for `skiff skipper`. Refused
  // when the person typed in the pane first: their line stays theirs.
  invoke("open_skiff_agent", { restore: s.input_gen ?? 0 }).catch(showError);
}

/** The name the last render saw, so a change from Settings shows. */
let shownName: string | null = null;

function rename(id: string, name: string) {
  invoke<SessionInfo>("rename_session", { session: id, name })
    .then((info) => {
      const s = sessions.get(id);
      if (s) s.name = info.name;
      render();
    })
    .catch(console.error);
}

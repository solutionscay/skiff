/**
 * What the Ledger shows: where each task stands, the items of a view, their
 * sections and counts. It reads the rows, the sessions and the view, and draws
 * nothing. The buckets come from the Host. `ledger.ts` keeps the public names.
 */
import { coordinatorOf, skipperWord } from "../app/skiffAgent";
import { S, sessions } from "../app/state";
import { shownIds, skipperSplit } from "../app/stateQueries";
import type { SessionInfo } from "../platform/types";
import { isSlot } from "./layoutSlots";
import { scopeFleet, scopeName, taskTitle } from "./model";
import { ledgerScope } from "./profile";
import { language, t, type MessageKey } from "../i18n";
import { byUid, children, generation, indexRows, paneIds, rows, titleOf, type Entry } from "./ledgerStore";

/** Where a task stands, from its row and its pane. */
export type State = "pending" | "sent" | "approval" | "blocked" | "interrupted" | "gone" | "exited" | "done";

/** The states that need the person, and the word that takes the pane name's place. */
export const NEED: Partial<Record<State, MessageKey>> = { approval: "ledger.needApproval", blocked: "ledger.needBlocked", interrupted: "ledger.needRestarted", gone: "ledger.needGone", exited: "ledger.needExited" };
/** The order of the needs, most urgent first: a plan shows the first its tasks have. */
const URGENT: State[] = ["approval", "blocked", "interrupted", "gone", "exited"];

/** The order in Open: needs the person, then done, then the rest. */
const rank = (st: State) => (NEED[st] ? 0 : st === "done" ? 1 : 2);

/** Finished keeps this many items. A plan counts as one. The Host reads no more. */
const KEEP = 100;

/** A task as the panel shows it. */
export interface Look {
  r: Entry;
  st: State;
  /** Its pane, when a click can open it. */
  pane: SessionInfo | null;
  /** The pane name, or what stands for it. */
  where: string;
  /** A hand-off's tasks: the project Skipper's rows under it. */
  kids?: Look[];
}

/** An item of the list: a task, or a plan with its tasks. */
export interface Unit {
  r: Entry;
  st: State;
  look?: Look;
  kids?: Look[];
  /** When the work was closed: its head's stamp. `null`: its bucket is not finished. */
  closed: string | null;
  /** An agent is on one of its tasks now. */
  working: boolean;
}

let projectionInputs = "";
let projectionVersion = 0;
const paneInputs = new Map<string, { state: SessionInfo["state"] | undefined; started: SessionInfo["started_at"] | undefined; title: string | null }>();
const projections = new Map<string | null, Unit[]>();
let counts: { need: number; owners: Map<string, { need: number; working: number }> } | null = null;

/** Session events can change a task without a ledger write. Read only the
 *  session fields and workspace membership that affect the projection. */
export function projectionKey(): number {
  indexRows();
  const scope = ledgerScope();
  const key = JSON.stringify([
    generation, scope ? [...scope.names].sort() : null,
    S.projects.map((p) => [p.name, p.fleet]), skipperWord(), language(),
  ]);
  let changed = key !== projectionInputs;
  if (changed) paneInputs.clear();
  // Session objects are updated in place. Compare their projected values;
  // unchanged callers neither serialize nor allocate another session snapshot.
  for (const id of paneIds) {
    const s = sessions.get(id);
    const title = s ? taskTitle(s) : null;
    const old = paneInputs.get(id);
    if (old && old.state === s?.state && old.started === s?.started_at && old.title === title) continue;
    paneInputs.set(id, { state: s?.state, started: s?.started_at, title });
    changed = true;
  }
  if (changed) {
    projectionVersion++;
    projectionInputs = key;
    projections.clear();
    counts = null;
  }
  return projectionVersion;
}

/** Where a task stands, for a surface outside the panel. */
export const taskState = (r: Entry): State => look(r).st;
/** The word for a state that needs the person, else `null`. */
export const needWord = (st: State): string | null => (NEED[st] ? t(NEED[st]!) : null);

function look(r: Entry): Look {
  const s = r.pane_id ? sessions.get(r.pane_id) ?? null : null;
  // A hand-off's pane is the project Skipper.
  const name = r.kind === "handoff" ? `${scopeName(r.project)} ${skipperWord()}` : s ? taskTitle(s) : r.pane_id?.slice(0, 8) ?? "";
  const base = { r, pane: s, where: name };
  if (r.elsewhere) return { ...base, st: r.state === "done" || r.state === "pending" ? r.state : "sent", pane: null, where: t("ledger.elsewhere") };
  // Done because the pane went: it has no result to show for it.
  if (r.state === "done") return { ...base, st: r.gone_at ? "gone" : "done" };
  // Not sent yet: no agent was on it, so a pane that went takes nothing from it.
  if (r.state === "pending") return { ...base, st: "pending" };
  if (!s) return { ...base, st: "gone" };
  const restarted = r.pane_gen !== null && !!s.started_at && s.started_at !== r.pane_gen;
  if (restarted) return { ...base, st: "interrupted" };
  if (r.state === "blocked") return { ...base, st: r.state };
  if (s.state === "waiting") return { ...base, st: "approval" };
  if (s.state === "done") return { ...base, st: "exited" };
  return { ...base, st: "sent" };
}

/** The stamp a unit files by: its work head's `closed_at` once the row's bucket is finished, else `null`. */
function closedOf(r: Entry): string | null {
  if (r.bucket !== "finished") return null;
  let at = r;
  // Two steps at most: plan, hand-off, task.
  for (let i = 0; i < 3 && at.parent; i++) at = byUid.get(at.parent) ?? at;
  return at.closed_at ?? r.updated_at;
}

/** A look and its hand-off's tasks. */
const flat = (l: Look): Look[] => [l, ...(l.kids ?? [])];

/** A hand-off with its tasks under it. Each task shows by the view. */
function handoff(r: Entry, view: (r: Entry) => boolean): Look {
  const l = look(r);
  return { ...l, kids: (children.get(r.uid) ?? []).filter(view).map(look) };
}

/** A plan's state from its tasks: the most urgent need shows. */
function planState(kids: Look[]): State {
  for (const st of URGENT) if (kids.some((k) => k.st === st)) return st;
  if (kids.every((k) => k.st === "done")) return "done";
  if (kids.every((k) => k.st === "pending")) return "pending";
  return "sent";
}

/** The projects of the fleet a scope names ("fleet:Harbor"), as projects.toml has them now. */
const fleetProjects = (fleet: string) => S.projects.filter((p) => p.fleet === fleet).map((p) => p.name);

/**
 * The row shows in the current Ledger view: no workspace or All work, its
 * project in the workspace, or no project. With `project`, only that
 * project's rows. A fleet ("fleet:Harbor") shows its Skipper's rows, the
 * hand-offs to it, and every row of its projects, whatever their owner.
 */
function inView(r: Entry, scope = ledgerScope(), project: string | null = null): boolean {
  const fleet = scopeFleet(project);
  if (fleet !== null) return r.project === project || r.owner === project || fleetProjects(fleet).includes(r.project);
  if (project !== null) return r.project === project;
  const at = scopeFleet(r.project);
  if (at !== null) return !scope || fleetProjects(at).some((p) => scope.names.has(p));
  return !scope || !r.project || scope.names.has(r.project);
}

/** Every item the view shows, in ledger order. `project`: only its rows; undefined takes the panel's filter. */
export function units(project: string | null | undefined = filter()): Unit[] {
  projectionKey();
  const cached = projections.get(project);
  if (cached) return cached;
  const scope = ledgerScope();
  const view = (k: Entry) => inView(k, scope, project);
  const heads = new Set(rows.filter((r) => r.kind === "plan" || r.kind === "handoff").map((r) => r.uid));
  // A hand-off in a plan, and a row under a hand-off, show under their head.
  const placed = (r: Entry) => !!r.parent && heads.has(r.parent);
  const out: Unit[] = [];
  for (const r of rows) {
    if (r.kind === "plan") {
      const all = (children.get(r.uid) ?? []).filter((k) => k.kind !== "plan");
      // Each task shows by its own project. The plan heads the ones that show.
      // A hand-off shows when it or one of its tasks does.
      const kids = all
        .map((k) => (k.kind === "handoff" ? handoff(k, view) : view(k) ? look(k) : null))
        .filter((k): k is Look => !!k && (view(k.r) || !!k.kids?.length));
      if (!kids.length) {
        if (!inView(r, scope)) continue;
        // A heading with no visible task keeps its own state.
        const st: State = r.state === "done" || r.state === "pending" || r.state === "blocked" ? r.state : "sent";
        out.push({ r, st, kids, closed: closedOf(r), working: false });
        continue;
      }
      const every = kids.flatMap(flat);
      out.push({ r, st: planState(every), kids, closed: closedOf(r), working: every.some((k) => k.st === "sent") });
    } else if (!placed(r)) {
      const l = r.kind === "handoff" ? handoff(r, view) : look(r);
      if (!view(r) && !l.kids?.length) continue;
      out.push({ r, st: l.st, look: l, closed: closedOf(r), working: flat(l).some((k) => k.st === "sent") });
    }
  }
  projections.set(project, out);
  return out;
}

/** The three sections. */
interface Sections {
  working: Unit[];
  open: Unit[];
  finished: Unit[];
}

/** Working and Open in send order, Open by its rank first; Finished newest closed first, the last [`KEEP`]. */
export function sections(us: Unit[]): Sections {
  const live = us.filter((u) => !u.closed);
  return {
    working: live.filter((u) => u.working),
    open: live.filter((u) => !u.working).sort((a, b) => rank(a.st) - rank(b.st) || a.r.id - b.r.id),
    finished: us
      .filter((u) => u.closed)
      .sort((a, b) => b.closed!.localeCompare(a.closed!) || b.r.id - a.r.id)
      .slice(0, KEEP),
  };
}

/** Every task of work not closed, hand-offs' tasks included. */
const openLooks = (us: Unit[]) =>
  us
    .filter((u) => !u.closed)
    .flatMap((u) => u.kids ?? (u.look ? [u.look] : []))
    .flatMap(flat);

/** The tasks of work not closed that need the person. */
export function needing(us: Unit[]): number {
  return openLooks(us).filter((l) => NEED[l.st]).length;
}

/** The count on Skipper's row: every row that needs the person, whatever its owner. */
export function ledgerNeeds(): number {
  return ledgerSummary().need;
}

/** The counts on a project Skipper's row: its tasks that need the person, and its tasks an agent is on. */
export function ledgerCounts(project: string): { need: number; working: number } {
  return ledgerSummary().owners.get(project) ?? { need: 0, working: 0 };
}

/** Share one count pass across the tree signatures, rows, and the panel. */
function ledgerSummary() {
  projectionKey();
  if (counts) return counts;
  const owners = new Map<string, { need: number; working: number }>();
  let need = 0;
  for (const l of openLooks(units(null))) {
    const count = owners.get(l.r.owner) ?? { need: 0, working: 0 };
    if (NEED[l.st]) { count.need++; need++; }
    if (l.st === "sent") count.working++;
    owners.set(l.r.owner, count);
  }
  return counts = { need, owners };
}

/** The coordinator whose pane is the one pane on screen, or the left pane of its Skipper split: "grand" or a project. */
export function shownOwner(): string | null {
  const sp = skipperSplit();
  if (sp) return coordinatorOf(sp.skipper);
  const ids = shownIds().filter((id) => !isSlot(id));
  return ids.length === 1 ? coordinatorOf(ids[0]) : null;
}

/** The project the filter picked for each coordinator's panel. Absent: its default. */
export const filters = new Map<string, string | null>();

/** The panel's project filter: null for all projects. The Skipper's panel opens on all, a project Skipper's on its project. */
export function filter(): string | null {
  const o = shownOwner();
  if (!o) return null;
  return filters.has(o) ? filters.get(o)! : o === "grand" ? null : o;
}

/** The task's pane is live, so a click can open it. */
export const opens = (l: Look) => !!l.pane && l.st !== "gone";

/**
 * The Skipper split's children: the live panes of the open rows the panel
 * shows, hand-offs' tasks included, in Ledger order (Working, then Open).
 * Each pane once. The Skipper's own pane is no child.
 */
export function childPanes(skipper: string): { id: string; title: string }[] {
  const x = sections(units());
  const out: { id: string; title: string }[] = [];
  for (const u of [...x.working, ...x.open]) {
    for (const l of (u.kids ?? (u.look ? [u.look] : [])).flatMap(flat)) {
      if (!opens(l) || l.pane!.id === skipper || out.some((c) => c.id === l.pane!.id)) continue;
      out.push({ id: l.pane!.id, title: titleOf(l.r) });
    }
  }
  return out;
}

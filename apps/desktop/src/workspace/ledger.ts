import { animate } from "./ledgerMotion";
import { still } from "../ui/motion";
/**
 * The Ledger: Skipper's tasks, in a panel at the right of its pane. It shows
 * only while Skipper's pane is the one pane on screen. Focus mode keeps it;
 * maximize hides it with the other chrome.
 *
 * Three sections, each shown when it has work. Working: an agent is on the
 * work now (a sent task whose pane works or ended its turn), in send order.
 * Open: the work is not closed and no agent is on it: the rows that need the
 * person first, then done tasks, then the rest. A row that needs the person
 * turns amber. Finished: the work Skipper closed on the person's word, newest
 * first, the last 100. A plan is a head row with its tasks always open under
 * it, and it counts as one item. Rows slide to their new place and fade in or
 * out; a close slides the work down into Finished.
 *
 * Ctrl+Shift+Right from Skipper's pane gives the keys to the panel, and opens
 * a folded one. Ctrl+Shift+Up/Down, or the plain arrows, step the lit row.
 * Enter does the row's click. Ctrl+Shift+Left or Esc gives the keys back to
 * Skipper's pane. The lit row stays across redraws.
 *
 * A click on a row does the main action for its state: open the pane or the
 * result. Small icons at the right edge do the rest, only when they apply:
 * the pane or the result. The panel never writes: only Skipper changes rows.
 * No row has a menu. The rows come read-only from the selected Host
 * (`ledger@1`), again on each change to its ledger. A result shows in the
 * page's own viewer, read by the Host from its row: no path opens here.
 *
 * An open workspace (#154) filters the rows by their project: its projects'
 * work, and work with no project. A plan shows by its own project or a visible
 * task, with only visible tasks under it. All work, in the head, shows every row.
 * The filter is a view: no row changes.
 *
 * Project Skippers (#project-skippers): the panel also shows beside a
 * project Skipper's pane. Its head filter shows all projects or one: the
 * Skipper's panel opens on all, a project Skipper's on its project. A
 * hand-off nests under its plan with its project's letters in place of the
 * dot, and its tasks nest one more step. A project Skipper's own rows show
 * the letters too.
 *
 * The person can fold the panel to a rail: a chevron, and the amber count of
 * the tasks that need them. The fold stays in localStorage on this machine.
 * A fold or an unfold slides the width; a saved fold at start does not.
 */
import { listen } from "@tauri-apps/api/event";
import { keyLabel } from "../app/keys";
import { render } from "../app/renderRequest";
import { coordinatorOf, projectSkipperOf, skiffAgentSession } from "../app/skiffAgent";
import { accent } from "../app/stateQueries";
import { ctxMenu } from "../ui/contextMenu";
import { S, sessions } from "../app/state";
import { shownIds } from "../app/stateQueries";
import type { SessionInfo } from "../platform/types";
import { openLedgerResult, previewToken } from "../terminal/peek";
import { hostInvoke } from "../app/hostCaps";
import { dataEpoch } from "../app/dataPlane";
import { fitWithLayout } from "../terminal/terminal";
import { panes } from "../terminal/terminalState";
import type { Action } from "../app/keys";
import { h, setAttr, setHidden, setText } from "../ui/dom";
import { isSlot } from "./layoutSlots";
import { scopeFleet, scopeName, taskTitle } from "./model";
import { skipperWord } from "../app/skiffAgent";
import { keepRow } from "./selection";
import { revealSession } from "./view";
import { ledgerAllWork, ledgerScope, setLedgerAllWork, workspaceName, workspaceOpen } from "./profile";
import { language, t, tn, upper, type MessageKey } from "../i18n";

/** A ledger row as `ledger_rows` returns it. */
export interface Entry {
  id: number;
  uid: string;
  kind: "task" | "plan" | "handoff";
  /** The coordinator whose row it is: "grand", or a project Skipper's project. */
  owner: string;
  parent: string | null;
  pane_id: string | null;
  pane_gen: number | null;
  /** The project the work is for. Empty: no project, shown in every workspace. */
  project: string;
  task: string;
  sent_at: string;
  /** The short title Skipper gave it. */
  title: string | null;
  state: "sent" | "pending" | "blocked" | "done";
  updated_at: string;
  /** When the person, or Skiff once every task was done, and reported or flushed, closed the work. Only a work head has it. */
  closed_at: string | null;
  /** The work's bucket, as `skiff ledger ls` gives it. Every row of one work has the same one.
   *  Missing from a Host that predates it: its rows file as not finished. */
  bucket?: "working" | "open" | "finished";
  /** When skiff set the task done because its pane was gone. */
  gone_at: string | null;
  /** The pane is on another daemon. */
  elsewhere?: boolean;
  /** The Host can show the result. */
  result_exists: boolean;
  /** When Settings › Disk use flushed the row's work folder. */
  flushed_at?: string | null;
}

/** Where a task stands, from its row and its pane. */
export type State = "pending" | "sent" | "approval" | "blocked" | "interrupted" | "gone" | "exited" | "done";

/** The states that need the person, and the word that takes the pane name's place. */
const NEED: Partial<Record<State, MessageKey>> = { approval: "ledger.needApproval", blocked: "ledger.needBlocked", interrupted: "ledger.needRestarted", gone: "ledger.needGone", exited: "ledger.needExited" };
/** The order of the needs, most urgent first: a plan shows the first its tasks have. */
const URGENT: State[] = ["approval", "blocked", "interrupted", "gone", "exited"];

/** The order in Open: needs the person, then done, then the rest. */
const rank = (st: State) => (NEED[st] ? 0 : st === "done" ? 1 : 2);

/** Finished keeps this many items. A plan counts as one. The Host reads no more. */
const KEEP = 100;

/** A task as the panel shows it. */
interface Look {
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
interface Unit {
  r: Entry;
  st: State;
  look?: Look;
  kids?: Look[];
  /** When the work was closed: its head's stamp. `null`: its bucket is not finished. */
  closed: string | null;
  /** An agent is on one of its tasks now. */
  working: boolean;
}

let rows: Entry[] = [];
let generation = 0;
let rowData = "";
let indexedGeneration = -1;
const byUid = new Map<string, Entry>();
const children = new Map<string, Entry[]>();
const paneIds = new Set<string>();
const workTitles = new Map<string, string>();

/** Index the Host's data once per changed reply, including the fallback hand-offs. */
function indexRows() {
  if (indexedGeneration === generation) return;
  indexedGeneration = generation;
  byUid.clear();
  children.clear();
  paneIds.clear();
  workTitles.clear();
  const fallback = new Map<string, string>();
  for (const r of rows) {
    byUid.set(r.uid, r);
    if (r.parent) {
      const kids = children.get(r.parent) ?? [];
      kids.push(r);
      children.set(r.parent, kids);
    }
    if (r.pane_id) paneIds.add(r.pane_id);
    if (r.kind === "handoff" && r.state !== "done") fallback.set(r.project, titleOf(r));
    if (r.bucket === "finished" || r.state === "done" || r.parent) continue;
    workTitles.set(r.owner, titleOf(r));
    if (r.kind === "handoff") workTitles.set(r.project, titleOf(r));
  }
  for (const [scope, title] of fallback) if (!workTitles.has(scope)) workTitles.set(scope, title);
}

let projectionInputs = "";
let projectionVersion = 0;
const paneInputs = new Map<string, { state: SessionInfo["state"] | undefined; started: SessionInfo["started_at"] | undefined; title: string | null }>();
const projections = new Map<string | null, Unit[]>();
let counts: { need: number; owners: Map<string, { need: number; working: number }> } | null = null;

/** Session events can change a task without a ledger write. Read only the
 *  session fields and workspace membership that affect the projection. */
function projectionKey(): number {
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
let error: string | null = null;
let loaded = false;
/** The last read asked for. Only its reply shows. */
let asked = 0;

/** Reads the rows again. A reply shows only when no read was asked since and
 *  the data connection it came on is still the one on screen. */
export async function loadLedger() {
  const mine = ++asked;
  const at = dataEpoch();
  let next: Entry[] = [];
  let err: string | null = null;
  try {
    next = await hostInvoke<Entry[]>("ledger", "ledger_rows");
  } catch (e) {
    err = String(e);
  }
  if (mine !== asked || at !== dataEpoch()) return;
  const data = JSON.stringify([next, err]);
  // Keep both the generation and row objects for repeated, unchanged replies.
  if (loaded && data === rowData) return;
  rowData = data;
  rows = next;
  error = err;
  loaded = true;
  generation++;
  render("ledger");
}

/** Reads the ledger now and on each change. main.ts calls it once. */
export function startLedger() {
  const panel = document.getElementById("ledger");
  panel?.addEventListener("keydown", rowKey);
  void listen("skiff:ledger-changed", () => void loadLedger());
  void loadLedger();
}

/**
 * The short title: the first sentence of the task text. It ends at the first
 * ". ", "; " or line break outside parentheses, and is cut at a space before
 * 80 characters. The row's ellipsis cuts the rest.
 */
function shortTitle(text: string): string {
  const t = text.trim();
  let depth = 0;
  let end = t.length;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (c === "(") depth++;
    else if (c === ")") depth = Math.max(0, depth - 1);
    else if (c === "\n" || (depth === 0 && (c === "." || c === ";") && (i + 1 === t.length || t[i + 1] === " "))) {
      end = i;
      break;
    }
  }
  const s = t.slice(0, end).trim();
  if (s.length <= 80) return s;
  const cut = s.lastIndexOf(" ", 80);
  return s.slice(0, cut > 0 ? cut : 80);
}

/** The title Skipper gave the row, else the one the task text gives. */
const titleOf = (r: Entry) => r.title?.trim() || shortTitle(r.task);

/** The rows as the panel holds them, for a surface that draws them its own way: mission control's strings. */
export const ledgerRows = (): readonly Entry[] => rows;
/** Where a task stands, for a surface outside the panel. */
export const taskState = (r: Entry): State => look(r).st;
/** The word for a state that needs the person, else `null`. */
export const needWord = (st: State): string | null => (NEED[st] ? t(NEED[st]!) : null);
export const ledgerTitle = titleOf;

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
function units(project: string | null | undefined = filter()): Unit[] {
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
function sections(us: Unit[]): Sections {
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
function needing(us: Unit[]): number {
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

/** The title of a project Skipper's newest open work: a hand-off it was given, or its own. */
export function ledgerWorkTitle(project: string): string | null {
  indexRows();
  return workTitles.get(project) ?? null;
}

/** The coordinator whose pane is the one pane on screen: "grand" or a project. */
function shownOwner(): string | null {
  const ids = shownIds().filter((id) => !isSlot(id));
  return ids.length === 1 ? coordinatorOf(ids[0]) : null;
}

/** The coordinator's session the panel sits beside. */
function owner(): SessionInfo | null {
  const o = shownOwner();
  return o === "grand" ? skiffAgentSession() : o ? projectSkipperOf(o) : null;
}

/** The project the filter picked for each coordinator's panel. Absent: its default. */
const filters = new Map<string, string | null>();

/** The panel's project filter: null for all projects. The Skipper's panel opens on all, a project Skipper's on its project. */
function filter(): string | null {
  const o = shownOwner();
  if (!o) return null;
  return filters.has(o) ? filters.get(o)! : o === "grand" ? null : o;
}

/** The panel shows while a Skipper's pane is the one pane on screen. A preview a row opened keeps it; any other preview hides it. */
function wanted(): boolean {
  const token = previewToken();
  return !!owner() && !(token && token !== ours);
}

/** The panel is on screen: focus mode keeps it, maximize hides it. The fold acts only then. */
export function ledgerShown(): boolean {
  return wanted() && !S.maximized;
}

const FOLD_KEY = "skiff.ledgerFolded";
let folded = (() => {
  try {
    return localStorage.getItem(FOLD_KEY) === "1";
  } catch {
    return false;
  }
})();

export const ledgerFolded = () => folded;

/** Folds the panel to its rail, or opens it. Nothing while the panel is not on screen. */
export function toggleLedger() {
  if (!ledgerShown()) return;
  folded = !folded;
  try {
    localStorage.setItem(FOLD_KEY, folded ? "1" : "0");
  } catch {
    /* the fold lasts for this run */
  }
  // At once: the terminal beside the panel takes its new size in the same frame.
  fitWithLayout();
  render();
}

/** The next draw of the open panel flashes the first row that needs the person. */
let flashNext = false;

function flashNeed(panel: HTMLElement) {
  const r = panel.querySelector<HTMLElement>(".lg-list .lg-row.need");
  if (!r) return;
  r.scrollIntoView({ block: "nearest" });
  r.classList.remove("flash");
  void r.offsetWidth;
  r.classList.add("flash");
}

function openPane(s: SessionInfo) {
  revealSession(s.id);
  requestAnimationFrame(() => keepRow(s.id));
}

/** The task's pane is live, so a click can open it. */
const opens = (l: Look) => !!l.pane && l.st !== "gone";

/** The row icons, at the stroke weight of the tree icons. */
const ICON = {
  fold: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M3.5 1.5 7 5 3.5 8.5"/></svg>',
  down: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M1.5 3.5 5 7l3.5-3.5"/></svg>',
  unfold: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M6.5 1.5 3 5l3.5 3.5"/></svg>',
  pane: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><rect x=".65" y="1.15" width="8.7" height="7.7"/><path d="M2.6 3.8 4.2 5 2.6 6.2M5.2 6.4h2"/></svg>',
  result: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M2 .65h4L8.35 3v6.35H2z"/><path d="M5.8.65V3.2h2.55"/></svg>',
};

/** What a row does: its click and the icons at its right edge. */
interface Acts {
  click: (() => void) | null;
  icons: [keyof typeof ICON, string, () => void][];
}

/** The preview a Ledger row opened. The panel stays beside it. */
let ours = 0;

const result = (r: Entry) =>
  r.result_exists
    ? () => {
        openLedgerResult(r.uid, titleOf(r));
        ours = previewToken();
      }
    : null;

/**
 * A task's actions. Running, approval, blocked, pending, exited: the click
 * opens the pane. Restarted: the pane, and a result icon when the file
 * exists. Gone: the result, else its owning Skipper. Done: the result, with a pane
 * icon. Finished: the result only.
 */
function taskActs(l: Look, fin: boolean): Acts {
  const r = l.r;
  const res = result(r);
  const pane = opens(l) ? () => openPane(l.pane!) : null;
  if (fin) return { click: res, icons: [] };
  if (l.st === "done") {
    const icons: Acts["icons"] = [];
    if (pane) icons.push(["pane", t("ledger.openPane"), pane]);
    return { click: res, icons };
  }
  if (l.st === "gone") {
    const skipper = r.owner === "grand" ? skiffAgentSession() : projectSkipperOf(r.owner);
    return { click: res ?? (skipper ? () => openPane(skipper) : null), icons: [] };
  }
  const icons: Acts["icons"] = l.st === "interrupted" && res ? [["result", t("ledger.openResult"), res]] : [];
  return { click: pane, icons };
}

const keptRows = new Map<string, { sig: string; row: HTMLElement }>();
let rowContext = "";

/** One 20px line, with its actions. */
function line(r: Entry, st: State, label: string, a: Acts, o: { plan?: boolean; kid?: boolean; kid2?: boolean; fin?: boolean; count?: string }): HTMLElement {
  const sig = JSON.stringify([r, byUid.get(r.parent ?? "")?.kind, st, label, o, rowContext, !!a.click, a.icons.map(([name, tip]) => [name, tip])]);
  const kept = keptRows.get(r.uid);
  if (kept?.sig === sig) return kept.row;
  const need = !o.fin && !!NEED[st];
  const row = h("div", `lg-row s-${st}` + (need ? " need" : "") + (o.plan ? " plan" : "") + (o.kid ? " kid" : "") + (o.kid2 ? " kid2" : "") + (o.fin ? " fin" : "") + (a.click ? "" : " dead"));
  row.dataset.key = r.uid;
  row.tabIndex = -1;
  // No tooltip: the title names the row for a screen reader.
  row.setAttribute("aria-label", titleOf(r));
  const acts = h("span", "lg-acts");
  for (const [name, tip, run] of a.icons) {
    const b = h("button", "lg-ico");
    b.type = "button";
    b.innerHTML = ICON[name];
    b.title = tip;
    b.setAttribute("aria-label", tip);
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      run();
    });
    acts.append(b);
  }
  // Disk use removed the work folder: the row stays, and says so.
  const flushed = !need && !r.result_exists && !!r.flushed_at;
  row.append(mark(r), h("span", "lg-title", titleOf(r)), h("span", o.plan ? "lg-n" : "lg-pane", flushed ? t("ledger.resultFlushed") : label));
  if (o.count) row.append(h("span", "lg-n", o.count));
  row.append(acts);
  if (a.click) row.addEventListener("click", a.click);
  keptRows.set(r.uid, { sig, row });
  return row;
}

/**
 * The dot, or the project's letters in its color: on a hand-off, and on a
 * project Skipper's top-level row. A task under a project's hand-off keeps
 * its dot. A hand-off to a fleet shows the fleet's letters with no color: a
 * fleet has none. A fleet Skipper's task shows its project's letters, so
 * the list says which repository it is in.
 */
function mark(r: Entry): HTMLElement {
  const fleet = r.kind === "handoff" ? scopeFleet(r.project) : null;
  if (fleet !== null) return h("span", "lg-tag fleet", fleet.slice(0, 3).toUpperCase());
  const ownFleet = scopeFleet(r.owner) !== null;
  const tagged = r.kind === "handoff" || ownFleet || (r.owner !== "grand" && byUid.get(r.parent ?? "")?.kind !== "handoff");
  const p = tagged ? S.projects.find((x) => x.name === (r.kind === "handoff" || ownFleet ? r.project : r.owner)) : null;
  if (!p) return h("span", "lg-dot");
  const tag = h("span", "lg-tag", p.short);
  tag.style.setProperty("--pc", accent(p));
  return tag;
}

/** "done/all" of a hand-off's tasks. */
const progress = (kids: Look[]) => (kids.length ? `${kids.filter((k) => k.st === "done").length}/${kids.length}` : "");

/** A task, or a hand-off with its tasks one step under it. `depth`: 0 at the top, 1 under a plan. */
function taskRows(l: Look, depth: 0 | 1, fin: boolean): HTMLElement[] {
  const row = line(l.r, l.st, (!fin && needWord(l.st)) || l.where, taskActs(l, fin), { kid: depth === 1, fin, count: l.kids ? progress(l.kids) : undefined });
  const kids = (l.kids ?? []).map((k) => line(k.r, k.st, (!fin && needWord(k.st)) || k.where, taskActs(k, fin), { kid: depth === 0, kid2: depth === 1, fin }));
  return [row, ...kids];
}

function unitRows(u: Unit, fin: boolean): HTMLElement[] {
  if (u.look) return taskRows(u.look, 0, fin);
  const kids = u.kids ?? [];
  // A plan row is a head for its tasks: its click opens plan.md.
  const res = result(u.r);
  const row = line(u.r, fin ? "done" : u.st, progress(kids), { click: res, icons: [] }, { plan: true, fin });
  return [row, ...kids.flatMap((k) => taskRows(k, 1, fin))];
}

/** Presentation inputs also affect row marks and fallback actions. */
function presentationKey(): string {
  return JSON.stringify([
    S.projects.map((p) => [p.name, p.short, accent(p)]),
    [...sessions.keys()].map((id) => [id, coordinatorOf(id)]).filter(([, scope]) => scope),
    language(),
  ]);
}

const sectionRows = new Map<string, HTMLElement>();

/** The panel's list was on screen at the last render, so a change can move. */
let drawn = false;

/** A click does not take the keys: the terminal that has them keeps them. */
const keepKeys = (b: HTMLElement) => b.addEventListener("mousedown", (e) => e.preventDefault());

function foldButton(name: "fold" | "unfold"): HTMLButtonElement {
  const b = h("button", "lg-fold") as HTMLButtonElement;
  b.type = "button";
  b.innerHTML = ICON[name];
  keepKeys(b);
  b.addEventListener("click", toggleLedger);
  return b;
}

/** A tooltip with the fold key, which `[keys]` can change. */
function foldTip(b: HTMLElement, what: string) {
  const k = keyLabel("ledger");
  setAttr(b, "title", k ? `${what} (${k})` : what);
  setAttr(b, "aria-label", what);
}

/** The folded panel: a chevron that opens it, and the amber count under it. */
function renderRail(panel: HTMLElement) {
  let rail = panel.querySelector<HTMLElement>(".lg-rail");
  if (!rail) {
    const count = h("button", "lg-rail-need");
    (count as HTMLButtonElement).type = "button";
    keepKeys(count);
    count.addEventListener("click", () => {
      flashNext = true;
      toggleLedger();
    });
    rail = h("div", "lg-rail");
    rail.append(foldButton("unfold"), count);
    panel.replaceChildren(rail);
  }
  foldTip(rail.querySelector(".lg-fold")!, t("ledger.show"));
  const count = rail.querySelector<HTMLElement>(".lg-rail-need")!;
  const n = needing(units());
  count.hidden = !n;
  count.textContent = String(n);
  count.title = tn("ledger.tasksNeedYou", n);
  count.setAttribute("aria-label", count.title);
}

/** What the head's scope button says, or null with no workspace open. */
const scopeLabel = () => (workspaceOpen() ? (ledgerAllWork() ? t("ledger.allWork") : workspaceName()) : null);

/** The head's scope button: the workspace's work, or All work. A click switches. */
function scopeButton(b: HTMLElement) {
  const label = scopeLabel();
  setHidden(b, !label);
  if (!label) return;
  const all = ledgerAllWork();
  setText(b, label);
  setAttr(b, "aria-pressed", String(all));
  const name = workspaceName();
  setAttr(b, "title", all ? t("ledger.allWorkTip", { name }) : t("ledger.workspaceTip", { name }));
  setAttr(b, "aria-label", all ? t("ledger.allWorkAria", { name }) : t("ledger.workspaceAria", { name }));
}

/** What the head's filter says: All projects, the fleet, or the project. */
const filterLabel = () => {
  const f = filter();
  return f === null ? t("ledger.allProjects") : scopeName(f);
};

/** The head's filter: a quiet text button with a chevron. A click lists All projects and each project. */
function filterButton(b: HTMLElement) {
  // Each render calls it: the button is rebuilt only when its label changes.
  if (b.dataset.label === filterLabel()) return;
  b.dataset.label = filterLabel();
  b.replaceChildren(h("span", "lg-filter-name", filterLabel()));
  const chev = h("span", "lg-filter-chev");
  chev.innerHTML = ICON.down;
  b.append(chev);
  b.setAttribute("aria-label", t("ledger.filterAria", { name: filterLabel() }));
}

function filterMenu(b: HTMLElement) {
  const o = shownOwner();
  if (!o) return;
  const now = filter();
  const fleets = [...new Set(S.projects.map((p) => p.fleet).filter((f): f is string => !!f))];
  const pick = (v: string | null) => () => {
    filters.set(o, v);
    render();
  };
  const r = b.getBoundingClientRect();
  ctxMenu.open(r.left, r.bottom, upper(t("section.ledger")), [
    { icon: now === null ? "indicators-check" : "", label: t("ledger.allProjects"), run: pick(null) },
    { sep: true },
    // The fleets, then the projects.
    ...fleets.map((f) => ({ icon: now === `fleet:${f}` ? "indicators-check" : "", label: f, run: pick(`fleet:${f}`) })),
    ...(fleets.length ? [{ sep: true as const }] : []),
    ...S.projects.map((p) => ({ icon: now === p.name ? "indicators-check" : "", label: p.name, run: pick(p.name) })),
  ]);
}

/** Runs on each render, before the modes: shows, hides and fills the panel. */
export function renderLedger() {
  const panel = document.getElementById("ledger");
  if (!panel) return;
  const show = wanted();
  setHidden(panel, !show);
  if (!show) {
    drawn = false;
    setKeys(false);
    return;
  }
  panel.classList.toggle("folded", folded);
  if (folded) {
    drawn = false;
    setKeys(false);
    renderRail(panel);
    return;
  }
  rowContext = presentationKey();
  const sig = JSON.stringify([projectionKey(), rowContext, scopeLabel(), filter(), loaded, error]);
  let list = panel.querySelector<HTMLElement>(".lg-list");
  let need = panel.querySelector<HTMLElement>(".lg-need");
  const fold = panel.querySelector<HTMLElement>(".lg-head .lg-fold");
  if (fold) foldTip(fold, t("ledger.hide"));
  const scope = panel.querySelector<HTMLElement>(".lg-head .lg-scope");
  if (scope) scopeButton(scope);
  const pick = panel.querySelector<HTMLElement>(".lg-head .lg-filter");
  if (pick) filterButton(pick);
  if (drawn && list && panel.dataset.sig === sig) return;
  panel.dataset.sig = sig;
  const us = units();
  const x = sections(us);

  if (!list || !need) {
    need = h("button", "lg-need");
    (need as HTMLButtonElement).type = "button";
    need.title = t("ledger.firstNeed");
    need.addEventListener("click", () => flashNeed(panel));
    const head = h("div", "lg-head");
    const fold = foldButton("fold");
    foldTip(fold, t("ledger.hide"));
    const scope = h("button", "lg-scope");
    (scope as HTMLButtonElement).type = "button";
    keepKeys(scope);
    scope.addEventListener("click", () => setLedgerAllWork(!ledgerAllWork()));
    scopeButton(scope);
    const pick = h("button", "lg-filter");
    (pick as HTMLButtonElement).type = "button";
    pick.setAttribute("aria-haspopup", "menu");
    keepKeys(pick);
    pick.addEventListener("click", () => filterMenu(pick));
    filterButton(pick);
    head.append(h("span", "lg-name", "Ledger"), pick, scope, need, fold);
    list = h("div", "lg-list");
    panel.replaceChildren(head, list);
  }
  const n = needing(us);
  setHidden(need, !n);
  setText(need, tn("ledger.needsYou", n));

  const move = drawn && loaded && !still();
  const out: HTMLElement[] = [];
  if (error) out.push(h("div", "lg-empty", error));
  else if (!x.working.length && !x.open.length && !x.finished.length && filter() !== null) {
    out.push(h("div", "lg-empty", t("ledger.noWorkIn", { name: filterLabel() })));
  } else if (!x.working.length && !x.open.length && !x.finished.length && scopeLabel() && !ledgerAllWork()) {
    out.push(h("div", "lg-empty", t("ledger.noWorkHere")));
  }
  for (const [label, part, fin] of [[t("ledger.working"), x.working, false], [t("ledger.open"), x.open, false], [t("ledger.finished"), x.finished, true]] as const) {
    if (!part.length) continue;
    let heading = sectionRows.get(label);
    if (!heading) sectionRows.set(label, heading = h("div", "lg-sec", label));
    out.push(heading);
    for (const u of part) out.push(...unitRows(u, fin));
  }
  const old = [...list.children].filter((r) => !r.classList.contains("ghost")) as HTMLElement[];
  // Rows have one fixed height; section heads and empty messages can have
  // another. Equal slots have equal positions, even if their content changed.
  const slots = (elements: HTMLElement[]) => {
    const slots = new Map<string, string>();
    let rows = 0;
    const heads: string[] = [];
    for (const el of elements) {
      if (el.dataset.key) slots.set(el.dataset.key, JSON.stringify([rows++, heads]));
      else heads.push(el.textContent ?? "");
    }
    return slots;
  };
  const oldSlots = slots(old);
  const nextSlots = slots(out);
  const before = new Map<string, { top?: number; el: HTMLElement }>();
  // Read only rows whose slots changed or which need a departing ghost.
  // All reads precede list writes; animate batches the destination reads.
  if (move) for (const r of old) if (r.dataset.key) {
    const id = r.dataset.key;
    before.set(id, { top: oldSlots.get(id) !== nextSlots.get(id) ? r.offsetTop : undefined, el: r });
  }
  const keep = new Set(out);
  for (const r of old) if (!keep.has(r)) r.remove();
  let at = list.firstElementChild;
  for (const row of out) {
    if (row !== at) list.insertBefore(row, at);
    at = row.nextElementSibling;
  }
  const ids = new Set(out.map((r) => r.dataset.key));
  for (const id of keptRows.keys()) if (!ids.has(id)) keptRows.delete(id);
  if (move) animate(list, before);
  if (keys) light();
  drawn = loaded;
  if (flashNext) {
    flashNext = false;
    flashNeed(panel);
  }
}

// ── Keys ────────────────────────────────────────────────────────────────────

/** The keys are in the panel. The panel wears the focused pane's frame meanwhile. */
let keys = false;

function setKeys(on: boolean) {
  keys = on;
  // One frame on screen: Skipper's pane drops its frame while the panel has the keys.
  document.body.classList.toggle("ledger-keys", on);
}
/** The lit row's uid. */
let lit: string | null = null;

const rowEls = () => [...document.querySelectorAll<HTMLElement>("#ledger .lg-list .lg-row[data-key]:not(.ghost)")];

/** Lights the row `lit` names, else the first that needs the person, else the first, and gives it the keys. */
function light() {
  const rs = rowEls();
  for (const r of rs) r.classList.remove("lit");
  const r = rs.find((x) => x.dataset.key === lit) ?? rs.find((x) => x.classList.contains("need")) ?? rs[0];
  if (!r) return;
  lit = r.dataset.key!;
  r.classList.add("lit");
  r.focus({ preventScroll: true });
  r.scrollIntoView({ block: "nearest" });
}

function step(dir: 1 | -1) {
  const rs = rowEls();
  if (!rs.length) return;
  const i = rs.findIndex((x) => x.dataset.key === lit);
  lit = rs[i < 0 ? 0 : (i + dir + rs.length) % rs.length].dataset.key!;
  light();
}

/** Ctrl+Shift+Right: the keys go to the panel. A folded panel opens first. */
function enter() {
  if (folded) toggleLedger();
  setKeys(true);
  light();
}

/** Ctrl+Shift+Left or Esc: the keys go back to the Skipper's pane. */
function leave() {
  setKeys(false);
  for (const r of rowEls()) r.classList.remove("lit");
  const s = owner();
  if (s) panes.get(s.id)?.term.focus();
}

/** Plain keys on a lit row: the arrows step, Enter clicks, Esc leaves. */
function rowKey(e: KeyboardEvent) {
  if (!keys || e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
  const r = (e.target as HTMLElement).closest<HTMLElement>(".lg-row[data-key]");
  if (!r) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") step(e.key === "ArrowDown" ? 1 : -1);
  else if (e.key === "Enter") r.click();
  else if (e.key === "Escape") leave();
  else return;
  e.preventDefault();
}

// The keys left the panel (a click, F6, a pane, an opened row): the light goes.
document.addEventListener("focusin", (e) => {
  if (!keys || document.getElementById("ledger")?.contains(e.target as Node)) return;
  setKeys(false);
  for (const r of rowEls()) r.classList.remove("lit");
});

/**
 * The list keys while the Ledger is on screen, and the maximize key on its lit
 * row. True when the Ledger took the key. From a project or fleet row, Ctrl+Shift+Right keeps its list meaning.
 */
export function ledgerKey(a: Action): boolean {
  if (!ledgerShown()) return false;
  if (keys) {
    if (a === "session-next" || a === "session-prev") step(a === "session-next" ? 1 : -1);
    else if (a === "list-project") leave();
    // Ctrl+Shift+Enter opens the lit row, as Enter does. No pane to maximize from here.
    else if (a === "maximize") rowEls().find((x) => x.dataset.key === lit)?.click();
    else return a === "list-back";
    return true;
  }
  if (a !== "list-back") return false;
  const el = document.activeElement;
  if (el?.classList.contains("project-pick") || el?.classList.contains("fleet-pick")) return false;
  enter();
  return true;
}

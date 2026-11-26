import { hostFields } from "../platform/invoke";
/**
 * The Skippers view: the side panel's second view, beside the tree. One flat
 * list: the Skiff agent (Admiral) first, then each fleet Skipper and project
 * Skipper in tree order. A row is the Skipper's avatar, its project or fleet,
 * the time of its last output, and the latest line of its work. A click or
 * Enter opens its pane. The rows are session rows, so the list keys work as
 * in the tree.
 *
 * Line 1 is the face, the model's icon in its own color, the name and the
 * fleet tag. Line 2 is one state word, then the work. The most urgent state
 * wins (`wordOf`). The number shows only while Ctrl+Shift is held. The time
 * of the last output is in the tooltip.
 */
import { agentName, scopeName } from "./model";
import { faceSig, skipperFace } from "./skipperFace";
import { BELL, FAILED, FLAG, workingIcon } from "../appearance/stateIcon";
import { icon } from "../ui/icons";
import { agentIcon, launchIcon } from "../appearance/agentIcon";
import { ledgerOpen, ledgerWorkTitle } from "./ledger";
import { reported } from "./programStatus";
import { isMac } from "../app/keys";
import { rowMenu, skiffAgentMenu } from "./menus";
import { revealSession } from "./view";
import { clearSelection, keepRow } from "./selection";
import { openSkiffAgent, projectWatch, SKIFF_AGENT_KEY, skiffAgentChoice, skiffWatch, watcherFailed, type WatchState } from "../app/skiffAgent";
import { dragSkipper } from "./skipperDrag";
import { numberedSkippers as numbered, skipperEntries, type Entry } from "./skipperOrder";
import { accent } from "../app/stateQueries";
import { DEFAULT_ACCENT, S } from "../app/state";
import { $, h } from "../ui/dom";
import { tally } from "../diagnostics/stalls";
import { render } from "../app/renderRequest";
import { t, tn } from "../i18n";
import { compactWhen } from "../i18n/time";

type SidebarView = "tree" | "skippers";

const VIEW_KEY = "skiff.sidebar.view";

function loadView(): SidebarView {
  try {
    return localStorage.getItem(VIEW_KEY) === "skippers" ? "skippers" : "tree";
  } catch {
    return "tree";
  }
}

let view: SidebarView = loadView();

/** True while the side panel shows the Skippers list. */
export const skippersOn = () => view === "skippers";

/** Shows the tree or the Skippers list. The choice lasts across restarts. */
export function setSidebarView(v: SidebarView) {
  if (v === view) return;
  view = v;
  try {
    localStorage.setItem(VIEW_KEY, v);
  } catch {
    /* the choice lasts for this run */
  }
  render("sidebar-view");
}

/** The Skippers that wait on the person: the count on the toggle. */
const waitingSkippers = () => skipperEntries().filter((e) => e.s?.state === "waiting").length;

/** The ledger owner of a Skipper: "grand" for the Skiff agent, else its project or fleet scope. */
const ownerOf = (e: Entry) => (e.kind === "admiral" ? "grand" : e.scope);

/**
 * The latest line of a Skipper's work: its newest open ledger work, else the
 * message its program reported for the last turn, else the terminal title,
 * else the agent's name. The Skiff agent's work is the ledger's "grand" rows.
 */
function latestLine(e: Entry): string {
  const s = e.s;
  if (!s) {
    const a = skiffAgentChoice();
    return a ? t("sidebar.agentStart", { agent: a.id }) : t("sidebar.noAgentOn");
  }
  const owner = ownerOf(e);
  const work = owner ? ledgerWorkTitle(owner) : null;
  const msg = reported(s)?.msg?.trim();
  const title = s.title?.replace(/^[^\p{L}\p{N}]+/u, "").trim();
  return work || msg || title || agentName(s);
}

const CLOCK = '<circle cx="12" cy="12" r="10"></circle><path d="M12 7v5l3 2"></path>';
const SHELL = '<path d="M4 17l6-5-6-5"></path><path d="M12 19h8"></path>';

type Word = { key: string; text: string; mark: () => HTMLElement | null };

const mark = (paths: string) => () => icon(paths) as unknown as HTMLElement;

/**
 * The row's one state word. The most urgent wins: Needs you, Watcher failed,
 * Working, Waiting on N, Done, Idle, Agent exited. Waiting on N: the watcher
 * runs, the Skipper is idle, and N of its ledger tasks are open.
 */
function wordOf(e: Entry, watch: { state: WatchState } | null): Word {
  const s = e.s;
  if (!s) return { key: "off", text: t("skippers.wordOff"), mark: () => null };
  if (s.state === "waiting") return { key: "needs", text: t("skippers.wordNeeds"), mark: mark(BELL) };
  if (watcherFailed(s)) return { key: "watch-failed", text: t("skippers.wordWatchFailed"), mark: mark(FAILED) };
  if (s.state === "working") return { key: "working", text: t("skippers.wordWorking"), mark: workingIcon };
  const exited = s.state === "done" || s.agent_exit != null;
  const owner = ownerOf(e);
  const open = owner ? ledgerOpen(owner) : 0;
  if (!exited && watch?.state === "runs" && open > 0) return { key: "waiting-on", text: tn("skippers.wordWaitingOn", open), mark: mark(CLOCK) };
  if (s.unread) return { key: "done", text: t("skippers.wordDone"), mark: mark(FLAG) };
  if (!exited) return { key: "idle", text: t("skippers.wordIdle"), mark: () => null };
  return { key: "exited", text: t("skippers.wordExited"), mark: mark(SHELL) };
}

/** The time today, else the day: "Oct 29". */
const when = (ms: number) => (ms ? compactWhen(ms) : "");

/** The watcher, as the tree's Skipper rows read it. */
function watchOf(e: Entry): { state: WatchState; text: string } | null {
  if (!e.s) return null;
  return e.kind === "admiral" ? skiffWatch(e.s) : projectWatch(e.s);
}

/** What a row shows, so an unchanged row is kept and its loops keep their phase. */
function rowSig(e: Entry, watch: { state: WatchState; text: string } | null, num: number): string {
  const s = e.s;
  return JSON.stringify([
    e.kind, e.name, s?.id ?? null, s?.state ?? null, s?.unread ?? false, s?.exit_code ?? null, s?.agent_exit ?? null,
    latestLine(e), wordOf(e, watch).text, when(s?.last_output_at ?? 0), watch, s ? S.selection.includes(s.id) : false,
    faceSig(e.kind === "admiral" ? null : e.scope), num,
  ]);
}

/** Opens a Skipper's pane, as a click on its row does. */
function openEntry(e: Entry) {
  const s = e.s;
  clearSelection();
  if (e.kind === "admiral") {
    if (!s) keepRow(SKIFF_AGENT_KEY);
    return openSkiffAgent();
  }
  revealSession(s!.id);
  requestAnimationFrame(() => keepRow(s!.id));
}

/** Ctrl+Shift+1..9 in the Skippers list: the Skipper with that number. */
export function pickSkipperRow(n: number) {
  const e = numbered()[n - 1];
  if (!e) return;
  openEntry(e);
  lightRow(e.s!.id);
}

/** Lights a Skipper's row after a key opened it. A key can pick a row below the fold; a click cannot. */
function lightRow(id: string) {
  requestAnimationFrame(() => {
    keepRow(id);
    document.querySelector(`.sk-row[data-session="${id}"]`)?.scrollIntoView({ block: "nearest" });
  });
}

/** `num`: the row's number key, 0 for none. */
function skipperRow(e: Entry, watch: { state: WatchState; text: string } | null, num: number): HTMLElement {
  const s = e.s;
  const color = e.project ? accent(e.project) : DEFAULT_ACCENT;
  const row = h("button", "session-row sk-row" + (s ? ` st-${s.state}` : "") + (s && S.selection.includes(s.id) ? " selected" : ""));
  row.type = "button";
  row.style.setProperty("--pc", color);
  if (s) row.dataset.session = s.id;
  else row.dataset.key = SKIFF_AGENT_KEY;
  const name = h("span", "sk-name", e.kind === "fleet" ? scopeName(e.scope!) : e.name);
  const top = h("span", "sk-top");
  // The number key shows only while Ctrl+Shift is held (body.mods-held).
  if (num) top.appendChild(h("span", "sk-num", String(num)));
  // The model beside the face, as the tree shows it: the running agent, or the one a start runs. Its own color.
  top.append(s ? agentIcon(s) : launchIcon(skiffAgentChoice()), name);
  if (e.kind === "fleet") top.appendChild(h("span", "sk-tag", "fleet"));
  const word = wordOf(e, watch);
  const state = h("span", `sk-state w-${word.key}`);
  const m = word.mark();
  if (m) state.appendChild(m);
  state.appendChild(document.createTextNode(word.text));
  const line = h("span", "sk-line");
  line.append(state, h("span", "title", latestLine(e)));
  const text = h("span", "sk-text");
  text.append(top, line);
  const face = skipperFace(e.kind === "admiral" ? null : e.scope, 40);
  face.classList.add("sk-face");
  row.append(face, text);
  if (e.scope) row.dataset.scope = e.scope;
  const what = e.kind === "admiral" ? t("skippers.admiral", { name: e.name }) : t(e.kind === "fleet" ? "skipper.avatarFleet" : "skipper.avatarProject", { scope: scopeName(e.scope!), skipper: "Skipper" });
  const vars = { what, line: latestLine(e), state: word.text, watch: watch?.text ?? "" };
  row.setAttribute("aria-label", watch?.text ? t("skippers.rowLabelWatch", vars) : t("skippers.rowLabel", vars));
  const last = when(s?.last_output_at ?? 0);
  const tip = [last ? t("skippers.lastOutput", { time: last }) : "", watch?.text ?? ""].filter(Boolean).join("\n");
  if (tip) row.title = tip;
  row.addEventListener("mousedown", (ev) => {
    if (ev.button === 0) row.focus();
    if (e.scope) dragSkipper(ev, row, e.scope, name.textContent ?? "");
  });
  row.addEventListener("click", () => openEntry(e));
  row.addEventListener("contextmenu", (ev) => {
    ev.preventDefault();
    if (s) rowMenu(s, ev.clientX, ev.clientY);
    else skiffAgentMenu(null, ev.clientX, ev.clientY);
  });
  return row;
}

/** Ctrl+Shift (⌘+Shift on macOS) held: the rows show their number keys. */
function syncMods(e: KeyboardEvent) {
  const held = (isMac ? e.metaKey : e.ctrlKey) && e.shiftKey;
  if (document.body.classList.contains("mods-held") !== held) document.body.classList.toggle("mods-held", held);
}
window.addEventListener("keydown", syncMods, true);
window.addEventListener("keyup", syncMods, true);
window.addEventListener("blur", () => document.body.classList.remove("mods-held"));

let kept = new Map<string, [sig: string, row: HTMLElement]>();

hostFields(() => ({ kept: new Map<string,[string,HTMLElement]>() }), () => ({ kept }),
  (f) => { ({ kept } = f); });

/** Draws the Skippers list into the side panel. Unchanged rows stay. */
export function renderSkippers(side: HTMLElement) {
  const next = new Map<string, [string, HTMLElement]>();
  const rows: HTMLElement[] = [];
  const all = skipperEntries();
  const nums = numbered(all);
  for (const e of all) {
    const key = e.s?.id ?? SKIFF_AGENT_KEY;
    const watch = watchOf(e);
    const num = nums.indexOf(e) + 1;
    const sig = rowSig(e, watch, num);
    const old = kept.get(key);
    const row = old && old[0] === sig ? old[1] : (tally("sidebar.row"), skipperRow(e, watch, num));
    next.set(key, [sig, row]);
    rows.push(row);
  }
  kept = next;
  if (!rows.length) rows.push(h("div", "note-row", t("skippers.none")));
  if (side.children.length === rows.length && rows.every((r, i) => side.children[i] === r)) return;
  side.replaceChildren(...rows);
}

/** The TREE | SKIPPERS toggle at the top of the side panel, with the count of Skippers that wait. */
export function renderViewToggle() {
  const bar = $<HTMLElement>("sidebar-view");
  if (!bar.childElementCount) {
    for (const [v, label] of [["tree", t("skippers.tabTree")], ["skippers", t("skippers.tabSkippers")]] as const) {
      const b = h("button", "sv-tab", label);
      b.type = "button";
      b.tabIndex = -1;
      b.dataset.view = v;
      b.addEventListener("click", () => setSidebarView(v));
      bar.appendChild(b);
    }
    bar.lastElementChild!.appendChild(h("span", "sv-count"));
  }
  for (const b of bar.querySelectorAll<HTMLElement>(".sv-tab")) {
    const on = b.dataset.view === view;
    b.classList.toggle("on", on);
    b.setAttribute("aria-pressed", String(on));
  }
  const n = waitingSkippers();
  const count = bar.querySelector<HTMLElement>(".sv-count")!;
  if (count.textContent !== (n ? String(n) : "")) count.textContent = n ? String(n) : "";
}

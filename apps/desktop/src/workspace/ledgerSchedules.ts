/**
 * The Ledger's Schedules tab (#262): one item of two 20px lines for each
 * schedule, and a schedule's series in the same tab. Read-only, as the Tasks
 * tab: only the CLI and Settings › Schedules change schedules.
 *
 * An item: the dot of the last run, the name and the When words, the next
 * run and the target, the last outcome. A click or Enter opens the series: a
 * head with the name, the When words, the target and the next run, then the
 * runs, newest first. A run is an ordinary ledger row; its agent step is a
 * child row under it. A click on a run opens its result; on an agent step,
 * its pane while it runs, else its result. ← or Esc goes back to the list.
 */
import { skiffAgentName, skipperWord } from "../app/skiffAgent";
import { S, sessions } from "../app/state";
import { accent } from "../app/stateQueries";
import { type MessageKey, t } from "../i18n";
import { h } from "../ui/dom";
import { scopeFleet } from "./model";
import { titleOf, type Entry } from "./ledgerStore";
import { schedules, targetProject, targetWords, type Run, type Schedule } from "./scheduleStore";

const CHEV = '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M3.5 1.5 7 5 3.5 8.5"/></svg>';
const BACK = '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M6.5 1.5 3 5l3.5 3.5"/></svg>';

/** The schedule shows under the panel's filter: All projects, a fleet, or one project. */
export function scheduleInView(s: Schedule, filter: string | null): boolean {
  if (filter === null) return true;
  const ps = [s.project, targetProject(s)].filter((p): p is string => !!p);
  const fleet = scopeFleet(filter);
  // A fleet Skipper's schedule shows under its fleet.
  if (fleet !== null) return s.owner === filter || ps.some((p) => S.projects.find((x) => x.name === p)?.fleet === fleet);
  return ps.includes(filter);
}

export const shownSchedules = (filter: string | null) => schedules.filter((s) => scheduleInView(s, filter));

/** The dot and word class of an outcome: ok, run, bad, none. */
function tone(last: Schedule["last"] | Run["status"]): string {
  if (last === "succeeded") return "ok";
  if (last === "running") return "run";
  if (last === "failed") return "bad";
  return "none";
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** `Nov 11 01:30` short: the time today, the weekday and time this week, else the date. */
export function shortNext(next: string): string {
  const m = /^([A-Z][a-z]{2}) (\d{1,2}) (\d{2}:\d{2})$/.exec(next);
  if (!m) return next;
  const now = new Date();
  const month = MONTHS.indexOf(m[1]);
  let year = now.getFullYear();
  if (month < now.getMonth() - 6) year++;
  const at = new Date(year, month, Number(m[2]));
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const days = Math.round((at.getTime() - today.getTime()) / 86_400_000);
  if (days === 0) return m[3];
  if (days > 0 && days < 7) return `${DAYS[at.getDay()]} ${m[3]}`;
  return `${m[1]} ${m[2]}`;
}

/** The project's letters in its color, else nothing. */
function tag(project: string | null): HTMLElement | null {
  const p = project ? S.projects.find((x) => x.name === project) : null;
  if (!p) return null;
  const el = h("span", "lg-tag", p.short);
  el.style.setProperty("--pc", accent(p));
  return el;
}

export const lastWord = (last: Schedule["last"] | Run["status"]) => (last ? t(`schedules.last.${last}` as MessageKey) : t("schedules.noRun"));

/** A fleet Skipper's name, as its session has it: "Harbor Skipper". */
export const fleetSkipperName = (fleet: string) => `${fleet} ${skipperWord()}`;

/** A Skipper by its owner key, as its session is named: "Admiral", "Harbor Skipper", "gesher Skipper". */
export function skipperOwnerName(owner: string): string {
  if (owner === "grand") return skiffAgentName();
  return owner.startsWith("fleet:") ? fleetSkipperName(owner.slice(6)) : `${owner} ${skipperWord()}`;
}

export const targetText = (s: Schedule) => targetWords(s, { skipper: skipperOwnerName, command: t("schedules.targetCommand"), nothing: t("schedules.nothing") });

/** One schedule: two 20px lines on the tab's column grid. */
function item(s: Schedule, open: (uid: string) => void): HTMLElement {
  const row = h("div", "lg-row sc-row" + (s.paused_at ? " paused" : ""));
  row.dataset.key = `sc:${s.uid}`;
  row.tabIndex = -1;
  row.setAttribute("aria-label", s.name);
  const target = h("span", "sc-target");
  const lead = tag(targetProject(s));
  if (lead) target.append(lead);
  target.append(h("span", "sc-target-name", targetText(s)));
  const chev = h("span", "sc-chev");
  chev.innerHTML = CHEV;
  row.append(
    h("span", `sc-dot ${s.paused_at ? "none" : tone(s.last)}`),
    h("span", "sc-name", s.name),
    h("span", "sc-next", s.next ? shortNext(s.next) : t("schedules.paused")),
    h("span", `sc-last ${tone(s.last)}`, s.last ? lastWord(s.last) : ""),
    h("span", "sc-when", s.when),
    target,
    chev,
  );
  row.addEventListener("click", () => open(s.uid));
  return row;
}

/** The column heads of the list. */
function cols(): HTMLElement {
  const row = h("div", "sc-cols");
  row.append(h("span"), h("span", "", t("schedules.colSchedule")), h("span", "", t("schedules.colNext")), h("span", "", t("schedules.colLast")), h("span"));
  return row;
}

/** What a run row and an agent step row do on a click. */
export interface SeriesActs {
  /** The run's or step's result, when the Host has it. */
  result: (r: Entry) => (() => void) | null;
  /** The step's live pane. */
  pane: (r: Entry) => (() => void) | null;
}

/** The right-hand word of a run: how the command ended, else the status. */
function runWord(r: Run): string {
  if (r.status === "pending" || r.status === "running") return lastWord(r.status);
  return r.row.ref || lastWord(r.status);
}

function runRow(r: Run, a: SeriesActs): HTMLElement[] {
  const click = a.result(r.row);
  const row = h("div", `lg-row sr-run ${tone(r.status)}` + (click ? "" : " dead"));
  row.dataset.key = r.row.uid;
  row.tabIndex = -1;
  row.setAttribute("aria-label", r.at);
  row.append(h("span", "lg-dot"), h("span", "lg-title", r.at), h("span", "lg-pane", runWord(r)));
  if (click) row.addEventListener("click", click);
  const kids = r.kids.map((k) => {
    const live = k.state !== "done" ? a.pane(k) : null;
    const go = live ?? a.result(k);
    const kid = h("div", `lg-row kid s-${k.state === "done" ? "done" : k.state === "pending" ? "pending" : "sent"}` + (go ? "" : " dead"));
    kid.dataset.key = k.uid;
    kid.tabIndex = -1;
    kid.setAttribute("aria-label", titleOf(k));
    const where = k.pane_id ? sessions.get(k.pane_id)?.name ?? k.type ?? "" : k.type ?? "";
    kid.append(h("span", "lg-dot"), h("span", "lg-title", k.kind === "handoff" ? t("schedules.handoff") : k.task), h("span", "lg-pane", where));
    if (go) kid.addEventListener("click", go);
    return kid;
  });
  return [row, ...kids];
}

/** The series head: back, the name and the next run; under it the words and the target. */
function seriesHead(s: Schedule, back: () => void): HTMLElement {
  const head = h("div", "sr-head");
  const b = h("button", "sr-back");
  (b as HTMLButtonElement).type = "button";
  b.innerHTML = BACK;
  b.setAttribute("aria-label", t("schedules.back"));
  b.addEventListener("mousedown", (e) => e.preventDefault());
  b.addEventListener("click", back);
  const gate = s.command && s.target && s.gate && s.gate !== "always" ? `, ${t(`schedules.gate.${s.gate}` as MessageKey)}: ${targetText(s)}` : s.command && s.target ? `, ${targetText(s)}` : s.command ? "" : targetText(s);
  const what = s.command ? `${t("schedules.targetCommand")}${gate}` : gate;
  head.append(
    b,
    h("span", "sr-name", s.name),
    h("span", "sr-next", s.next ? t("schedules.nextAt", { at: shortNext(s.next) }) : t("schedules.paused")),
    h("span", "sr-when", `${s.when} · ${what}`),
  );
  return head;
}

/** The tab's rows: the list, or the series of `open`. The head of a series goes above the list. */
export function scheduleView(filter: string | null, open: string | null, acts: SeriesActs & { open: (uid: string) => void; back: () => void }): { head: HTMLElement | null; rows: HTMLElement[] } {
  const s = open ? schedules.find((x) => x.uid === open) : null;
  if (s) {
    const rows: HTMLElement[] = [h("div", "lg-sec", t("schedules.runs"))];
    if (!s.runs.length) rows.push(h("div", "lg-empty", t("schedules.noRuns")));
    for (const r of s.runs) rows.push(...runRow(r, acts));
    return { head: seriesHead(s, acts.back), rows };
  }
  const list = shownSchedules(filter);
  if (!list.length) return { head: null, rows: [h("div", "lg-empty", t("schedules.none"))] };
  return { head: null, rows: [cols(), ...list.map((x) => item(x, acts.open))] };
}

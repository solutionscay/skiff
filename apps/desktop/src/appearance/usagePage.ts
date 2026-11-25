/**
 * Settings › Usage (#276): one band for each agent the Host has, in Settings ›
 * Agents order, and one row for each window its last read returned. Every
 * number is percent used. The page adds no row the source did not return.
 */
import { h } from "../ui/dom";
import { smallButton } from "../ui/controls";
import { locale, t } from "../i18n";
import type { AgentInfo } from "../platform/types";
import { launchIcon } from "./agentIcon";
import { readUsage, usage, usageError } from "../app/usage";
import { hostCan } from "../app/hostCaps";
import { type AgentUsage, type UsageWindow, countdown, level, live, pace, productName, readClock, resetClock, usedLeft, windowName } from "../app/usageModel";

/** The agents the Host has a reader for. */
const READERS = new Set(["claude", "codex", "grok"]);
/** The agents the page leaves out: Antigravity (gemini) has no usage row. */
const UNLISTED = new Set(["gemini"]);

/** The agents the page lists: installed, and not left out. */
const listed = (agents: AgentInfo[]) => agents.filter((a) => a.installed && !UNLISTED.has(a.id));

/** The program of an agent's command line, as the band shows it: "claude". */
const program = (a: AgentInfo) => (a.command.trim().split(/\s+/)[0] ?? a.id).split("/").pop() || a.id;

/** The installed agents. A count for the page row: "4 of 6", agents with a reader of agents shown. */
export function usageCount(agents: AgentInfo[]): string {
  // A Host without usage reads gives no count.
  if (!hostCan("agent_usage")) return "";
  const shown = listed(agents);
  if (!shown.length) return "";
  return t("settings.ofCount", { on: shown.filter((a) => usage.some((u) => u.agent === a.id)).length, all: shown.length });
}

export function usageSection(agents: AgentInfo[]): HTMLElement {
  const main = h("div", "set-main u-page");
  const head = h("div", "set-head");
  const acts = h("div", "set-head-acts");
  acts.append(smallButton(t("usage.readNow"), () => void readUsage("all")));
  head.append(h("div", "set-h", t("settings.tab.usage")), acts);
  main.append(head);
  if (usageError) {
    const err = h("div", "set-error", usageError);
    err.setAttribute("role", "alert");
    main.append(err);
  }
  // A Host without usage reads: its refusal only. It cannot say which agent has a usage command.
  if (!hostCan("agent_usage")) {
    main.append(h("div", "set-note", t("hostCaps.refusal", { what: t("hostCaps.agent_usage") })));
    return main;
  }
  const cols = h("div", "set-row set-cols");
  cols.append(h("span", "", t("usage.col.limit")), h("span", "", t("usage.col.used")), h("span", "", t("usage.col.resets")));
  main.append(cols);
  const now = Date.now();
  // Not installed: not shown, as Settings › Agents "Not found".
  for (const a of listed(agents)) main.append(...agentRows(a, usage.find((u) => u.agent === a.id), now));
  return main;
}

/** The band, then the error, the windows and the extra values. */
function agentRows(a: AgentInfo, u: AgentUsage | undefined, now: number): HTMLElement[] {
  const name = a.custom ? a.id : productName(a.id);
  const band = h("div", "set-band u-band");
  const title = h("span", "set-band-title");
  title.append(launchIcon(a), h("span", "u-name", name), h("span", "u-cmd", program(a)));
  band.append(title);
  // A reader whose first answer has not come: the band alone. No reader: the note.
  if (!u) return READERS.has(a.id) ? [band] : [band, note(t("usage.noSource", { name }))];
  const right = [u.plan, u.read_at ? t("usage.readAt", { time: readClock(u.read_at) }) : ""].filter(Boolean).join(" · ");
  band.append(h("span", "set-band-count", right));
  const rows: HTMLElement[] = [band];
  if (u.error) {
    const err = h("div", "set-error", t("usage.failed", { time: readClock(u.error.at), message: u.error.message }));
    err.setAttribute("role", "alert");
    rows.push(err);
  }
  if (u.read_at === undefined) {
    if (u.reading) rows.push(note(t("usage.reading")));
  } else if (!u.windows.length) rows.push(note(t("usage.noWindows")));
  rows.push(...u.windows.map((w) => windowRow(w, now)));
  if (u.credits) {
    const value = u.credits.unlimited ? t("usage.unlimited") : (u.credits.balance ?? 0).toLocaleString(locale(), { maximumFractionDigits: 0 });
    const row = h("div", "set-row");
    row.append(h("span", "u-win", t("usage.creditsRow")), h("span", "u-text", value), h("span", ""));
    rows.push(row);
  }
  return rows;
}

function note(text: string): HTMLElement {
  const row = h("div", "set-row note");
  row.append(h("span", "", text));
  return row;
}

/** Limit · Used · Resets. A window whose reset time has passed shows no number. */
function windowRow(w: UsageWindow, now: number): HTMLElement {
  const row = h("div", "set-row");
  const name = h("span", "u-win");
  if (w.group) name.append(h("span", "u-grp", w.group));
  name.append(windowName(w));
  const meter = h("span", "u-meter");
  const bar = h("span", "u-bar");
  const pct = h("span", "u-pct");
  const resets = h("span", "u-reset");
  if (live(w, now)) {
    const lvl = level(w);
    const fill = h("span", `u-fill ${lvl}`);
    fill.style.width = `${Math.min(w.used, 100)}%`;
    bar.append(fill);
    const gone = pace(w, now);
    if (gone !== null) {
      const tick = h("span", "u-tick");
      tick.style.left = `${gone * 100}%`;
      tick.title = t("usage.paceTip", { n: Math.round(gone * 100) });
      bar.append(tick);
    }
    pct.className = `u-pct ${lvl}`;
    pct.textContent = `${w.used}%`;
    meter.title = usedLeft(w.used);
    if (w.resets_at !== undefined) resets.append(resetClock(w.resets_at), h("span", "u-in", countdown(w.resets_at, now)));
    else resets.append(h("span", "u-in", "–"));
  } else {
    // After a reset with no new read: never the old %.
    resets.append(h("span", "u-in", "–"));
  }
  meter.append(bar, pct);
  row.append(name, meter, resets);
  return row;
}

/**
 * Agent plan usage as the Host reads it (`agent_usage@1`), and the rules the
 * Usage page and the top bar cell share. Every number is percent used: the
 * Host's readers turned "left" into "used" already.
 */
import { t } from "../i18n";
import { clockTime, weekdayTime } from "../i18n/time";

export interface UsageWindow {
  /** The vendor's group or model name: "Gemini Models", "Fable". */
  group?: string;
  /** The window's length: 300 is 5 hours, 10080 a week. */
  minutes: number;
  /** Grok's weekly credits. */
  credits?: boolean;
  /** Percent used, 0 to 100. */
  used: number;
  /** Unix ms. Absent: the source gives no reset time. */
  resets_at?: number;
  /** 90 for 5 hours or less, 80 for longer. */
  threshold: number;
}

export interface AgentUsage {
  agent: string;
  plan?: string;
  /** Unix ms of the last read that worked. */
  read_at?: number;
  windows: UsageWindow[];
  credits?: { balance?: number; unlimited?: boolean };
  error?: { at: number; message: string };
  reading: boolean;
}

type Level = "ok" | "near" | "limit";

/** Below the threshold, from it, and at 100% (decision 3). */
export function level(w: UsageWindow): Level {
  if (w.used >= 100) return "limit";
  return w.used >= w.threshold ? "near" : "ok";
}

/** The window's values still hold: its reset time has not passed. */
export const live = (w: UsageWindow, now: number) => w.resets_at === undefined || w.resets_at > now;

/** The product names (decision 6). A custom agent keeps its Settings › Agents name. */
const PRODUCTS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Antigravity",
  grok: "Grok",
  opencode: "OpenCode",
  copilot: "Copilot",
  hermes: "Hermes",
  kimi: "Kimi",
};
export const productName = (id: string) => PRODUCTS[id] ?? id;

/** The window's name from its length, never from its place: "5-hour", "Daily", "Weekly". */
function spanName(minutes: number): string {
  if (minutes === 1440) return t("usage.span.daily");
  if (minutes === 10080) return t("usage.span.weekly");
  if (minutes % 1440 === 0) return t("usage.span.days", { n: minutes / 1440 });
  if (minutes % 60 === 0) return t("usage.span.hours", { n: minutes / 60 });
  return t("usage.span.minutes", { n: minutes });
}

/** The Limit column's name, without the group: "Weekly", "Weekly credits". */
export const windowName = (w: UsageWindow) => (w.credits ? t("usage.credits", { span: spanName(w.minutes) }) : spanName(w.minutes));

/** The name in a sentence, after the group or the agent: "Fable weekly", "Codex weekly". */
export const inlineName = (w: UsageWindow, lead: string) => t("usage.inline", { lead, window: windowName(w).toLocaleLowerCase() });

/** The share of the window that has gone, 0 to 1. `null`: no reset time. */
export function pace(w: UsageWindow, now: number): number | null {
  if (w.resets_at === undefined || !w.minutes) return null;
  const length = w.minutes * 60_000;
  return Math.min(1, Math.max(0, (now - (w.resets_at - length)) / length));
}

/** "2% used · 98% left". */
export const usedLeft = (used: number) => t("usage.usedLeft", { used, left: 100 - used });

/** The clock time of a reset: "Wed 09:44". */
export const resetClock = (ms: number) => weekdayTime(ms);

/** The time of a read: "05:10". */
export const readClock = (ms: number) => clockTime(ms);

/** The time to a reset: "in 1 d 4 h", "in 21 h", "in 12 min". */
export function countdown(ms: number, now: number): string {
  const min = Math.max(0, Math.round((ms - now) / 60_000));
  const d = Math.floor(min / 1440);
  const hrs = Math.floor((min % 1440) / 60);
  if (d) return t("usage.in.days", { d, h: hrs });
  if (hrs) return t("usage.in.hours", { h: hrs });
  return t("usage.in.minutes", { m: min });
}

/** One window at or above its threshold, for the top bar cell. */
interface Hot {
  agent: string;
  w: UsageWindow;
}

/**
 * The top bar cell (§2): every live window at or above its threshold. `top`
 * is the agent with the highest % used, `more` the count of other agents.
 * `null` at rest: the cell is not there.
 */
export function cellOf(list: AgentUsage[], now: number): { agent: string; w: UsageWindow; more: number; tip: string } | null {
  const hot: Hot[] = list.flatMap((a) => a.windows.filter((w) => live(w, now) && level(w) !== "ok").map((w) => ({ agent: a.agent, w })));
  if (!hot.length) return null;
  // Highest % first; the order of the list breaks a tie.
  const top = hot.reduce((a, b) => (b.w.used > a.w.used ? b : a));
  const agents = new Set(hot.map((x) => x.agent));
  const tip = hot
    .map(({ agent, w }) => {
      const lead = w.group ? `${productName(agent)} ${w.group}` : productName(agent);
      const line = t("usage.tip.window", { window: inlineName(w, lead), usedLeft: usedLeft(w.used) });
      return w.resets_at === undefined ? line : `${line} ${t("usage.tip.resets", { time: resetClock(w.resets_at) })}`;
    })
    .join("\n");
  return { agent: top.agent, w: top.w, more: agents.size - 1, tip };
}

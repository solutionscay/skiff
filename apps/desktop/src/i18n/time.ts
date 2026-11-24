/**
 * Every time and date the app shows goes through here. Two settings shape
 * them, from Settings › Appearance: the time zone (Auto: the system's, or an
 * IANA zone) and the clock (Auto: the app language's, 24-hour or 12-hour).
 * Words come from the app language, never from the system locale alone.
 * Stored values and file names stay in UTC; only the display changes.
 */
import { locale, t } from "./index";

/** `24`, `12`, or null for the app language's clock. */
type ClockSetting = "24" | "12" | null;

let zone: string | undefined;
let clockSet: ClockSetting = null;
const cache = new Map<string, Intl.DateTimeFormat>();

/** A zone name Intl knows: "Europe/Madrid", "UTC". */
export function validZone(name: string): boolean {
  try {
    new Intl.DateTimeFormat("en", { timeZone: name });
    return true;
  } catch {
    return false;
  }
}

/** Sets both settings. An unknown zone gives the system's. The next draw shows them. */
export function setTimeDisplay(timeZone: string | null | undefined, clock: string | null | undefined) {
  zone = timeZone && validZone(timeZone) ? timeZone : undefined;
  clockSet = clock === "12" || clock === "24" ? clock : null;
  cache.clear();
}

/** The zone the app shows: the chosen one, else the system's. */
export const shownZone = () => zone ?? systemZone();

/** The system's zone: "America/Panama". */
export const systemZone = () => new Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";

/** The zones to choose from, sorted, with UTC. */
export function zones(): string[] {
  const list = typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [];
  return [...new Set(["UTC", ...list])].sort();
}

/** The clock the app language uses when the setting is Auto. */
export function languageHour12(): boolean {
  const cycle = new Intl.DateTimeFormat(locale(), { hour: "numeric" }).resolvedOptions().hourCycle;
  return cycle === "h11" || cycle === "h12";
}

/** True when times show as "2:05 PM". */
export const hour12 = () => (clockSet === null ? languageHour12() : clockSet === "12");

function fmt(kind: string, o: Intl.DateTimeFormatOptions, utc = false): Intl.DateTimeFormat {
  const key = `${kind}|${utc}`;
  let f = cache.get(key);
  if (!f) {
    f = new Intl.DateTimeFormat(locale(), { ...o, timeZone: utc ? "UTC" : zone });
    cache.set(key, f);
  }
  return f;
}

/** The hour and minute fields in the chosen clock. */
const hm = (): Intl.DateTimeFormatOptions => (hour12() ? { hour: "numeric", minute: "2-digit", hour12: true } : { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

/** "14:05" or "2:05 PM". */
export const clockTime = (ms: number) => fmt("time", hm()).format(ms);

/** A wall-clock time with no zone, such as a cron time on the Host: "09:00" or "9:00 AM". */
export const wallClock = (hour: number, minute: number) => fmt("time", hm(), true).format(Date.UTC(2000, 0, 1, hour, minute));

/** "Nov 17". */
export const shortDate = (ms: number) => fmt("date", { month: "short", day: "numeric" }).format(ms);

/** "Nov 17, 2026". */
export const fullDate = (ms: number) => fmt("full", { year: "numeric", month: "short", day: "numeric" }).format(ms);

/** "Wed 14:05". */
export const weekdayTime = (ms: number) => `${fmt("weekday", { weekday: "short" }).format(ms)} ${clockTime(ms)}`;

/** "Nov 17, 14:05". */
export const dateTime = (ms: number) => fmt("datetime", { month: "short", day: "numeric", ...hm() }).format(ms);

/** "Nov 17, 2026, 14:05:09": a tooltip's full stamp. */
export const fullDateTime = (ms: number) => fmt("fulltime", { year: "numeric", month: "short", day: "numeric", ...hm(), second: "2-digit" }).format(ms);

/** The calendar day of `ms` in the shown zone, as days since 1970. */
function dayNumber(ms: number): number {
  const parts = fmt("day", { year: "numeric", month: "numeric", day: "numeric" });
  const p = Object.fromEntries(parts.formatToParts(ms).map((x) => [x.type, x.value]));
  // formatToParts gives Latin digits for these options in every catalog language.
  return Math.round(Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day)) / 86_400_000);
}

/** Days from `ms` to `now`, by the calendar in the shown zone: 0 today, 1 yesterday, -1 tomorrow. */
export const daysAgo = (ms: number, now = Date.now()) => dayNumber(now) - dayNumber(ms);

/** Today "14:05", else "Nov 17": a compact list cell. */
export const compactWhen = (ms: number, now = Date.now()) => (daysAgo(ms, now) === 0 ? clockTime(ms) : shortDate(ms));

/** Today "14:05", yesterday "Yesterday 14:05", else "Nov 17, 14:05". */
export function recentWhen(ms: number, now = Date.now()): string {
  const days = daysAgo(ms, now);
  if (days === 0) return clockTime(ms);
  if (days === 1) return t("time.yesterday", { time: clockTime(ms) });
  return dateTime(ms);
}

/** A time ahead: today "14:05", this week "Wed 14:05", else "Nov 17". */
export function aheadWhen(ms: number, now = Date.now()): string {
  const days = -daysAgo(ms, now);
  if (days === 0) return clockTime(ms);
  if (days > 0 && days < 7) return weekdayTime(ms);
  return shortDate(ms);
}

/** Unix ms of a UTC stamp such as `2026-11-11T01:30:00Z`. NaN for any other text. */
export const parseUtc = (text: string | null | undefined) => (text ? Date.parse(text) : NaN);

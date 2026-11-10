/**
 * The When sentence of Settings › Schedules (#262): the shapes a schedule
 * takes and their cron line. Nothing here draws. The same shapes are in
 * `crates/skiff-client/src/schedule/when.rs`; keep the two in step. A cron
 * line no shape shows is Custom.
 */

/** Weekdays as a bit set on cron numbers: bit 0 is Sunday. */
export const ALL_DAYS = 0x7f;
export const MON_FRI = 0b0011_1110;
/** Monday first, with the cron number. */
export const WEEK: [number, string][] = [[1, "Mon"], [2, "Tue"], [3, "Wed"], [4, "Thu"], [5, "Fri"], [6, "Sat"], [0, "Sun"]];
/** The interval values in minutes: each divides the hour or the day evenly. */
export const MINUTES = [5, 10, 15, 20, 30];
export const HOURS = [1, 2, 3, 4, 6, 8, 12];

export type At = [number, number];

export type When =
  | { kind: "day"; at: At }
  | { kind: "weekday"; at: At }
  | { kind: "week"; days: number; at: At }
  | { kind: "month"; day: number; at: At }
  /** Every `minutes`, from hour `window[0]` to `window[1]` (no run starts at the end), on `days`. */
  | { kind: "interval"; minutes: number; window: [number, number] | null; days: number }
  | { kind: "custom"; cron: string };

const num = (s: string, hi: number): number | null => (/^\d{1,2}$/.test(s) && Number(s) <= hi ? Number(s) : null);

function daysOf(field: string): number | null {
  if (field === "*") return ALL_DAYS;
  let set = 0;
  for (const part of field.split(",")) {
    const [a, b = a] = part.split("-");
    const x = num(a, 7);
    const y = num(b, 7);
    if (x === null || y === null || x > y) return null;
    for (let d = x; d <= y; d++) set |= 1 << d % 7;
  }
  return set;
}

function daysField(days: number): string {
  if (days === ALL_DAYS) return "*";
  if (days === MON_FRI) return "1-5";
  return WEEK.filter(([n]) => days & (1 << n)).map(([n]) => n).join(",");
}

/** The shape `cron` shows, else Custom. */
export function fromCron(cron: string): When {
  const text = cron.trim().split(/\s+/).join(" ");
  const custom: When = { kind: "custom", cron: text };
  const f = text.split(" ");
  if (f.length !== 5 || f[3] !== "*") return custom;
  const m = num(f[0], 59);
  const h = num(f[1], 23);
  if (m !== null && h !== null) {
    const at: At = [h, m];
    if (f[2] === "*") {
      const days = daysOf(f[4]);
      if (days === ALL_DAYS) return { kind: "day", at };
      if (days === MON_FRI) return { kind: "weekday", at };
      return days ? { kind: "week", days, at } : custom;
    }
    const day = num(f[2], 28);
    return day && f[4] === "*" ? { kind: "month", day, at } : custom;
  }
  if (f[2] !== "*") return custom;
  const days = daysOf(f[4]);
  if (!days) return custom;
  const hours = (field: string): { step: number; window: [number, number] | null } | null => {
    const [range, s] = field.split("/");
    const step = s === undefined ? 1 : num(s, 23);
    if (!step) return null;
    if (range === "*") return { step, window: null };
    const [a, b] = range.split("-");
    const x = num(a, 23);
    const y = b === undefined ? x : num(b, 23);
    return x !== null && y !== null && x <= y ? { step, window: [x, y + 1] } : null;
  };
  const step = /^\*\/(\d+)$/.exec(f[0]);
  if (step) {
    const hs = hours(f[1]);
    const minutes = Number(step[1]);
    return hs && hs.step === 1 && MINUTES.includes(minutes) ? { kind: "interval", minutes, window: hs.window, days } : custom;
  }
  if (f[0] === "0") {
    const hs = hours(f[1]);
    if (hs && HOURS.includes(hs.step)) return { kind: "interval", minutes: hs.step * 60, window: hs.window, days };
  }
  return custom;
}

export function toCron(w: When): string {
  const ma = (at: At) => `${at[1]} ${at[0]}`;
  switch (w.kind) {
    case "day":
      return `${ma(w.at)} * * *`;
    case "weekday":
      return `${ma(w.at)} * * 1-5`;
    case "week":
      return `${ma(w.at)} * * ${daysField(w.days)}`;
    case "month":
      return `${ma(w.at)} ${w.day} * *`;
    case "interval": {
      const d = daysField(w.days);
      const hours = w.window ? (w.window[1] - w.window[0] > 1 ? `${w.window[0]}-${w.window[1] - 1}` : `${w.window[0]}`) : "*";
      if (w.minutes < 60) return `*/${w.minutes} ${hours} * * ${d}`;
      const step = w.minutes / 60;
      if (step === 1) return `0 ${hours} * * ${d}`;
      return w.window ? `0 ${hours}/${step} * * ${d}` : `0 */${step} * * ${d}`;
    }
    case "custom":
      return w.cron;
  }
}

/** `08:00`. */
export const hhmm = (at: At) => `${String(at[0]).padStart(2, "0")}:${String(at[1]).padStart(2, "0")}`;

/** `HH:MM`, 00:00 to 23:59, else null. */
export function parseTime(text: string): At | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h <= 23 && min <= 59 ? [h, min] : null;
}

/** `Mon–Fri`, `Mon, Thu`, `Mon–Sun`. */
export function daysWords(days: number): string {
  if (days === ALL_DAYS) return "Mon–Sun";
  if (days === MON_FRI) return "Mon–Fri";
  return WEEK.filter(([n]) => days & (1 << n)).map(([, s]) => s).join(", ");
}

/** The first picker's value: `day`, `weekday`, `week`, `month`, `30m`, `2h` or `custom`. */
export function every(w: When): string {
  if (w.kind === "interval") return w.minutes < 60 ? `${w.minutes}m` : `${w.minutes / 60}h`;
  return w.kind;
}

/** The time a shape keeps when the first picker changes, else 09:00. */
const atOf = (w: When): At => ("at" in w ? w.at : [9, 0]);

/** The shape for a new first-picker value, keeping what carries over. */
export function withEvery(w: When, value: string): When {
  const at = atOf(w);
  const days = w.kind === "interval" || w.kind === "week" ? w.days : ALL_DAYS;
  if (value === "day") return { kind: "day", at };
  if (value === "weekday") return { kind: "weekday", at };
  if (value === "week") return { kind: "week", days: days === ALL_DAYS ? 1 << 1 : days, at };
  if (value === "month") return { kind: "month", day: 1, at };
  if (value === "custom") return { kind: "custom", cron: toCron(w) };
  const n = Number(value.slice(0, -1));
  const minutes = value.endsWith("h") ? n * 60 : n;
  return { kind: "interval", minutes, window: w.kind === "interval" ? w.window : null, days };
}

/** A week of Mon to Fri is a weekday schedule; every day is a day schedule. */
export function weekOf(days: number, at: At): When {
  if (days === MON_FRI) return { kind: "weekday", at };
  if (days === ALL_DAYS) return { kind: "day", at };
  return { kind: "week", days, at };
}

import { invoke } from "../platform/invoke";
import { contrast, isLight, mix, readable, rgb } from "./colors";
import type { TerminalTheme } from "../platform/types";

/** The surfaces text and marks sit on: bg, panel, raised. Stylesheet defaults until a theme applies. */
let surfaces = ["#0f1216", "#13171c", "#1b2129"];
let lightScheme = false;

/**
 * `c` pushed away from the surfaces (toward black on a light theme, white on
 * a dark one) until it reaches `ratio` on every one of them. A color that
 * already passes comes back unchanged.
 */
function legible(c: string, ratio: number, on = surfaces): string {
  if (!/^#[0-9a-f]{6}/i.test(c)) return c;
  return readable(c, on, ratio);
}

/**
 * A mark color (icon, accent bar, project name) made readable on the current
 * app theme: at least 3:1 on its surfaces. Colors picked for dark themes turn
 * darker on a light one.
 */
export const ink = (c: string): string => {
  let v = inked.get(c);
  if (v === undefined) inked.set(c, (v = legible(c, 3)));
  return v;
};

/** `ink` per color, for the surfaces of the moment. Rows ask for the same few colors on every render. */
const inked = new Map<string, string>();

/** Minimum contrast per text tier and per mark, on every surface. */
const TEXT_MIN: Record<string, number> = {
  "--text": 7, "--text-2": 5.5, "--text-3": 4.5, "--text-dim": 3.2,
  "--state-waiting-text": 4.5, "--state-done": 4.5, "--state-idle": 3.2, "--error": 4.5,
  "--state-working": 3, "--state-waiting": 3, "--project-default": 3, "--accent": 3,
};

/** The CSS variables the app theme sets. The stylesheet provides the loading fallback. */
const VARS = [
  "--bg", "--bg-deep", "--bg-panel", "--bg-raised", "--rule", "--rule-strong",
  "--text", "--text-2", "--text-3", "--text-dim",
  "--state-working", "--state-waiting", "--state-waiting-text", "--state-waiting-bg", "--state-waiting-rule",
  "--state-done", "--state-idle", "--error", "--project-default", "--accent",
  "--hover", "--press", "--gloss", "--shade",
  "--error-bg", "--error-bg-hover", "--error-rule", "--error-text",
  "--danger", "--danger-hover", "--danger-text", "--ok", "--scrim", "--on-working", "--watch", "--rank",
  "--diff-add", "--diff-del",
];

/**
 * App colors from a terminal theme: surfaces step from the background toward
 * the foreground, text steps back, and the states take the theme's blue,
 * yellow, and red. `null` returns to the stylesheet defaults.
 */
/** The app background for the next start screen, so the first paint has this run's color. */
function keepStartBg(bg: string | undefined) {
  try {
    if (bg) localStorage.setItem("skiff.startBg", bg);
    else localStorage.removeItem("skiff.startBg");
  } catch {
    /* the next start paints Harbor's */
  }
}

/** The theme the app wears, so a skin change can apply it again. */
let worn: TerminalTheme | null = null;
/** True while the Bubble frame wears a light frame made from a dark theme. */
let lit = false;

/** Whether the app chrome wears a light frame made from a dark theme (Bubble only). Pane heads then take the real theme. */
export const litFrame = () => lit;

/** The app theme again, after a skin change. */
export function reapplyAppTheme() {
  if (worn) applyAppTheme(worn);
}

export function applyAppTheme(t: TerminalTheme | null) {
  const root = document.documentElement.style;
  worn = t;
  lit = !!t && document.documentElement.dataset.skin === "bubble" && !isLight(t.ui?.background ?? t.background);
  if (lit) t = lightFrame(t!);
  if (!t) {
    keepStartBg(undefined);
    for (const v of VARS) root.removeProperty(v);
    root.removeProperty("color-scheme");
    surfaces = ["#0f1216", "#13171c", "#1b2129"];
    lightScheme = false;
    inked.clear();
    void invoke("set_window_dark", { dark: true }).catch(() => {});
    return;
  }
  const set = themeVars(t);
  keepStartBg(set["--bg"]);
  inked.clear();
  for (const [k, v] of Object.entries(set)) root.setProperty(k, v);
  root.setProperty("color-scheme", lightScheme ? "light" : "dark");
  // The GNOME header bar and its menu follow the app theme.
  void invoke("set_window_dark", { dark: !lightScheme }).catch(() => {});
}

/**
 * The app colors a theme gives, without applying them: for a part of the
 * window in another project's theme, as mission control's lanes are.
 * `color-scheme` is in the set too.
 */
export function appThemeVars(t: TerminalTheme): Record<string, string> {
  const keep = [surfaces, lightScheme] as const;
  const set = themeVars(t);
  set["color-scheme"] = lightScheme ? "light" : "dark";
  [surfaces, lightScheme] = keep;
  inked.clear();
  return set;
}

/** The theme's app colors. Sets the surfaces the contrast checks use. */
function themeVars(t: TerminalTheme): Record<string, string> {
  const bg = t.background;
  const fg = t.foreground;
  const light = isLight(bg);
  const [, red, , yellow, blue, magenta] = t.palette;
  const set: Record<string, string> = t.ui ? fromUi(t.ui, t) : {
    "--bg": bg,
    "--bg-deep": light ? mix(bg, fg, 0.04) : mix(bg, "#000000", 0.25),
    "--bg-panel": mix(bg, fg, 0.05),
    "--bg-raised": mix(bg, fg, 0.1),
    "--rule": mix(bg, fg, 0.14),
    "--rule-strong": mix(bg, fg, 0.22),
    "--text": fg,
    "--text-2": mix(fg, bg, 0.15),
    "--text-3": mix(fg, bg, 0.35),
    "--text-dim": mix(fg, bg, 0.5),
    "--state-working": blue,
    "--state-waiting": yellow,
    "--state-waiting-text": mix(yellow, fg, 0.3),
    "--state-waiting-bg": mix(bg, yellow, 0.12),
    "--state-waiting-rule": mix(bg, yellow, 0.4),
    "--state-done": mix(fg, bg, 0.35),
    "--state-idle": mix(fg, bg, 0.5),
    "--error": red,
    "--project-default": magenta,
  };
  set["--accent"] = set["--project-default"];
  // Enforce contrast last, so theme files and derived mixes both pass.
  surfaces = [set["--bg"], set["--bg-panel"], set["--bg-raised"]];
  lightScheme = isLight(set["--bg"]);
  for (const [k, min] of Object.entries(TEXT_MIN)) set[k] = legible(set[k], min);
  Object.assign(set, alerts(set, t.palette[2]));
  // The watcher's teal takes the theme's cyan.
  set["--watch"] = legible(t.palette[6], 3);
  // The Skiff agent's rank stripes: the theme's yellow, toward the text, off the waiting amber.
  set["--rank"] = legible(mix(t.palette[3], set["--text"], 0.35), 3);
  // The stylesheet's pale diff colors read only on a dark ground.
  set["--diff-add"] = lightScheme ? legible(t.palette[2], 4.5) : "#7fd08a";
  set["--diff-del"] = lightScheme ? legible(t.palette[1], 4.5) : "#ff8a80";
  set["--hover"] = lightScheme ? "rgba(0, 0, 0, 0.05)" : "rgba(255, 255, 255, 0.05)";
  set["--press"] = lightScheme ? "rgba(0, 0, 0, 0.09)" : "rgba(255, 255, 255, 0.09)";
  // A skin's highlight and shade: the ground toward white on a light theme, the text on a dark one.
  set["--gloss"] = lightScheme ? mix(set["--bg"], "#ffffff", 0.6) + "e0" : set["--text"] + "26";
  set["--shade"] = lightScheme ? set["--text"] + "29" : "#00000059";
  return set;
}

/** Hue (0-360), saturation and lightness (0-1) of a hex color. */
function hsl(hex: string): [number, number, number] {
  const [r, g, b] = rgb(hex).map((v) => v / 255);
  const hi = Math.max(r, g, b), lo = Math.min(r, g, b), l = (hi + lo) / 2, d = hi - lo;
  if (!d) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  const h = hi === r ? ((g - b) / d) % 6 : hi === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [(h * 60 + 360) % 360, s, l];
}

/** A hex color from hue, saturation and lightness. */
function hex(h: number, s: number, l: number): string {
  const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return "#" + [r, g, b].map((v) => Math.round((v + m) * 255).toString(16).padStart(2, "0")).join("");
}

/**
 * Bubble's frame for a dark theme: a light ground tinted with the theme's own
 * color (its main color, else its foreground), white panels and dark text, on
 * the steps of the light built-in themes (Peppermint's frame is about 88%
 * light, its text 15%). The palette stays: the contrast checks push its state
 * colors to read on the light surfaces. Terminals keep the real theme.
 */
function lightFrame(t: TerminalTheme): TerminalTheme {
  const [h, s0] = hsl(t.main_color ?? t.foreground);
  const s = Math.min(s0, 0.65);
  const tone = (sat: number, l: number) => hex(h, s * sat, l);
  const bg = "#ffffff";
  const fg = tone(1, 0.15);
  return {
    ...t,
    background: bg,
    foreground: fg,
    ui: {
      background: bg,
      sidebar: tone(1, 0.88),
      tertiary: tone(0.9, 0.82),
      secondary: tone(1, 0.95),
      border: tone(0.7, 0.72),
      mutedForeground: tone(0.6, 0.38),
      sidebarForeground: tone(1, 0.2),
      primary: t.ui?.primary ?? t.main_color ?? t.palette[5],
      working: t.ui?.working ?? t.palette[4],
      warning: t.palette[3],
      destructive: t.palette[1],
    },
  };
}

/** Error banners, the destructive button, the ok dot, and the modal scrim, from the theme's red and green. */
function alerts(v: Record<string, string>, green: string): Record<string, string> {
  const bg = v["--bg"];
  const red = v["--error"];
  const errorBg = mix(bg, red, lightScheme ? 0.1 : 0.14);
  const danger = lightScheme ? mix(red, "#000000", 0.15) : mix(bg, red, 0.45);
  const dangerText = lightScheme ? "#ffffff" : mix("#ffffff", red, 0.12);
  const working = v["--state-working"];
  const onWorking = [v["--bg-deep"], "#ffffff", "#000000"].reduce((a, b) => (contrast(b, working) > contrast(a, working) ? b : a));
  return {
    "--error-bg": errorBg,
    "--error-bg-hover": mix(bg, red, lightScheme ? 0.16 : 0.2),
    "--error-rule": mix(bg, red, 0.4),
    "--error-text": legible(mix(red, v["--text"], 0.2), 4.5, [errorBg, ...surfaces]),
    "--danger": danger,
    "--danger-hover": mix(danger, lightScheme ? "#000000" : "#ffffff", 0.1),
    "--danger-text": dangerText,
    "--ok": legible(green, 3),
    "--scrim": lightScheme ? "rgba(0, 0, 0, 0.32)" : "rgba(5, 7, 10, 0.72)",
    "--on-working": onWorking,
  };
}

/**
 * A theme file's own app colors (Superset's ui block, shadcn names). Any
 * missing key falls back to a mix of background and foreground. `primary` is
 * the accent. `working`, Skiff's own key, keeps the working state off it.
 */
function fromUi(u: Record<string, string>, t: TerminalTheme): Record<string, string> {
  const bg = u.background ?? t.background;
  const fg = u.foreground ?? t.foreground;
  const muted = u.mutedForeground ?? mix(fg, bg, 0.45);
  const warning = u.warning ?? t.palette[3];
  const border = u.border ?? mix(bg, fg, 0.14);
  return {
    "--bg": bg,
    "--bg-deep": u.tertiary ?? u.sidebar ?? (isLight(bg) ? mix(bg, fg, 0.04) : mix(bg, "#000000", 0.25)),
    "--bg-panel": u.sidebar ?? u.card ?? mix(bg, fg, 0.05),
    "--bg-raised": u.secondary ?? u.muted ?? mix(bg, fg, 0.1),
    "--rule": border,
    "--rule-strong": mix(border, fg, 0.12),
    "--text": fg,
    "--text-2": u.sidebarForeground ?? mix(fg, bg, 0.15),
    "--text-3": mix(fg, muted, 0.5),
    "--text-dim": muted,
    "--state-working": u.working ?? u.primary ?? t.palette[4],
    "--state-waiting": warning,
    "--state-waiting-text": mix(warning, fg, 0.3),
    "--state-waiting-bg": mix(bg, warning, 0.12),
    "--state-waiting-rule": mix(bg, warning, 0.4),
    "--state-done": mix(fg, muted, 0.5),
    "--state-idle": muted,
    "--error": u.destructive ?? t.palette[1],
    "--project-default": u.primary ?? t.palette[5],
  };
}

import type { TerminalTheme } from "./types";

type Rgb = [number, number, number];

const rgb = (hex: string): Rgb => {
  const n = parseInt(hex.replace("#", "").slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const toHex = (c: Rgb) => "#" + c.map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0")).join("");
/** `a` moved `t` of the way to `b`. */
const mix = (a: string, b: string, t: number) => {
  const x = rgb(a);
  const y = rgb(b);
  return toHex([0, 1, 2].map((i) => x[i] + (y[i] - x[i]) * t) as Rgb);
};
const luminance = (hex: string) => {
  const [r, g, b] = rgb(hex);
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
};

/** The CSS variables the app theme sets. Harbor is the stylesheet itself. */
const VARS = [
  "--bg", "--bg-deep", "--bg-panel", "--bg-raised", "--rule", "--rule-strong",
  "--text", "--text-2", "--text-3", "--text-dim",
  "--state-working", "--state-waiting", "--state-waiting-text", "--state-waiting-bg", "--state-waiting-rule",
  "--state-done", "--state-idle", "--error", "--project-default",
];

/**
 * App colors from a terminal theme: surfaces step from the background toward
 * the foreground, text steps back, and the states take the theme's blue,
 * yellow, and red. `null` returns to the stylesheet's Harbor.
 */
export function applyAppTheme(t: TerminalTheme | null) {
  const root = document.documentElement.style;
  if (!t) {
    for (const v of VARS) root.removeProperty(v);
    root.removeProperty("color-scheme");
    return;
  }
  const bg = t.background;
  const fg = t.foreground;
  const light = luminance(bg) > 0.5;
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
  for (const [k, v] of Object.entries(set)) root.setProperty(k, v);
  root.setProperty("color-scheme", light ? "light" : "dark");
}

/**
 * A theme file's own app colors (Superset's ui block, shadcn names). Any
 * missing key falls back to a mix of background and foreground.
 */
function fromUi(u: Record<string, string>, t: TerminalTheme): Record<string, string> {
  const bg = u.background ?? t.background;
  const fg = u.foreground ?? t.foreground;
  const muted = u.mutedForeground ?? mix(fg, bg, 0.45);
  const warning = u.warning ?? t.palette[3];
  const border = u.border ?? mix(bg, fg, 0.14);
  return {
    "--bg": bg,
    "--bg-deep": u.tertiary ?? u.sidebar ?? mix(bg, "#000000", 0.25),
    "--bg-panel": u.sidebar ?? u.card ?? mix(bg, fg, 0.05),
    "--bg-raised": u.secondary ?? u.muted ?? mix(bg, fg, 0.1),
    "--rule": border,
    "--rule-strong": mix(border, fg, 0.12),
    "--text": fg,
    "--text-2": u.sidebarForeground ?? mix(fg, bg, 0.15),
    "--text-3": mix(fg, muted, 0.5),
    "--text-dim": muted,
    "--state-working": u.primary ?? t.palette[4],
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

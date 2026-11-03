/**
 * A Mission control window wears the theme of the pane it stands for: the
 * session theme, else the project theme, else the app theme, as `themeIdFor`
 * picks it for the pane. Its body takes the terminal's background and text,
 * as `applyThemes` gives the pane. The state column takes the app colors that
 * theme gives (`appThemeVars`: the theme file's own, else from its palette,
 * pushed to read on its own background), so its state words, task and counts
 * read on every theme. The header, the strings and the session cards keep the
 * board's colors.
 *
 * The render path repaints the windows (`repaintWindows`, a step after the
 * panes' themes), so a theme pick shows on the board when it shows in the pane.
 */
import type { SessionInfo } from "../../platform/types";
import { S, sessions } from "../../app/state";
import { appThemeVars } from "../../appearance/appTheme";
import { DEFAULT_THEME } from "../../appearance/colors";
import { themeIdFor, xtermTheme } from "../../appearance/themes";

/** The app colors the window's body uses, from its theme. */
const KEYS = ["--rule", "--text-2", "--text-3", "--text-dim", "--state-working", "--state-waiting", "--state-waiting-text", "--state-done", "--state-idle", "color-scheme"];

/** Each window's body, by session, while the board shows it. */
const bodies = new Map<string, HTMLElement>();
/** Each theme's colors, by id and the colors it was made from: a theme file edit makes new ones. */
const made = new Map<string, Record<string, string>>();

/** Paints `body` in the theme of `s`'s pane. A body already in that theme stays. */
export function paintWindow(body: HTMLElement, s: SessionInfo) {
  bodies.set(s.id, body);
  const id = themeIdFor(s);
  const t = S.themes.find((x) => x.id === id) ?? S.themes.find((x) => x.id === DEFAULT_THEME);
  if (!t) return;
  const key = `${t.id} ${t.background} ${t.foreground} ${t.palette.join(",")} ${JSON.stringify(t.ui ?? null)}`;
  if (body.dataset.theme === key) return;
  body.dataset.theme = key;
  let v = made.get(key);
  if (!v) made.set(key, (v = appThemeVars(t)));
  body.style.background = t.background;
  body.style.color = xtermTheme(t).foreground;
  for (const k of KEYS) body.style.setProperty(k, v[k] ?? "");
}

/** The render path: every window on the board in its pane's theme again. */
export function repaintWindows() {
  for (const [id, body] of bodies) {
    const s = sessions.get(id);
    if (s && body.isConnected) paintWindow(body, s);
    else bodies.delete(id);
  }
}

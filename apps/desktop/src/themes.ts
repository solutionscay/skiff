/** Terminal and app themes. */
import { applyAppTheme } from "./appTheme";
import { sessionsOf } from "./layout";
import { taskTitle } from "./model";
import { pickTheme } from "./themeCards";
import type { Group, SessionInfo, TerminalTheme } from "./types";
import { invoke } from "@tauri-apps/api/core";
import { showError } from "./dom";
import { render } from "./render";
import { panes, S, sessions } from "./state";

const DEFAULT_THEME = "builtin:harbor";

/** Foot themes became built-ins; old `foot:x` choices mean `builtin:x`. */
const canonTheme = (id: string | null) => (id ? id.replace(/^foot:/, "builtin:") : id);

export async function loadThemes() {
  const list = await invoke<TerminalTheme[]>("list_themes").catch(() => S.themes);
  // An older daemon still lists the Foot themes as foot:<slug>.
  S.themes = list.map((t) => ({ ...t, id: canonTheme(t.id)!, source: t.source === "file" ? "file" : "built-in" }));
}

/** A project with no theme of its own uses the app theme; Harbor if the app follows projects. */
function defaultTerminalTheme(): string {
  return canonTheme(S.appTheme) ?? DEFAULT_THEME;
}

/** A terminal's theme: its own, else the app's. */
export function themeIdFor(s: SessionInfo | undefined): string {
  return (s && canonTheme(s.theme)) || defaultTerminalTheme();
}

let appliedApp: string | undefined;

/** The app's own colors: sidebar, top bar, panels. */
export function applyApp() {
  const want = canonTheme(S.appTheme) ?? DEFAULT_THEME;
  if (want === appliedApp) return;
  appliedApp = want;
  applyAppTheme(want === DEFAULT_THEME ? null : S.themes.find((t) => t.id === want) ?? null);
}

function xtermTheme(t: TerminalTheme) {
  const [black, red, green, yellow, blue, magenta, cyan, white, brightBlack, brightRed, brightGreen, brightYellow, brightBlue, brightMagenta, brightCyan, brightWhite] = t.palette;
  return {
    foreground: t.foreground,
    background: t.background,
    cursor: t.cursor ?? t.foreground,
    selectionBackground: t.selection ?? brightBlack,
    black, red, green, yellow, blue, magenta, cyan, white,
    brightBlack, brightRed, brightGreen, brightYellow, brightBlue, brightMagenta, brightCyan, brightWhite,
  };
}

/** Give every open terminal its project's theme. Only changed panes repaint. */
export function applyThemes() {
  for (const [id, pane] of panes) {
    const want = themeIdFor(sessions.get(id));
    if (pane.theme === want) continue;
    const t = S.themes.find((x) => x.id === want) ?? S.themes.find((x) => x.id === DEFAULT_THEME);
    if (!t) continue;
    pane.theme = want;
    pane.term.options.theme = xtermTheme(t);
    pane.el.style.background = t.background;
  }
}

/** Right-click a terminal, Terminal theme: this terminal only. */
export async function sessionThemeMenu(s: SessionInfo) {
  await loadThemes();
  const app = S.themes.find((t) => t.id === defaultTerminalTheme());
  pickTheme(`Terminal theme: ${taskTitle(s)}`, S.themes, {
    active: canonTheme(s.theme),
    none: { label: "Same as app", theme: app },
    pick: (id) => {
      const live = sessions.get(s.id);
      if (!live) return;
      live.theme = id;
      applyThemes();
      applyApp();
      invoke<SessionInfo>("set_session_theme", { session: s.id, theme: id }).catch(showError);
    },
  });
}

/** Right-click a group, Terminal theme: every terminal in it. */
export async function groupThemeMenu(g: Group) {
  await loadThemes();
  const ids = sessionsOf(g.layout);
  const members = ids.map((id) => sessions.get(id)).filter((s): s is SessionInfo => !!s);
  const first = canonTheme(members[0]?.theme ?? null);
  const shared = members.every((s) => canonTheme(s.theme) === first) ? first : null;
  const app = S.themes.find((t) => t.id === defaultTerminalTheme());
  pickTheme(`Terminal theme: ${g.name}`, S.themes, {
    active: shared,
    none: { label: "Same as app", theme: app },
    pick: (id) => {
      // Look sessions up now: one may have ended while the picker was open.
      for (const sid of ids) {
        const s = sessions.get(sid);
        if (!s) continue;
        s.theme = id;
        invoke<SessionInfo>("set_session_theme", { session: sid, theme: id }).catch(showError);
      }
      applyThemes();
      applyApp();
      render();
    },
  });
}

/** The theme every terminal in the group was given by hand, or null when they differ or none was set. */
export function groupTheme(g: Group): TerminalTheme | null {
  const ids = sessionsOf(g.layout).map((id) => canonTheme(sessions.get(id)?.theme ?? null));
  const first = ids[0];
  if (!first || !ids.every((id) => id === first)) return null;
  return S.themes.find((t) => t.id === first) ?? null;
}

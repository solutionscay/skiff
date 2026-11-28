/**
 * A pane head in the pane's own theme: the session theme, else the project
 * theme. The cell gets that theme's frame, text, rule, state and gloss colors
 * as `--head-*`, from `appThemeVars`, as mission control paints its windows.
 * A pane that follows the app theme gets none. Only the Bubble skin reads
 * them (`styles/tokens.css`); Default heads keep the app colors.
 */
import type { SessionInfo } from "../platform/types";
import { S } from "../app/state";
import { appThemeVars } from "../appearance/appTheme";
import { ownThemeId } from "../appearance/themes";

/** App variable, then the cell variable that carries it. */
const KEYS: [string, string][] = [
  ["--bg-deep", "--head-bg"], ["--text", "--head-text"], ["--text-2", "--head-text-2"], ["--text-3", "--head-text-3"],
  ["--text-dim", "--head-text-dim"], ["--rule", "--head-rule"], ["--state-working", "--head-working"], ["--state-done", "--head-done"],
  ["--gloss", "--head-gloss"], ["--shade", "--head-shade"], ["--hover", "--head-hover"],
];

/** Each theme's colors, by id and the colors it was made from: a theme file edit makes new ones. */
const made = new Map<string, Record<string, string>>();

/** Gives `cell` the head colors of `s`'s own theme, or takes them away when it follows the app. */
export function paintHead(cell: HTMLElement, s: SessionInfo | undefined) {
  const id = s ? ownThemeId(s) : null;
  const t = id ? S.themes.find((x) => x.id === id) : undefined;
  const key = t ? `${t.id} ${t.background} ${t.foreground} ${t.palette.join(",")} ${JSON.stringify(t.ui ?? null)}` : "";
  if ((cell.dataset.head ?? "") === key) return;
  if (!t) {
    delete cell.dataset.head;
    for (const [, k] of KEYS) cell.style.removeProperty(k);
    return;
  }
  cell.dataset.head = key;
  let v = made.get(key);
  if (!v) made.set(key, (v = appThemeVars(t)));
  for (const [from, to] of KEYS) cell.style.setProperty(to, v[from] ?? "");
}

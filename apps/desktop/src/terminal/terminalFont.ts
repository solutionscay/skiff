import { invoke } from "@tauri-apps/api/core";

import { h } from "../ui/dom";
import { host } from "./terminalHost";

import { FONT_DEFAULT, S } from "../app/state";

import { panes } from "./terminalState";

import { fitShown } from "./terminal";

let saveFontSize: ReturnType<typeof setTimeout> | undefined;

/**
 * The daemon's `[appearance]` keeps the size across machines and cleared
 * caches. localStorage is the fallback for a daemon older than the setting.
 * `save: false` applies a size the daemon already has.
 */
export function setFontSize(n: number, save = true) {
  S.fontSize = Math.min(28, Math.max(8, n));
  try {
    localStorage.setItem("skiff.fontSize", String(S.fontSize));
  } catch {
    /* a private window keeps the size for this run only */
  }
  for (const p of panes.values()) p.term.options.fontSize = S.fontSize;
  applyZoom();
  fitShown();
  if (!save) return;
  showZoom();
  // A held key steps the size many times; the config file gets the last one.
  clearTimeout(saveFontSize);
  saveFontSize = setTimeout(() => invoke("set_font_size", { size: S.fontSize }).catch(console.error), 300);
}

let zoomTimer: ReturnType<typeof setTimeout> | undefined;

/** Ctrl+Plus/Minus: the size as a percent of the default, briefly, over the terminals. */
function showZoom() {
  const badge = host.querySelector<HTMLElement>(".zoom-badge") ?? host.appendChild(h("div", "zoom-badge"));
  badge.setAttribute("role", "status");
  badge.textContent = `${Math.round((S.fontSize / FONT_DEFAULT) * 100)}%`;
  clearTimeout(zoomTimer);
  zoomTimer = setTimeout(() => badge.remove(), 1200);
}

/** The app text follows the terminal size: 13 is 1x. */
export function applyZoom() {
  document.documentElement.style.setProperty("--zoom", String(S.fontSize / FONT_DEFAULT));
}

import "@xterm/xterm/css/xterm.css";
import "@fontsource/space-grotesk/latin-700.css";
import "./styles.css";

import { keyLabel, setKeymap } from "./keys";
import { bySessionPriority } from "./model";
import type { DaemonEvent, DaemonStatus } from "./types";
import { Channel, invoke } from "@tauri-apps/api/core";
import { switcher } from "./commands";
import { loadProjects, onEvent, refreshSessions, setDaemon } from "./daemon";
import { $, showError } from "./dom";
import { loadGroups } from "./groups";
import { settings } from "./panels";
import { render, scheduleRender } from "./render";
import { groupOf, S, sessions } from "./state";
import { loadThemes } from "./themes";
import { focusNextWaiting, showGroup, showSingle } from "./view";

// Skiff owns right-click. The webview's own menu (Back, Reload, Inspect)
// never shows; text fields keep theirs for cut, copy and paste.
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  const field = t?.closest("input, textarea:not(.xterm-helper-textarea), [contenteditable=true]");
  if (!field) e.preventDefault();
});

async function boot() {
  $("next-waiting").addEventListener("click", focusNextWaiting);
  $("open-switcher").addEventListener("click", () => switcher.open());
  $("open-settings").addEventListener("click", () => (settings.isOpen ? settings.close() : void settings.open()));
  render();

  const status = await invoke<DaemonStatus>("daemon_status");
  setDaemon(status);
  if (!status.connected) return;

  const events = new Channel<DaemonEvent>();
  events.onmessage = onEvent;
  await invoke("subscribe_events", { onEvent: events });

  await refreshSessions();
  await Promise.all([
    loadProjects(),
    loadThemes(),
    invoke<string | null>("get_appearance").then((t) => (S.appTheme = t)).catch(() => null),
    invoke<Record<string, string>>("get_keys").then(setKeymap).catch(() => null),
  ]);
  // The top bar's search button is the palette; show its real key.
  const kbd = document.querySelector("#open-switcher kbd");
  if (kbd) kbd.textContent = keyLabel("palette").replace(/\+/g, " ");
  const hint = document.querySelector("#open-switcher .spacer");
  if (hint) hint.textContent = "Commands, sessions, worktrees";
  await loadGroups();
  const first = [...sessions.values()].sort(bySessionPriority)[0];
  const g = first && groupOf(first.id);
  if (first) g ? showGroup(g.id, false, first.id) : showSingle(first.id);

  let refreshing = false;
  setInterval(() => {
    if (refreshing) return;
    refreshing = true;
    refreshSessions()
      .catch(console.error)
      .finally(() => {
        refreshing = false;
        scheduleRender();
      });
  }, 10_000);
}

// Show the app once the first data is in. A slow daemon does not hold it past 2 s.
const reveal = () => document.body.classList.add("ready");
setTimeout(reveal, 2000);
boot().catch(showError).finally(reveal);

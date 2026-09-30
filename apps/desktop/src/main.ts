import "@xterm/xterm/css/xterm.css";
import "@fontsource/space-grotesk/latin-700.css";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-700.css";
import "./styles.css";

import { keyLabel, setKeymap } from "./keys";
import { bySessionPriority } from "./model";
import type { Appearance, DaemonEvent, DaemonStatus } from "./types";
import { Channel, invoke } from "@tauri-apps/api/core";
import { switcher } from "./commands";
import { loadProjects, onEvent, refreshSessions, setDaemon } from "./daemon";
import { $ } from "./dom";
import { showError } from "./alerts";
import { loadGroups } from "./groups";
import { initTrace } from "./latency";
import { settings } from "./panels";
import { render, scheduleRender } from "./render";
import { FONT_DEFAULT, groupOf, S, sessions } from "./state";
import { setFontSize } from "./terminal";
import { loadThemes } from "./themes";
import { loadPlace, trackPlace } from "./stored";
import { startMemory } from "./memory";
import { focusNextWaiting, selectProject, showGroup, showSingle } from "./view";

// Skiff owns right-click. The webview's own menu (Back, Reload, Inspect)
// never shows; text fields keep theirs for cut, copy and paste.
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  const field = t?.closest("input, textarea:not(.xterm-helper-textarea), [contenteditable=true]");
  if (!field) e.preventDefault();
});

async function boot() {
  // `tauri dev` build: a badge in the status bar tells it apart from an installed Skiff.
  if (import.meta.env.DEV) {
    const badge = document.createElement("span");
    badge.className = "status-item dev-badge";
    badge.textContent = "DEV";
    badge.title = "Development build (tauri dev)";
    $("statusbar").prepend(badge);
  }
  $("next-waiting").addEventListener("click", focusNextWaiting);
  $("open-switcher").addEventListener("click", () => switcher.open());
  $("open-settings").addEventListener("click", () => (settings.isOpen ? settings.close() : void settings.open()));
  render();

  void initTrace();
  startMemory();
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
    invoke<Appearance>("get_appearance")
      .then((a) => {
        S.appTheme = a.theme;
        // No size in the daemon yet: keep this machine's and hand it over.
        if (a.font_size) setFontSize(a.font_size, false);
        else if (S.fontSize !== FONT_DEFAULT) setFontSize(S.fontSize);
      })
      .catch(() => null),
    invoke<Record<string, string>>("get_keys").then(setKeymap).catch(() => null),
  ]);
  // The top bar's search button is the palette; show its real key.
  const kbd = document.querySelector("#open-switcher kbd");
  if (kbd) kbd.textContent = keyLabel("palette").replace(/\+/g, " ");
  const nwKbd = document.querySelector("#next-waiting kbd");
  if (nwKbd) nwKbd.textContent = keyLabel("next-waiting").replace(/\+/g, " ");
  const hint = document.querySelector("#open-switcher .spacer");
  if (hint) hint.textContent = "Commands, sessions, worktrees";
  await loadGroups();
  restorePlace();
  trackPlace();

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

/** Back to the project, session and group open when the app last ran. Without
 *  one, the first session by priority. */
function restorePlace() {
  const last = loadPlace();
  const s = last.session ? sessions.get(last.session) : undefined;
  const lastGroup = last.group ? S.groups.find((x) => x.id === last.group) : undefined;
  const hasProject = !!last.project && S.projects.some((p) => p.name === last.project);
  if (lastGroup && (last.picked || !s)) showGroup(lastGroup.id, !!last.picked, s?.id);
  else if (s) {
    const g = groupOf(s.id);
    if (g) showGroup(g.id, false, s.id);
    else showSingle(s.id);
  } else if (hasProject) selectProject(last.project!);
  else {
    const first = [...sessions.values()].sort(bySessionPriority)[0];
    const g = first && groupOf(first.id);
    if (first) g ? showGroup(g.id, false, first.id) : showSingle(first.id);
  }
  // The project on screen may not be the open session's.
  if (hasProject && S.selectedProject !== last.project) {
    S.selectedProject = last.project!;
    render();
  }
}

// Show the app once the first data is in. A slow daemon does not hold it past 2 s.
const reveal = () => document.body.classList.add("ready");
setTimeout(reveal, 2000);
boot().catch(showError).finally(reveal);

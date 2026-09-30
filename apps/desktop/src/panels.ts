/** Settings, the launch menu, and the Add project dialog. */
import { PRESETS, startCanvas } from "./canvas";
import { glyph } from "./layout";
import { createAddProject } from "./addProject";
import { createLaunchMenu } from "./launchMenu";
import { createSettings } from "./settings";
import { invoke } from "@tauri-apps/api/core";
import { loadProjects, newSession } from "./daemon";
import { host } from "./terminalHost";
import { showError } from "./alerts";
import { render } from "./render";
import { collapsed, enabledAgents, S, selectedWorktree } from "./state";
import { loadThemes } from "./themes";
import { refocusTerminal, selectProject } from "./view";

export const settings = createSettings(
  (a) => {
    S.agents = a;
    render();
  },
  () => refocusTerminal(),
  {
    themes: () => S.themes,
    current: () => S.appTheme,
    load: loadThemes,
    set: (id) => {
      S.appTheme = id;
      render();
      invoke("set_appearance", { theme: id }).catch(showError);
    },
  },
);

export const launchMenu = createLaunchMenu({
  agents: enabledAgents,
  start: (p, w, agent) => {
    S.selectedProject = p.name;
    selectedWorktree.set(p.name, w.path);
    collapsed.delete(w.path);
    S.justAdded = null;
    newSession(w.path, agent?.command ?? null, agent?.id ?? "", agent ? "agent" : "shell").catch(showError);
  },
  openSettings: () => void settings.open(),
  splits: () => PRESETS.map((s) => ({ name: s.name, glyph: glyph(s.make(), 18, 13) })),
  split: (p, w, i) => startCanvas(p, w, i),
  onClose: () => refocusTerminal(),
});

export const addProject = createAddProject(
  async (p) => {
    S.justAdded = p.name;
    await loadProjects();
    selectProject(p.name);
    host.querySelector<HTMLButtonElement>(".welcome button")?.focus();
  },
  () => refocusTerminal(),
);

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real themes.ts. A fleet Skipper is in no project: its pane on screen
// gives the app its session theme, else the app theme.
const theme = (id: string) => ({ id, name: id, background: "#101010", foreground: "#e0e0e0", palette: Array(16).fill("#808080") });

async function app() {
  const projects = [{ name: "skiff", path: "/skiff", fleet: "Harbor" }];
  const S: any = {
    projects, selectedProject: "skiff", focused: null, activeGroup: null, single: null,
    appTheme: "adriatic", settingsShown: false,
    themes: ["ultraviolet", "adriatic", "flamingo", "ember"].map(theme),
  };
  const sessions = new Map<string, any>([
    ["fleet", { id: "fleet", skipper: "fleet:Harbor", cwd: "/agent/fleets/Harbor", theme: "ember" }],
    ["web", { id: "web", skipper: "skiff", cwd: "/skiff" }],
  ]);
  let applied: string | null = null;
  const place = (s: any) => (s.cwd.startsWith("/skiff") ? { project: projects[0] } : null);
  const themes = moduleFromSource("../src/appearance/themes.ts", {
    "@tauri-apps/api/core": { invoke: async (cmd: string) => (cmd === "project_themes" ? [["skiff", "flamingo"]] : []) },
    "./colors": { DEFAULT_THEME: "ultraviolet", isLight: () => false, mix: (a: string) => a, readable: (c: string) => c },
    "./appTheme": { applyAppTheme: (t: any) => (applied = t?.id ?? null) },
    "../app/state": { S, sessions },
    "../app/stateQueries": { currentProject: () => projects.find((p) => p.name === S.selectedProject) ?? null, place },
    "../workspace/model": { taskTitle: (s: any) => s.id, scopeFleet: (x: string | null | undefined) => (x?.startsWith("fleet:") ? x.slice(6) : null) },
    "../app/skiffAgent": { skiffAgentSession: () => null },
    "../app/renderRequest": { render() {} },
    "../terminal/terminalState": { panes: new Map() },
  }, {}, "");
  if (themes.loadProjectThemes) await themes.loadProjectThemes();
  else themes.projectThemesFrom([{ ...projects[0], theme: "flamingo" }]);
  return { S, sessions, themes, applied: () => applied };
}

test("a fleet Skipper on screen gives the app its session theme, else the app theme", async () => {
  const a = await app();
  // A project Skipper: its project's theme.
  a.S.single = "web";
  a.themes.applyApp();
  assert.equal(a.applied(), "flamingo");
  // The fleet Skipper: its session theme, not the project selected before.
  a.S.single = "fleet";
  a.themes.applyApp();
  assert.equal(a.applied(), "ember");
  // With no session theme: the app theme.
  a.sessions.get("fleet").theme = null;
  a.S.single = "web";
  a.themes.applyApp();
  a.S.single = "fleet";
  a.themes.applyApp();
  assert.equal(a.applied(), "adriatic");
  // Settings still wears the app theme.
  a.sessions.get("fleet").theme = "ember";
  a.S.settingsShown = true;
  a.themes.applyApp();
  assert.equal(a.applied(), "adriatic");
});

test("a fleet Skipper row has a color for the current row's tint and bar", () => {
  const css = readFileSync(new URL("../src/styles/tree-project.css", import.meta.url), "utf8");
  const rule = css.match(/\.tree-kids > \.session-row\.project-skipper-row \{([^}]*)\}/);
  assert.ok(rule, "the fleet Skipper row rule");
  assert.match(rule![1], /--pc:\s*var\(--project-default\)/);
});

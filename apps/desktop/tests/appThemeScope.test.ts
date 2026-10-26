import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real themes.ts and themeCards.ts. Settings › Appearance picks the app
// theme while the selected project has a theme of its own.
class El {
  className: string;
  textContent: string;
  type = "";
  children: El[] = [];
  dataset: Record<string, string> = {};
  style = { background: "", borderRight: "", width: "", color: "", setProperty() {} };
  listeners = new Map<string, (() => void)[]>();
  classList = { toggle() {}, add() {}, contains: () => false };
  constructor(_tag: string, cls = "", text = "") {
    this.className = cls;
    this.textContent = text;
  }
  append(...kids: El[]) { this.children.push(...kids); }
  appendChild(k: El) { this.children.push(k); return k; }
  replaceChildren(...kids: El[]) { this.children = kids; }
  setAttribute() {}
  addEventListener(ev: string, run: () => void) { this.listeners.set(ev, [...(this.listeners.get(ev) ?? []), run]); }
  querySelectorAll() { return []; }
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

const theme = (id: string, name: string) => ({
  id, name, background: "#101010", foreground: "#e0e0e0",
  palette: ["#000000", "#cc3333", "#33cc33", "#cccc33", "#3366cc", "#cc33cc", "#33cccc", "#cccccc",
    "#555555", "#ff5555", "#55ff55", "#ffff55", "#5599ff", "#ff55ff", "#55ffff", "#ffffff"],
});

async function app() {
  const projects = [{ name: "skiff", path: "/skiff", theme: "flamingo" }];
  const S: any = {
    projects, selectedProject: "skiff", focused: null, activeGroup: null, single: null,
    appTheme: "adriatic", settingsShown: false,
    themes: [theme("ultraviolet", "Ultraviolet"), theme("adriatic", "Adriatic"), theme("flamingo", "Flamingo"), theme("ember", "Ember")],
  };
  const sessions = new Map<string, any>([["s1", { id: "s1", title: "fix login", cwd: "/skiff", theme: "ember" }]]);
  let applied: string | null = null;
  const themes = moduleFromSource("../src/appearance/themes.ts", {
    "@tauri-apps/api/core": { invoke: async (cmd: string) => (cmd === "project_themes" ? [["skiff", "flamingo"]] : []) },
    "./colors": { DEFAULT_THEME: "ultraviolet", isLight: () => false, mix: (a: string) => a, readable: (c: string) => c },
    "./appTheme": { applyAppTheme: (t: any) => (applied = t?.id ?? null) },
    "../app/state": { S, sessions },
    "../app/stateQueries": { currentProject: () => projects.find((p) => p.name === S.selectedProject) ?? null, place: () => ({ project: projects[0] }) },
    "../workspace/model": { taskTitle: (s: any) => s.title },
    "../app/skiffAgent": { skiffAgentSession: () => null },
    "../app/renderRequest": { render() {} },
    "../terminal/terminalState": { panes: new Map() },
  }, {}, "");
  // The project themes come from projects.toml on main and from the Host's rows on v2.
  if (themes.loadProjectThemes) await themes.loadProjectThemes();
  else themes.projectThemesFrom(projects);
  const cards = moduleFromSource("../src/appearance/themeCards.ts", {
    "./colors": { isLight: () => false, mix: (a: string) => a, readable: (c: string) => c },
    "./themeProfile": { COLOR_FAMILIES: [], themeProfile: () => ({ family: "Blue", color: "#3366cc" }) },
    "../ui/dom": { h },
    "../ui/icons": { icon: () => h("svg") },
  }, {}, "");
  return { S, themes, cards, applied: () => applied };
}

test("Settings wears the app theme: a pick there shows at once over a project theme", async () => {
  const a = await app();
  a.themes.applyApp();
  assert.equal(a.applied(), "flamingo");
  // Settings opens: the app takes the app theme.
  a.S.settingsShown = true;
  a.themes.applyApp();
  assert.equal(a.applied(), "adriatic");
  // A pick changes the app colors at once.
  a.S.appTheme = "ember";
  a.themes.applyApp();
  assert.equal(a.applied(), "ember");
  // Settings closes: the project's own theme is back.
  a.S.settingsShown = false;
  a.themes.applyApp();
  assert.equal(a.applied(), "flamingo");
});

test("the card a project or session uses names that scope", async () => {
  const a = await app();
  a.S.focused = "s1";
  const overrides = a.themes.themeOverrides();
  assert.deepEqual(JSON.parse(JSON.stringify(overrides)), [{ id: "flamingo", label: "Project: skiff" }, { id: "ember", label: "Session: fix login" }]);
  const grid: El = a.cards.themeGrid(a.S.themes, { active: "adriatic", pick() {}, scopes: overrides });
  const foot = (id: string) => grid.all().find((e) => e.dataset.theme === id)!.all().map((e) => e.textContent).filter(Boolean);
  assert.ok(foot("adriatic").includes("✓ Active"));
  assert.ok(foot("flamingo").includes("Project: skiff"));
  assert.ok(foot("ember").includes("Session: fix login"));
  assert.ok(!foot("ultraviolet").some((t) => /^(Project|Session): /.test(t)));
  // A project on the app theme overrides nothing.
  a.S.appTheme = "flamingo";
  a.S.focused = null;
  assert.equal(a.themes.themeOverrides().length, 0);
});

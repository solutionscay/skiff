import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real menus.ts. Skippers are always on: the project and fleet menus
// offer their Skipper with no setting behind it.
const stub = new Proxy({}, { get: (_, k) => (k === "__esModule" ? false : () => []) });
const withStubs = (imports: Record<string, unknown>) => new Proxy(imports, { get: (t, k: string) => t[k] ?? stub });
const orStub = (mod: Record<string, unknown>) => new Proxy(mod, { get: (t, k: string) => t[k] ?? (stub as any)[k] });

function menus(fleetRuns: boolean) {
  const projects = [{ name: "api", path: "/harbor/api", fleet: "Harbor", worktrees: [{ path: "/harbor/api", branch: "main", is_main: true }] }];
  // As projects.toml reads now: no switch.
  const S: any = { projects, skiffAgent: { folder: "/skipper" }, focused: null, selection: [], groups: [], closedProjects: [] };
  const skipper = { id: "fs", skipper: "fleet:Harbor", state: "idle" };
  const skiffAgent = orStub({
    skiffAgentFolder: () => S.skiffAgent.folder,
    fleetSkipperOf: () => (fleetRuns ? skipper : null),
    projectSkipperOf: () => null,
    fleetScope: (f: string) => `fleet:${f}`,
  });
  const el = () => ({ append() {}, appendChild() {}, setAttribute() {}, addEventListener() {}, classList: { add() {} }, dataset: {} });
  let items: any[] = [];
  const m = moduleFromSource(
    "../src/workspace/menus.ts",
    withStubs({
      "../app/skiffAgent": skiffAgent,
      "../app/state": { S, sessions: new Map(), removing: new Set(), removeErrors: new Map(), selectedWorktree: new Map() },
      "../app/stateQueries": orStub({ place: () => null }),
      "../ui/dom": { h: el, button: el, $: el },
      "../app/panels": { launchMenu: { entries: () => [] } },
      "./tree": orStub({ bareWorktree: (p: any) => p.worktrees[0] }),
      "../ui/contextMenu": { ctxMenu: { open: (_x: number, _y: number, _t: string, list: any[]) => (items = list) } },
    }),
    { console, localStorage: { getItem: () => null, setItem() {} } },
    "",
  );
  const labels = () => items.filter((i) => i.label).map((i) => i.label);
  return {
    project: () => (m.projectMenu(projects[0], 0, 0), labels()),
    fleet: () => (m.fleetMenu({ name: "Harbor", projects }, 0, 0), labels()),
  };
}

test("the project and fleet menus offer their Skipper with no setting behind it", () => {
  const idle = menus(false);
  assert.ok(idle.project().includes("Start project Skipper"));
  assert.ok(idle.fleet().includes("Start fleet Skipper"));
  // While the fleet Skipper runs, the project menu opens it in place of a project Skipper.
  const running = menus(true);
  const project = running.project();
  assert.ok(project.includes("Open fleet Skipper") && !project.includes("Start project Skipper"));
  assert.ok(running.fleet().includes("Open fleet Skipper"));
});

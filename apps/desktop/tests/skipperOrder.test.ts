import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real skipperOrder.ts: the list's order and the tree's order stay apart.
const session = (id: string) => ({ id, state: "idle" });
const S: any = { skiffAgent: { folder: "/skipper", skippers_order: [] } };
const projects = [{ name: "a" }, { name: "b", fleet: "web" }, { name: "c", fleet: "web" }, { name: "d" }];
const sections = () => {
  const loose = projects.filter((p) => !p.fleet);
  return [{ fleet: null, projects: loose }, { fleet: "web", projects: projects.filter((p) => p.fleet === "web") }];
};
const wrote: unknown[] = [];
const O: any = moduleFromSource(
  "../src/workspace/skipperOrder.ts",
  {
    "../app/skiffAgent": {
      projectSkipperOf: (scope: string) => (scope === "c" ? null : session(`s-${scope}`)),
      fleetSkipperOf: (fleet: string) => session(`s-fleet-${fleet}`),
      skiffAgentFolder: () => "/skipper",
      skiffAgentName: () => "Admiral",
      skiffAgentSession: () => session("s-admiral"),
    },
    "../app/hostCaps": { hostInvoke: async (_cap: string, _cmd: string, args: any) => void wrote.push(args.edit) },
    "../app/renderRequest": { render() {} },
    "../app/state": { S },
    "../ui/alerts": { showError() {} },
    "./tree": { treeSections: sections },
  },
  { console },
  "",
);
// Plain arrays: the module runs in its own realm.
const plain = (v: unknown) => JSON.parse(JSON.stringify(v));
const list = () => plain(O.skipperEntries().map((e: any) => e.scope));

test("the Skippers list's order and the tree's order do not change each other", async () => {
  // No stored order: tree order, the Skiff agent first.
  assert.deepEqual(list(), [null, "a", "d", "fleet:web", "b"]);

  // A drag stores the list's order and leaves the tree's as it is.
  const tree = projects.map((p) => p.name);
  await O.moveSkipper("b", "a", "before");
  assert.deepEqual(list(), [null, "b", "a", "d", "fleet:web"]);
  assert.deepEqual(projects.map((p) => p.name), tree);
  assert.deepEqual(plain(wrote), [{ key: "skippers_order", value: ["b", "a", "d", "fleet:web"] }]);

  // A move in the tree leaves the list as it is. A new row comes last.
  projects.reverse();
  projects.push({ name: "new" });
  assert.deepEqual(list(), [null, "b", "a", "d", "fleet:web", "new"]);

  // A stored name with no row is skipped and kept, and takes its place when its row is back.
  S.skiffAgent.skippers_order = ["gone", "b", "c", "a"];
  assert.deepEqual(list().slice(1, 3), ["b", "a"]);
  assert.deepEqual(plain(O.movedScopes(["b", "a", "d"], ["gone", "b", "c", "a"], "d", "b", "before")), ["gone", "d", "b", "c", "a"]);
});

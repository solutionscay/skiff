import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

function harness() {
  class El {
    dataset: Record<string, string> = {};
    style = { setProperty() {}, getPropertyValue() { return ""; } };
    classList = { add() {}, remove() {}, toggle() {}, contains() { return false; } };
    children: any[] = [];
    value = ""; hidden = false; outerHTML = ""; replacements = 0;
    append(...c: any[]) { this.children.push(...c); }
    appendChild(c: any) { this.children.push(c); return c; }
    setAttribute() {} addEventListener() {} after() {} focus() {} select() {}
    replaceChildren(...c: any[]) { this.children = c; this.replacements++; }
  }
  const h = (_tag: string, _cls = "", text = "") => { const e = new El(); e.outerHTML = text; return e; };
  const document = { body: new El(), documentElement: new El(), createElementNS: () => new El(), getElementById: () => null };
  const stored = new Map<string, string>();
  let host: string | null = null;
  const project = { name: "A", worktrees: [] };
  const imports = {
    "../ui/dom": { h }, "../app/hosts": { shownHost: () => host, hostName: () => host ?? "local" },
    "../app/state": { S: { themes: [] }, sessions: new Map() },
    "../app/hostCaps": { hostCan: () => false },
    "./tree": { treeSections: () => [{ projects: [project] }] },
    "./sidebarLines": { worktreeLines: () => [] },
    "../app/skiffAgent": { skiffAgentSession: () => null },
    "./missionControl/state": { indexLedger: () => ({}) },
    "./missionControl/terminal": { faceLayer: new El() },
    "./missionControl/edges": { mountEdges() {} },
    "./missionControl/connectors": { mountConnectors() {} },
    "../app/keys": { keyLabel: () => "" },
    "../i18n": { t: (key: string) => key, tn: (key: string) => key, upper: (s: string) => s },
  };
  const mod = moduleFromSource("../src/workspace/missionControl.ts", imports, {
    document, localStorage: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, val: string) => stored.set(key, val) },
    window: {}, console,
  }, `\nexport const review = { loadKept, saveKept, model, refill, setRunning, kept: () => ({ panel: panelOpen }) };\n`);
  return { mod, stored, setHost: (next: string | null) => { host = next; }, El };
}

test("loading old persistence removes obsolete layout fields", () => {
  const h = harness();
  h.stored.set("skiff.mission.local", JSON.stringify({ folded: ["project:A"], opened: [], panel: false, offs: {}, order: [] }));
  h.mod.review.loadKept();
  assert.deepEqual(JSON.parse(h.stored.get("skiff.mission.local")!), { panel: false });
});

test("the panel uses a separate key for each Host", () => {
  const h = harness();
  h.stored.set("skiff.mission.local", JSON.stringify({ folded: ["project:A"], panel: false }));
  h.stored.set("skiff.mission.remote", JSON.stringify({ opened: ["project:A"], panel: true }));
  h.mod.review.loadKept();
  assert.equal(h.mod.review.kept().panel, false);
  h.setHost("remote"); h.mod.review.loadKept();
  assert.equal(h.mod.review.kept().panel, true);
});

test("unchanged labels do not replace panel nodes", () => {
  const h = harness();
  const el = new h.El();
  const part = new h.El(); part.outerHTML = '<span style="--blink-delay:1s;">A</span>';
  h.mod.review.refill(el, [part]);
  const same = new h.El(); same.outerHTML = '<span style="--blink-delay:2s;">A</span>';
  h.mod.review.refill(el, [same]);
  assert.equal(el.replacements, 1);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

import * as G from "../src/workspace/missionLayout.ts";
import * as Grid from "../src/workspace/missionControl/grid.ts";

function harness() {
  class El {
    dataset: Record<string, string> = {};
    style = { setProperty() {}, getPropertyValue() { return ""; } };
    classList = { add() {}, remove() {}, toggle() {}, contains() { return false; } };
    children: any[] = [];
    value = ""; hidden = false; outerHTML = ""; replacements = 0;
    append(...c: any[]) { this.children.push(...c); }
    appendChild(c: any) { this.children.push(c); return c; }
    querySelectorAll() { return []; }
    querySelector() { return null; }
    setAttribute() {} addEventListener() {} after() {} focus() {} select() {}
    replaceChildren(...c: any[]) { this.children = c; this.replacements++; }
  }
  const h = (_tag: string, _cls = "", text = "") => { const e = new El(); e.outerHTML = text; return e; };
  const document = { body: new El(), documentElement: new El(), createElementNS: () => new El(), getElementById: () => null };
  const stored = new Map<string, string>();
  let host: string | null = null;
  const project = { name: "A", worktrees: [] };
  const storage = { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, val: string) => stored.set(key, val) };
  const persistence = moduleFromSource("../src/workspace/missionControl/persistence.ts", {}, { localStorage: storage }, "");
  const render = moduleFromSource("../src/workspace/missionControl/render.ts", {}, {}, "");
  const interact = moduleFromSource("../src/workspace/missionControl/interact.ts", { "../missionLayout": G }, {}, "");
  const routes = [0, 1].map(entry => ({entry, endpoint: entry ? "remote" : null, host: String(entry), generation: 1}));
  const connections = routes.map(r => ({...r, phase: "up"}));
  const session = { id: "same", state: "waiting", title: "shared" };
  const admiral = { id: "admiral", state: "idle", title: "Admiral", coordinator: true };
  const sessions = new Map([[session.id, session], [admiral.id, admiral]]);
  let current = routes[0];
  const opened: string[] = [];
  const graphRenderer = {
    createRenderer(state: any, _dom: any, hooks: any) {
      return { draw() {
        state.m = hooks.model();
        state.placed = Grid.placeGraph(state.m.g);
        for (const id of state.placed.cards.keys()) {
          const card = new El();
          card.classList.contains = () => id === session.id;
          state.cards.set(id, card);
        }
        for (const id of state.placed.clusters.keys()) state.clusters.set(id, new El());
      }};
    },
  };
  const imports = {
    "./missionControl/interact": interact, "./missionControl/persistence": persistence, "./missionControl/render": graphRenderer, "./missionControl/panel": {createPanel: () => ({drawPanel() {}, read() {}})},
    "./missionLayout": G,
    "./missionControl/model": { sinceText: () => "", matchText: () => "shared" },
    "./model": { fuzzy: (q: string, text: string) => text.includes(q) ? 1 : -1 },
    "../platform/invoke": { routeOf: (i: number) => routes[i], pageRoute: () => current,
      withinHost: (route: typeof current, run: () => unknown) => {
        const prev = current; current = route;
        try { return run(); } finally { current = prev; }
      }},
    "../app/attention": { hostCounts: () => ({waiting: 1, working: 0}), liveRoute: (r: typeof current) => connections[r.entry].phase === "up" && r === routes[r.entry] },
    "../app/hostsPanel": {hostIcon: () => new El()},
    "../app/connections": {orderedConnections: () => connections, titleOfHost: (c: typeof current) => c.endpoint ?? "local",
      focusConnection: (c: typeof current) => { current = routes[c.entry]; }},
    "./missionMotion": { closeMission: (id: string) => opened.push(current.host + "/" + id) },
    "../ui/dom": { h }, "../app/hosts": { hostPrefix: () => current.endpoint ?? "local", hostName: () => host ?? "local" },
    "../app/state": { S: { themes: [] }, sessions },
    "../app/hostCaps": { hostCan: () => true },
    "./ledger": {ledgerRows: () => [{uid: "task-1", kind: "task", pane_id: "same", owner: "grand", title: String(current.host)}]},
    "./tree": { treeSections: () => [{ projects: [project] }] },
    "./sidebarLines": { worktreeLines: () => [{ session }] },
    "../app/skiffAgent": { skiffAgentSession: () => admiral, skiffAgentName: () => "Admiral" },
    "./missionControl/state": { indexLedger: () => ({}) },
    "./missionControl/terminal": { createFaces: () => ({faceLayer: new El()}) },
    "./missionControl/edges": { mountEdges() {}, drawEdges() {} },
    "../app/keys": { keyLabel: () => "" },
    "../i18n": { t: (key: string) => key, tn: (key: string) => key, upper: (s: string) => s },
  };
  const mod = moduleFromSource("../src/workspace/missionControl.ts", imports, {
    document, localStorage: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, val: string) => stored.set(key, val) },
    window: {}, console, getComputedStyle: () => ({ getPropertyValue: () => "1" }),
  }, `export const review = { draw, search, allCount, targets, light, nextWaitingCard, openCard, frames };`);
  const state = { panelOpen: true, keptFor: undefined };
  const kept = persistence.createPersistence(state, () => host);
  return { mod, render, stored, kept, state, opened, routes, connections, setHost: (next: string | null) => { host = next; }, El };
}

test("loading old persistence removes obsolete layout fields", () => {
  const h = harness();
  h.stored.set("skiff.mission.local", JSON.stringify({ folded: ["project:A"], opened: [], panel: false, offs: {}, order: [] }));
  h.kept.loadKept();
  assert.deepEqual(JSON.parse(h.stored.get("skiff.mission.local")!), { panel: false });
});

test("the panel uses a separate key for each Host", () => {
  const h = harness();
  h.stored.set("skiff.mission.local", JSON.stringify({ folded: ["project:A"], panel: false }));
  h.stored.set("skiff.mission.remote", JSON.stringify({ opened: ["project:A"], panel: true }));
  h.kept.loadKept();
  assert.equal(h.state.panelOpen, false);
  h.setHost("remote"); h.kept.loadKept();
  assert.equal(h.state.panelOpen, true);
});

test("unchanged labels do not replace panel nodes", () => {
  const h = harness();
  const el = new h.El();
  const part = new h.El(); part.outerHTML = '<span style="--blink-delay:1s;">A</span>';
  h.render.refill(el, [part]);
  const same = new h.El(); same.outerHTML = '<span style="--blink-delay:2s;">A</span>';
  h.render.refill(el, [same]);
  assert.equal(el.replacements, 1);
});

test("Mission searches both Host frames, visits both waits and rejects a stale card", async () => {
  const { mod, routes, connections, opened } = harness();
  await mod.board.prepare("same");
  const { review } = mod;
  const [local, remote] = [...review.targets.keys()].filter(key => review.targets.get(key).id === "same");
  assert.equal(review.targets.size, 4);
  for (const [entry, frame] of review.frames) {
    assert.equal(frame.state.m.strs.length, 1);
    assert.equal(frame.state.m.strs[0].row.title, String(entry));
    assert.equal(frame.state.m.strs[0].from, "admiral");
    assert.equal(frame.state.m.strs[0].to, "same");
  }
  const first = review.frames.get(0).el.style;
  const second = review.frames.get(1).el.style;
  assert.equal(parseFloat(second.left) - parseFloat(first.width), 40);
  assert.equal(review.nextWaitingCard(), remote);
  review.search.value = "remote:shared";
  review.draw();
  assert.equal(review.allCount.textContent, "1");
  review.openCard(remote, false);
  assert.deepEqual(opened, ["1/same"]);
  routes[1] = {...routes[1], generation: 2};
  review.openCard(remote, false);
  connections[1].phase = "failed";
  review.draw();
  assert.equal(review.frames.size, 2);
  assert.equal(review.frames.get(1).body.inert, true);
  assert.equal(review.nextWaitingCard(), local);
  review.openCard(local, false);
  assert.deepEqual(opened, ["1/same", "0/same"]);
});

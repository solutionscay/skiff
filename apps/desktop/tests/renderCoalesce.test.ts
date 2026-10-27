import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real render.ts. Every step it calls is a counter.
function app() {
  const timers: (() => void)[] = [];
  const window = {
    addEventListener() {},
    setTimeout: (run: () => void) => timers.push(run),
  };
  const globals = { window, console };
  // The trace's counts, as stalls.ts keeps them while the trace runs.
  let counts = new Map<string, number>();
  const stalls = {
    step: (_: string, run: () => void) => run(),
    tally: (name: string) => counts.set(name, (counts.get(name) ?? 0) + 1),
    takeTallies: () => {
      const tally = counts;
      counts = new Map();
      return { tally };
    },
  };
  const ran = { sidebar: 0, menu: 0 };
  const stub = new Proxy({}, { get: (_, k) => (k === "__esModule" ? false : () => {}) });
  const imports: Record<string, unknown> = {
    "../diagnostics/stalls": stalls,
    "../workspace/sidebar": { renderSidebar: () => ran.sidebar++ },
    "./commands": { syncMenuSoon: () => ran.menu++ },
    "../terminal/terminalHost": { host: { classList: { toggle() {} } } },
    "../workspace/sidebarLines": { worktreeLines: () => [] },
    "./state": { S: { focused: null, selectedProject: null }, OTHER: "\0other" },
  };
  const render = moduleFromSource("../src/app/render.ts", new Proxy(imports, { get: (t, k: string) => t[k] ?? stub }), globals, "");
  const fire = () => timers.splice(0).forEach((run) => run());
  return { render, stalls, ran, fire, timers };
}

test("daemon events in one burst make one render, counted once per cause", () => {
  const a = app();
  a.render.scheduleRender("event.state");
  a.render.scheduleRender("event.state");
  a.render.scheduleRender("event.title");
  a.render.scheduleRender("poll");
  assert.equal(a.timers.length, 1);
  assert.equal(a.ran.sidebar, 0);
  a.fire();
  assert.equal(a.ran.sidebar, 1);
  const { tally } = a.stalls.takeTallies();
  assert.deepEqual(Object.fromEntries(tally), { render: 1, "render.event.state": 1, "render.event.title": 1, "render.poll": 1 });
});

test("a direct render takes the requests made before it; the timer then draws nothing", () => {
  const a = app();
  a.render.scheduleRender("event.state");
  a.render.render();
  assert.equal(a.ran.sidebar, 1);
  a.fire();
  assert.equal(a.ran.sidebar, 1);
  const { tally } = a.stalls.takeTallies();
  assert.deepEqual(Object.fromEntries(tally), { render: 1, "render.event.state": 1, "render.ui": 1 });
});

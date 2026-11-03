import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

test("pane opens share a fit frame and unchanged grids skip FitAddon.fit", async () => {
  const frames = new Map<number, () => void>();
  const timers = new Map<number, () => void>();
  let seq = 0;
  const host = { addEventListener() {}, contains: () => false };
  const park = {};
  const panes = new Map<string, any>();
  let hooks: any;
  let fits = 0;
  let measures = 0;
  let measured = true;
  let cols = 80;
  const resize: any[] = [];
  const imports = new Proxy({
    "./terminalRuntime": {
      openPane: async (id: string) => {
        const p = {
          el: { isConnected: true, parentElement: host },
          term: { cols: 80, rows: 24 },
          fit: {
            proposeDimensions: () => { measures++; return measured ? { cols, rows: 24 } : undefined; },
            fit: () => { fits++; p.term.cols = cols; },
          },
        };
        panes.set(id, p);
        return p;
      },
      useWebgl() {},
    },
    "../workspace/layoutView": { createLayoutView: (_: unknown, h: unknown) => { hooks = h; return {}; } },
    "../workspace/layoutSlots": { isSlot: () => false },
    "./terminalHost": { host, park },
    "./terminalState": { panes },
    "../app/state": { S: {}, sessions: new Map(["a", "b", "c", "d"].map(id => [id, { cols: 80, rows: 24 }])) },
    "../diagnostics/stalls": { step: (_: string, run: () => void) => run() },
    "../app/hostCaps": { hostInvoke: async (_: string, cmd: string, args: unknown) => { resize.push({ cmd, args }); } },
  } as Record<string, unknown>, { get: (obj, key: string) => obj[key] ?? new Proxy({}, { get: () => () => {} }) });
  const terminal = moduleFromSource("../src/terminal/terminal.ts", imports, {
    window: { setTimeout: (run: () => void) => { timers.set(++seq, run); return seq; } },
    clearTimeout: (id: number) => timers.delete(id),
    requestAnimationFrame: (run: () => void) => { frames.set(++seq, run); return seq; },
    cancelAnimationFrame: (id: number) => frames.delete(id),
    performance: { now: () => 1000 },
    ResizeObserver: class { observe() {} },
    console,
  }, "");
  for (const id of ["a", "b", "c", "d"]) hooks.attach(id, { isConnected: true, appendChild() {} });
  await Promise.resolve();
  assert.equal(measures, 0);
  assert.equal(frames.size, 1);
  const frame = () => { const pending = [...frames.values()]; frames.clear(); pending.forEach(run => run()); };
  frame();
  assert.equal(measures, 4);
  assert.equal(fits, 0);
  // A cell not yet measured retries. A changed grid still fits and sends its size.
  measured = false;
  terminal.fitShown();
  assert.equal(frames.size, 1);
  measured = true;
  cols = 81;
  frame();
  assert.equal(fits, 4);
  for (const run of timers.values()) run();
  assert.equal(resize.length, 4);
  assert.ok(resize.every(r => r.cmd === "pty_resize" && r.args.cols === 81));
});

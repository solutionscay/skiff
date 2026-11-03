import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

test("pane fits share one frame, read before writes and reuse observer sizes and cells", async () => {
  const frames = new Map<number, () => void>();
  const timers = new Map<number, () => void>();
  const targets = new Set<object>();
  let observe: (entries: any[]) => void;
  let loaded: () => void;
  let seq = 0;
  let clock = 0;
  const host = { addEventListener() {}, contains: () => false };
  const park = {};
  const panes = new Map<string, any>();
  let hooks: any;
  let measures = 0;
  let styles = 0;
  let valid = true;
  const events: string[] = [];
  const resize: any[] = [];
  const globals = {
    window: {
      devicePixelRatio: 1,
      setTimeout: (run: () => void, delay: number) => { assert.equal(delay, 50); timers.set(++seq, run); return seq; },
      getComputedStyle: () => { styles++; events.push("read"); return { paddingLeft: "0", paddingRight: "0", paddingTop: "0", paddingBottom: "0" }; },
    },
    document: { fonts: { addEventListener: (_: string, f: () => void) => { loaded = f; } } },
    clearTimeout: (id: number) => timers.delete(id),
    requestAnimationFrame: (run: () => void) => { frames.set(++seq, run); return seq; },
    cancelAnimationFrame: (id: number) => frames.delete(id),
    ResizeObserver: class {
      constructor(f: typeof observe) { observe = f; }
      observe(el: object) { targets.add(el); }
      unobserve(el: object) { targets.delete(el); }
    },
    performance: { now: () => (clock += 1000) },
    console,
  };
  const fitter = moduleFromSource("../src/terminal/terminalFit.ts", {}, globals, "");
  const imports = new Proxy({
    "./terminalFit": fitter,
    "./terminalRuntime": {
      openPane: async (id: string) => {
        const chars = { hasValidSize: true, measure() { measures++; chars.hasValidSize = valid; } };
        const p = {
          el: { isConnected: true, parentElement: host },
          term: {
            element: { parentElement: {} },
            options: { fontFamily: "mono", fontSize: 13 }, cols: 80, rows: 24,
            _core: {
              _charSizeService: chars,
              _renderService: { dimensions: { css: { cell: { width: 10, height: 20 } } }, clear() { events.push("clear"); } },
            },
            resize(cols: number, rows: number) {
              assert.equal(events.at(-1), "clear");
              events.push("write");
              chars.measure(); // xterm's synchronous post-resize measurement must use the cache.
              p.term.cols = cols; p.term.rows = rows;
            },
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
    "../app/state": { S: { fontSize: 13 }, sessions: new Map(["a", "b", "c", "d"].map(id => [id, { cols: 80, rows: 24 }])) },
    "../diagnostics/stalls": { step: (_: string, run: () => void) => run() },
    "../app/hostCaps": { hostInvoke: async (_: string, cmd: string, args: unknown) => { resize.push({ cmd, args }); } },
  } as Record<string, unknown>, { get: (obj, key: string) => obj[key] ?? new Proxy({}, { get: () => () => {} }) });
  const terminal = moduleFromSource("../src/terminal/terminal.ts", imports, globals, "");
  const frame = () => { const pending = [...frames.values()]; frames.clear(); pending.forEach(run => run()); };
  const sizes = (width: number) => observe!([...targets].map(target => ({ target, contentRect: { width, height: 480 } })));
  for (const id of ["a", "b", "c", "d"]) hooks.attach(id, { isConnected: true, appendChild() {} });
  await Promise.resolve();
  for (let i = 0; i < 20; i++) terminal.fitShown();
  assert.equal(frames.size, 1);
  frame(); // Register boxes; wait for the first observer delivery.
  assert.equal(measures, 0);
  sizes(824);
  frame();
  assert.equal(measures, 4);
  assert.equal(styles, 4);
  assert.deepEqual(events, ["read", "read", "read", "read", "clear", "write", "clear", "write", "clear", "write", "clear", "write"]);
  assert.equal(timers.size, 1);
  events.length = 0;
  sizes(834);
  frame();
  assert.equal(measures, 4);
  assert.equal(styles, 4);
  assert.equal(events.filter(e => e === "write").length, 4);
  for (const run of timers.values()) run();
  timers.clear();
  assert.equal(resize.length, 4);
  assert.ok(resize.every(r => r.cmd === "pty_resize" && r.args.cols === 82));
  events.length = 0;
  for (let i = 0; i < 20; i++) terminal.fitShown();
  assert.equal(frames.size, 1);
  frame();
  assert.deepEqual(events, []);
  assert.equal(measures, 4);
  // A font change measures only that pane. A loaded font or DPR change invalidates all.
  panes.get("a").term.options.fontSize = 14;
  terminal.fitShown(); frame();
  assert.equal(measures, 5);
  loaded!(); frame();
  assert.equal(measures, 9);
  globals.window.devicePixelRatio = 2;
  terminal.fitShown(); frame();
  assert.equal(measures, 13);
  terminal.holdFits(); sizes(844); terminal.fitShown();
  assert.equal(frames.size, 0);
  terminal.releaseFits(); frame();
  assert.equal(panes.get("a").term.cols, 83);
  // Unmeasured cells retry without sending a grid. Parked boxes are released.
  const p = panes.get("a");
  p.term._core._charSizeService.hasValidSize = false;
  p.term._core._renderService.dimensions.css.cell.width = 0;
  valid = false;
  terminal.fitShown(); frame();
  assert.equal(frames.size, 1);
  valid = true;
  p.term._core._renderService.dimensions.css.cell.width = 10;
  frame();
  p.el.parentElement = park;
  terminal.fitShown(); frame();
  assert.equal(targets.size, 3);
  // Even a failed resize restores xterm's own measurement method.
  const measure = p.term._core._charSizeService.measure;
  p.term.resize = () => { throw new Error("resize failed"); };
  assert.throws(() => fitter.resizeTerminal(p.term, { cols: 90, rows: 24 }), /resize failed/);
  assert.equal(p.term._core._charSizeService.measure, measure);
});

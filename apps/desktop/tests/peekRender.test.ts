import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, moduleFromSource } from "./workspaceHarness.ts";

/** #303: a local diff with a renderer runs Skiff's diff runner in the peek.
 *  The real peek controller, its terminal record and its card run over a
 *  fake xterm and a fake clock. The Host calls are recorded. */

class El {
  cls: Set<string>;
  text: string;
  kids: El[] = [];
  parent: El | null = null;
  hidden = false;
  tabIndex = 0;
  title = "";
  constructor(_tag: string, cls = "", text = "") {
    this.cls = new Set(cls.split(" ").filter(Boolean));
    this.text = text;
  }
  get classList() {
    return { contains: (c: string) => this.cls.has(c), add: (c: string) => void this.cls.add(c), remove: (c: string) => void this.cls.delete(c) };
  }
  set textContent(v: string) { this.text = v; }
  append(...kids: (El | string)[]) {
    for (const k of kids) {
      if (typeof k === "string") this.text += k;
      else {
        k.remove();
        k.parent = this;
        this.kids.push(k);
      }
    }
  }
  appendChild(k: El) {
    this.append(k);
    return k;
  }
  get parentElement() { return this.parent; }
  setAttribute() {}
  addEventListener() {}
  focus() { doc.activeElement = this; }
  contains(n: unknown): boolean {
    for (let x = n as El | null; x; x = x.parent) if (x === this) return true;
    return false;
  }
  remove() {
    if (this.parent) this.parent.kids = this.parent.kids.filter((k) => k !== this);
    this.parent = null;
  }
  querySelector(sel: string): El | null {
    const want = sel.split(",").map((s) => s.trim().split(".").pop()!);
    for (const k of this.kids) {
      if (want.some((c) => k.cls.has(c))) return k;
      const r = k.querySelector(sel);
      if (r) return r;
    }
    return null;
  }
}

const doc = { activeElement: null as unknown, addEventListener() {} };
const DONE = (code: number) => `\x1b]7750;${code}\x07`;

/** The xterm parts the peek uses: text by line, an OSC handler, a write queue. */
class FakeTerminal {
  static all: FakeTerminal[] = [];
  cols = 100;
  rows = 30;
  lines: string[] = [""];
  osc = new Map<number, (data: string) => boolean>();
  focused = false;
  disposed = false;
  parser = { registerOscHandler: (id: number, f: (data: string) => boolean) => void this.osc.set(id, f) };
  buffer = { active: { get length() { return 0; }, getLine: (_y: number) => null as unknown } };
  constructor() {
    FakeTerminal.all.push(this);
    const lines = this.lines;
    this.buffer.active = {
      get length() { return lines.length; },
      getLine: (y: number) => ({ translateToString: () => lines[y] ?? "" }),
    };
  }
  loadAddon() {}
  open() {}
  attachCustomKeyEventHandler() {}
  onData() {}
  focus() { this.focused = true; }
  dispose() { this.disposed = true; }
  write(data: Uint8Array | string, done?: () => void) {
    let text = typeof data === "string" ? data : new TextDecoder().decode(data);
    text = text.replace(/\x1b\](\d+);([^\x07]*)\x07/g, (_m, id: string, body: string) => (this.osc.get(Number(id))?.(body), ""));
    const parts = text.split(/\r?\n/);
    this.lines[this.lines.length - 1] += parts[0];
    this.lines.push(...parts.slice(1));
    // xterm parses later: the callback comes after a turn.
    if (done) setImmediate(done);
  }
  get all() { return this.lines.join("\n"); }
}

function peek(opened: (file: string) => unknown) {
  const main = new El("main");
  const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);
  let clock = 1_000_000;
  let timers: { at: number; run: () => void; id: number }[] = [];
  let nextTimer = 1;
  const calls: { cmd: string; args: any; ok: (v: unknown) => void; fail: (e: unknown) => void }[] = [];
  const channels = new Map<string, { onmessage(m: unknown): void }>();
  const errors: unknown[] = [];
  const panes = new Map();
  let row = "change:a";
  const rows: Record<string, unknown> = {};
  for (const k of ["a", "b", "c"]) rows[`change:${k}`] = { kind: "diff", wt: "/w", file: `${k} file`, base: undefined };
  const mod = moduleFromSource(
    "../src/terminal/peek.ts",
    {
      "@tauri-apps/api/core": {
        invoke: (cmd: string, args: any) => {
          if (cmd === "open_diff") return Promise.resolve(opened(args.file));
          if (cmd === "peek_owner") return Promise.resolve("me");
          return Promise.resolve(null);
        },
        Channel: class {},
      },
      "@xterm/xterm": { Terminal: FakeTerminal },
      "@xterm/addon-fit": { FitAddon: class { fit() {} } },
      "../ui/dom": { $: () => main, h, button: (cls: string, text: string) => h("button", cls, text) },
      "../ui/icons": { icon: () => h("svg") },
      "../ui/alerts": { showError: (e: unknown) => void errors.push(e) },
      "./osc52": { handleOsc52() {} },
      "../platform/clipboard": {},
      "../diagnostics/clipTrace": {},
      "../app/keyboard": { currentRow: () => ({ key: row }), itemKey: (r: { key: string }) => r.key, appKey: () => null },
      "../app/renderRequest": { render() {} },
      "../app/keys": { keyLabel: () => "Ctrl+Shift+W" },
      "../app/state": { S: { fontSize: 13 }, sessions: new Map() },
      "../platform/ipcBytes": { toBytes: (m: unknown) => m },
      "../diagnostics/latency": { traceEcho: () => false },
      "../platform/os": { openUrl() {} },
      "../workspace/rowActs": { rowActs: (key: string) => ({ preview: rows[key] }) },
      "./pasteClean": { pasteInto() {}, guardPasteEvents() {} },
      "./terminalState": { panes },
      "./terminal": { fitShown() {} },
      "./terminalRuntime": { coverPanes() {}, FONT: "mono", TERM_THEME: {} },
      "../app/hostCaps": {
        hostInvoke: (_cap: string, cmd: string, args: any) => {
          if (cmd === "subscribe_output") {
            channels.set(args.session, args.onOutput);
            return Promise.resolve(null);
          }
          return new Promise((ok, fail) => calls.push({ cmd, args, ok, fail }));
        },
      },
      "../app/dataPlane": { dataEpoch: () => 1 },
      "./peekHostView": {},
      "./peekHostReads": { showDiff() {}, showFile() {}, showRead() {} },
    },
    {
      Promise, Map, Set, JSON, document: doc, URL, ResizeObserver: class { observe() {} disconnect() {} },
      Date: { now: () => clock },
      sessionStorage: { getItem: () => null, setItem() {}, removeItem() {} },
      window: {
        setTimeout: (run: () => void, ms = 0) => (timers.push({ at: clock + ms, run, id: nextTimer }), nextTimer++),
      },
      clearTimeout: (id: number) => void (timers = timers.filter((t) => t.id !== id)),
      console,
      TextDecoder,
    },
    "",
  );
  const encoder = new TextEncoder();
  return {
    mod, calls, errors, panes, main,
    on: (key: string) => void (row = key),
    /** Moves the clock, running each timer when due. */
    async advance(ms: number) {
      const end = clock + ms;
      for (;;) {
        await flush();
        timers.sort((a, b) => a.at - b.at);
        const t = timers[0];
        if (!t || t.at > end) break;
        timers.shift();
        clock = Math.max(clock, t.at);
        t.run();
      }
      clock = end;
      await flush();
    },
    created: () => calls.filter((c) => c.cmd === "create_session"),
    killed: () => calls.filter((c) => c.cmd === "kill_session").map((c) => c.args.session),
    /** Answers the oldest pending create with session `id`. */
    async start(id: string) {
      const c = calls.find((c) => c.cmd === "create_session" && !c.args.done)!;
      c.args.done = true;
      c.ok({ id, peek: true, interactive: false, state: "working" });
      await flush();
      await flush();
    },
    output: (id: string, text: string) => channels.get(id)!.onmessage(encoder.encode(text)),
    term: () => FakeTerminal.all.at(-1)!,
    peekEl: () => main.kids.find((k) => k.cls.has("peek") && !k.cls.has("file-preview")),
  };
}

const render = (file: string) => ({ kind: "render", program: "/opt/skiff/skiff", args: ["open-with", "render", JSON.stringify({ file })], cwd: "/w", label: "delta" });

test("a renderer shows at once, leaves the keys on the row, and is not let go as an app", async () => {
  const v = peek(render);
  v.mod.startPreview("change:a", false);
  await flush();
  const el = v.peekEl()!;
  assert.ok(el && !el.hidden && !el.cls.has("pending"), "it shows before skiffd answers");
  assert.equal(v.term().focused, false, "the keys stay on the row");
  const spec = v.created()[0].args.spec;
  assert.deepEqual([spec.command, spec.args.slice(0, 2), spec.peek, spec.cwd], ["/opt/skiff/skiff", ["open-with", "render"], true, "/w"]);
  await v.start("s1");
  // A tool that waits for more than 3 s keeps its peek.
  await v.advance(10_000);
  assert.deepEqual(v.killed(), []);
  assert.ok(v.peekEl() === el && !el.hidden);
  // Entering the preview gives it the keys.
  v.mod.enterPreview();
  assert.equal(v.term().focused, true);
});

test("the exit waits for the runner's last output, even when it comes late", async () => {
  const v = peek(render);
  v.mod.startPreview("change:a", false);
  await flush();
  await v.start("s1");
  v.output("s1", "delta top\r\n");
  v.mod.peekExited("s1", 0);
  // The exit came first; the rest of the output is still on its way.
  await v.advance(500);
  assert.doesNotMatch(v.term().all, /Press any key/);
  v.output("s1", "delta bottom\r\n" + DONE(0));
  await v.advance(100);
  const all = v.term().all;
  assert.match(all, /delta top[\s\S]*delta bottom[\s\S]*Press any key to close\./);
  assert.ok(v.peekEl() && !v.peekEl()!.hidden);
});

test("a failed git run says so; a missing tool is named", async () => {
  const v = peek(render);
  v.mod.startPreview("change:a", false);
  await flush();
  await v.start("s1");
  v.output("s1", "fatal: bad revision 'x'\r\ngit: exit 128\r\n" + DONE(90));
  v.mod.peekExited("s1", 90);
  await v.advance(100);
  assert.match(v.term().all, /fatal: bad revision[\s\S]*Git could not make the diff\./);
  assert.doesNotMatch(v.term().all, /delta exited/);

  v.on("change:b");
  v.mod.startPreview("change:b", false);
  await flush();
  await v.start("s2");
  v.output("s2", "delta: not found\r\n" + DONE(127));
  v.mod.peekExited("s2", 127);
  await v.advance(100);
  assert.match(String(v.errors.at(-1)), /delta not found/);
});

test("rapid row switches end every runner, one still starting too, and leave no session or pane", async () => {
  const v = peek(render);
  v.mod.startPreview("change:a", false);
  await flush();
  // The row changes before skiffd answers for a.
  v.on("change:b");
  v.mod.startPreview("change:b", false);
  await flush();
  await v.start("s-a");
  assert.deepEqual(v.killed(), ["s-a"], "a runner that started for a row the user left ends");
  await v.start("s-b");
  v.on("change:c");
  v.mod.startPreview("change:c", false);
  await flush();
  assert.deepEqual(v.killed(), ["s-a", "s-b"]);
  await v.start("s-c");
  v.mod.closePreview();
  assert.deepEqual(v.killed(), ["s-a", "s-b", "s-c"]);
  assert.equal(v.created().length, 3);
  assert.equal(v.panes.size, 0, "no pane");
  assert.equal(v.peekEl(), undefined, "no peek left on screen");
  assert.ok(FakeTerminal.all.slice(-3).every((t) => t.disposed));
});

test("a shell command let go after 3 s that prints and exits shows its output on its row", async () => {
  const v = peek(() => ({ kind: "run", script: "slow-diff {target}", cwd: "/w" }));
  v.mod.startPreview("change:a", false);
  await flush();
  await v.start("s1");
  // Quiet for 3 s: held as an app, hidden.
  await v.advance(3_100);
  assert.ok(!v.peekEl() || v.peekEl()!.hidden || v.peekEl()!.cls.has("pending"));
  v.output("s1", "late diff\r\n");
  v.mod.peekExited("s1", 0);
  await v.advance(300);
  const el = v.peekEl()!;
  assert.ok(el && !el.hidden, "its output shows");
  assert.match(v.term().all, /late diff[\s\S]*Press any key/);
  assert.deepEqual(v.killed(), []);
});

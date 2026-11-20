import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, moduleFromSource } from "./workspaceHarness.ts";

/** A Host switch reloads the page. The latency trace keeps running, with the
 *  lines not yet written; only stopTrace() ends it. */
test("the latency trace survives a page reload and stops only on stopTrace", async () => {
  const store = new Map<string, string>();
  const sessionStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  };
  const written: string[] = [];
  const calls: string[] = [];
  let pagehide: (() => void) | null = null;
  const el = () => ({ title: "", isConnected: true, replaceChildren() {}, remove() {}, classList: { toggle() {} } });
  // One page load: a fresh module over the same window storage.
  const load = () =>
    moduleFromSource(
      "../src/diagnostics/latency.ts",
      {
        "@tauri-apps/api/core": {
          invoke: async (cmd: string, args?: { lines?: string[]; on?: boolean }) => {
            calls.push(cmd + (args?.on !== undefined ? `:${args.on}` : ""));
            if (cmd === "trace_info") return ["/tmp/latency.jsonl", false];
            if (cmd === "trace_write") written.push(...(args?.lines ?? []));
            if (cmd === "system_load") return { cpu: null, io: null, load: null };
            return "app debug, skiffd release";
          },
        },
        "./stalls": { startStalls() {}, stopStalls() {}, takeTallies: () => ({ tally: new Map(), steps: new Map() }) },
        "../ui/dom": { h: el, button: el },
        "../app/render": { scheduleRender() {} },
        "../app/state": { S: {}, sessions: new Map() },
        "../i18n": { t: (k: string) => k, tn: (k: string) => k },
      },
      {
        sessionStorage,
        performance,
        navigator: { hardwareConcurrency: 4 },
        document: { querySelector: () => null, querySelectorAll: () => [], createElement: () => ({ getContext: () => null }) },
        window: {
          setInterval: () => 0,
          setTimeout: () => 0,
          addEventListener: (ev: string, f: () => void) => ev === "pagehide" && (pagehide = f),
          removeEventListener: (ev: string) => ev === "pagehide" && (pagehide = null),
        },
        clearInterval() {},
      },
      "",
    );

  const first = load();
  await first.initTrace();
  assert.equal(first.tracing(), false, "off at launch without SKIFF_TRACE");
  first.startTrace();
  first.traceLine({ mark: "before-switch" });
  (pagehide as (() => void) | null)?.();

  const second = load();
  await second.initTrace();
  await flush();
  assert.equal(second.tracing(), true, "a reload keeps the trace on");
  second.stopTrace();
  assert.ok(written.some((l) => l.includes("before-switch")), "the unsent line reaches latency.jsonl");
  assert.ok(calls.includes("trace_set:false"));

  const third = load();
  await third.initTrace();
  assert.equal(third.tracing(), false, "stopTrace() ends it for the next load too");
});

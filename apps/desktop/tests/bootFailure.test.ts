import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

/** A value that answers every property, call and `new` with another one. */
function stub(): any {
  const props = new Map<PropertyKey, unknown>();
  return new Proxy(function () {}, {
    get(_, key) {
      if (key === Symbol.toPrimitive) return () => "";
      if (key === Symbol.iterator) return function* () {};
      if (typeof key === "symbol" || key === "then") return undefined;
      if (!props.has(key)) props.set(key, stub());
      return props.get(key);
    },
    set(_, key, value) {
      props.set(key, value);
      return true;
    },
    apply: () => stub(),
    construct: () => stub(),
  });
}

/** The real main.ts with every import a stub, except the ones a test names. */
function page(over: Record<string, unknown>, globals: Record<string, unknown>) {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8").replace(/import\.meta\.env\.DEV/g, "false");
  const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const mods = new Map<string, unknown>();
  const require = (name: string) => {
    if (Object.hasOwn(over, name)) return over[name];
    if (!mods.has(name)) mods.set(name, stub());
    return mods.get(name);
  };
  vm.runInNewContext(code, { exports: {}, require, document: stub(), setTimeout: () => 0, setInterval: () => 0, console: { error() {}, warn() {} }, ...globals });
}

// A launch whose Host answers late: the capabilities read fails, then the
// event stream fails with it, so boot stops half way. Later clicks reach the
// Host, but nothing runs boot's rest again: no events, no poll, no projects.
test("a boot that fails shows its error with Try again, which runs boot again from the start", async () => {
  const shown: { e: unknown; action?: { label: string; run: () => Promise<void> } }[] = [];
  let reloads = 0;
  page(
    {
      // This Mac's daemon answers; only the Host's own reads fail.
      "@tauri-apps/api/core": { invoke: async () => ({ connected: true }) },
      "./i18n": { t: (key: string) => key },
      "./ui/alerts": { showError: (e: unknown, action?: any) => void shown.push({ e, action }) },
      "./app/hostCaps": { loadHostCaps: () => Promise.reject(new Error("no Host")) },
      "./app/daemon": new Proxy({}, {
        get: (_, key) => (key === "subscribeEvents" ? () => Promise.reject(new Error("no Host")) : key === "then" ? undefined : () => stub()),
      }),
    },
    { location: { reload: () => void reloads++ } },
  );
  for (let i = 0; i < 20 && !shown.length; i++) await new Promise((r) => setImmediate(r));
  assert.equal(shown.length, 1, "the failure shows once");
  assert.match(String((shown[0].e as Error).message), /no Host/);
  assert.ok(shown[0].action, "the error offers a way out of the half-started window");
  assert.equal(shown[0].action.label, "connect.tryAgain");
  await shown[0].action.run();
  assert.equal(reloads, 1, "Try again loads the page again, so boot runs in full");
});

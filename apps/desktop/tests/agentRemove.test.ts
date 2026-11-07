import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource, realControls } from "./workspaceHarness.ts";

// The real rowTable.ts, as Settings › Agents uses it: a row that something
// starts has its − off, and the − says why. Status holds only its word. Any
// other row, default or custom, removes.
class El {
  children: El[] = [];
  listeners = new Map<string, () => void>();
  dataset: Record<string, string> = {};
  classList = { add: (c: string) => (this.className += ` ${c}`), toggle() {} };
  attrs: Record<string, string> = {};
  disabled = false;
  type = "";
  title = "";
  id = "";
  tag: string;
  className: string;
  textContent: string;
  constructor(tag: string, className = "", textContent = "") {
    this.tag = tag;
    this.className = className;
    this.textContent = textContent;
  }
  append(...kids: El[]) { this.children.push(...kids); }
  appendChild(k: El) { this.children.push(k); return k; }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  addEventListener(ev: string, run: () => void) { this.listeners.set(ev, run); }
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
}

function table(rows: { id: string; used_by: string | null }[]) {
  const removed: string[] = [];
  const state = { redraws: 0 };
  const m = moduleFromSource(
    "../src/appearance/rowTable.ts",
    { "../ui/dom": { h: (tag: string, cls = "", text = "") => new El(tag, cls, text) }, "../ui/runes": { rune: () => new El("svg") }, "../ui/controls": realControls((tag: string, cls = "", text = "") => new El(tag, cls, text)) },
    { console, document: { querySelector: () => null, querySelectorAll: () => [] }, CSS: { escape: (s: string) => s }, HTMLInputElement: class {} },
    "",
  );
  const tableState = m.tableState();
  const t: El = m.rowTable(
    {
      grid: "g-agents",
      labelledBy: "x",
      rows,
      id: (a: any) => a.id,
      cols: [{ key: "name", head: "AGENT", name: "name", kind: "key", get: (a: any) => a.id }],
      status: () => new El("span", "c-state", "Installed"),
      kept: (a: any) => a.used_by,
      edit: async () => null,
      remove: async (a: any) => (removed.push(a.id), null),
    },
    tableState,
    () => { state.redraws++; },
  );
  const minus = (id: string) => t.all().find((e) => e.className.includes("c-remove") && t.all().some((r) => r.dataset.id === id && r.all().includes(e)))!;
  const end = (id: string) => t.all().find((r) => r.dataset.id === id)!.children.at(-1)!;
  // No line goes under the row.
  assert.equal(t.children.some((e) => !e.className.startsWith("set-row") && e.className !== "set-error"), false);
  return { minus, end, removed, tableState };
}

test("a used agent keeps its − off, with the reason on the −, not in Status", async () => {
  const { minus, end, removed, tableState } = table([
    { id: "claude", used_by: "Skipper +2" },
    { id: "hermes", used_by: null },
    { id: "aider", used_by: null },
  ]);
  const off = minus("claude");
  // aria-disabled, so the keys reach it. `disabled` would take it out of the tab order.
  assert.equal(off.disabled, false);
  assert.equal(off.attrs["aria-disabled"], "true");
  assert.equal(off.title, "Used by Skipper +2");
  const note = end("claude").children.find((e) => e.id === off.attrs["aria-describedby"]);
  assert.equal(note?.textContent, "Used by Skipper +2");
  assert.equal(note?.className, "visually-hidden");
  // What a sighted person reads in Status is its word alone.
  assert.equal(end("claude").children.filter((e) => e.className !== "visually-hidden" && !e.className.includes("c-remove")).map((e) => e.textContent).join(), "Installed");
  assert.equal(end("hermes").children.map((e) => e.className.includes("c-remove") ? "" : e.textContent).join(""), "Installed");
  // A press says why and removes nothing.
  off.listeners.get("click")!();
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(removed, []);
  assert.equal(tableState.error, "Used by Skipper +2");
  // A default (hermes) and a custom agent (aider) both remove.
  assert.equal(minus("hermes").attrs["aria-disabled"], undefined);
  minus("hermes").listeners.get("click")!();
  minus("aider").listeners.get("click")!();
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(removed, ["hermes", "aider"]);
});

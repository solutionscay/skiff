import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real Flags cell of Settings › Agents. One menu fills the cell, and it
// says what the command does. Any other flag stays in the command, off the
// cell's face: the tooltip, the open menu and a screen reader read it.
class El {
  children: El[] = [];
  attrs: Record<string, string> = {};
  id = "";
  className: string;
  textContent: string;
  title = "";
  constructor(_tag: string, cls = "", text = "") { this.className = cls; this.textContent = text; }
  append(...kids: El[]) { this.children.push(...kids); }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  addEventListener() {}
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

type Pick = { value: string; shows?: string; disabled?: boolean; entries: () => ({ value?: string; head?: string; label?: string; run?: () => void })[]; pick: (v: string) => void };

function cell(agent: unknown) {
  const edits: Record<string, string>[] = [];
  const picks: Pick[] = [];
  const m = moduleFromSource("../src/appearance/settingsAgents.ts", {
    "../ui/dom": { h },
    "../ui/controls": { picker: (o: Pick) => (picks.push(o), h("button", "ctl-pick")), smallButton: () => h("button"), switchButton: () => h("button") },
    "./rowTable": { rowTable: () => h("div"), sectionHead: () => h("div") },
    "./programPicker": { pickProgram() {} },
  }, { Promise }, "");
  const el: El = m.flagsCell({ agents: () => [agent], table: {}, render() {}, edit: async (_c: string, a: Record<string, string>) => (edits.push(a), null) }, agent);
  return { el, picks, edits };
}

// The lists of crates/skiff-core/src/tasks.rs skip_flags, read from the source, so this test follows the table.
const tasks = readFileSync(new URL("../../../crates/skiff-core/src/tasks.rs", import.meta.url), "utf8");
function listed(id: string): string[] {
  const line = tasks.match(new RegExp(`"${id}" => &\\[(.*)\\]`));
  assert.ok(line, id);
  return [...line[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}
const agent = (id: string, command: string, more = {}) => ({ id, command, installed: true, skip_approvals: listed(id), ...more });

test("the menu tells the truth for every spelling of every agent", () => {
  const cases: [string, string, string][] = [
    ["grok", "grok --yolo", "skip"],
    ["grok", "grok --always-approve", "skip"],
    ["grok", "grok --permission-mode bypassPermissions", "skip"],
    ["grok", "grok --permission-mode plan", "ask"],
    ["grok", "grok", "ask"],
    ["gemini", "agy -dangerously-skip-permissions", "skip"],
    ["gemini", "agy --dangerously-skip-permissions", "skip"],
    ["gemini", "agy", "ask"],
    ["claude", "claude --dangerously-skip-permissions", "skip"],
    ["claude", "claude --permission-mode=bypassPermissions", "skip"],
    ["claude", "claude --verbose", "ask"],
    ["codex", "codex --yolo", "skip"],
    ["codex", "codex -a never", "skip"],
    ["codex", "codex -a on-request", "ask"],
    ["opencode", "opencode --auto", "skip"],
    ["opencode", "opencode", "ask"],
  ];
  for (const [id, command, want] of cases) assert.equal(cell(agent(id, command)).picks[0].value, want, command);
});

test("Ask drops every spelling, so the agent asks again", () => {
  for (const [id, command, want] of [
    ["gemini", "agy -dangerously-skip-permissions --dangerously-skip-permissions", "agy"],
    ["grok", "grok --yolo --verbose", "grok --verbose"],
    ["grok", "grok --permission-mode bypassPermissions", "grok"],
  ]) {
    const { picks, edits } = cell(agent(id, command));
    assert.equal(picks[0].value, "skip");
    picks[0].pick("ask");
    assert.equal(edits[0].command, want);
  }
});

test("the menu writes the listed flag and keeps every other flag", () => {
  const { picks, edits } = cell(agent("claude", "claude --verbose"));
  picks[0].pick("skip");
  assert.equal(edits[0].command, "claude --verbose --dangerously-skip-permissions");
});

test("drawing the cell writes nothing", () => {
  for (const command of ["grok --yolo", "grok", "grok --odd-flag --yolo", "  grok   --yolo  "]) assert.deepEqual(cell(agent("grok", command)).edits, []);
});

test("other flags leave the cell's face and stay readable", () => {
  const { el, picks } = cell(agent("claude", "claude --verbose --debug"));
  const menu = el.children[0];
  // One control in the cell. The only other child takes no room.
  assert.deepEqual(el.children.map((c) => c.className), ["ctl-pick", "visually-hidden"]);
  assert.equal(el.all().some((e) => e.className.includes("c-flags-rest")), false);
  assert.ok(menu.title.includes("--verbose --debug"));
  assert.equal(el.children[1].id, menu.attrs["aria-describedby"]);
  assert.ok(el.children[1].textContent.includes("--verbose --debug"));
  assert.ok(picks[0].entries().at(-1)!.head!.includes("--verbose --debug"));
});

test("with no other flag the cell is the menu alone", () => {
  const { el } = cell(agent("grok", "grok --yolo"));
  assert.deepEqual(el.children.map((c) => c.className), ["ctl-pick"]);
  assert.equal(el.children[0].attrs["aria-describedby"], undefined);
});

test("an agent with no listed flag: its flags are in a menu with nothing to pick", () => {
  const { el, picks } = cell({ id: "aider", command: "aider --no-auto-commits", installed: true, custom: true });
  assert.equal(picks[0].disabled, false);
  assert.equal(picks[0].entries().some((e) => "value" in e), false);
  // An item, not a head: a menu with no item would not take the keys, and Esc would not reach it.
  assert.ok(picks[0].entries()[0].label!.includes("--no-auto-commits"));
  assert.equal(typeof picks[0].entries()[0].run, "function");
  assert.equal(el.children[0].title.includes("--no-auto-commits"), true);
});

test("an agent with no flag at all has no menu", () => {
  const { el, picks } = cell({ id: "aider", command: "aider", installed: true, custom: true });
  assert.equal(picks.length, 0);
  assert.equal(el.children.length, 0);
});

test("a missing agent cannot change, and the keys still reach its other flags", () => {
  const off = cell(agent("grok", "grok --yolo", { installed: false }));
  assert.equal(off.picks[0].disabled, true);
  const flags = cell(agent("grok", "grok --yolo --odd", { installed: false }));
  assert.equal(flags.picks[0].disabled, false);
  assert.ok(flags.picks[0].shows);
  assert.equal(flags.picks[0].entries().some((e) => "value" in e), false);
});

test("the CSS gives the menu the whole cell and has no flag text", () => {
  const css = readFileSync(new URL("../src/styles/settings.css", import.meta.url), "utf8");
  assert.equal(css.includes("c-flags-rest"), false);
  assert.match(css, /\.g-agents \.c-flags \.ctl-pick \{ flex: 1 1 0; \}/);
});

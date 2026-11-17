import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real usagePage.ts (#276): what Settings › Usage says for each agent, and
// what it says when the Host has no usage reads at all.
class El {
  className: string;
  textContent: string;
  title = "";
  type = "";
  children: (El | string)[] = [];
  style: Record<string, string> = {};
  constructor(_tag: string, cls = "", text = "") {
    this.className = cls;
    this.textContent = text;
  }
  append(...kids: (El | string)[]) { this.children.push(...kids); }
  setAttribute() {}
  addEventListener() {}
  all(): El[] { return [this, ...this.children.flatMap((c) => (typeof c === "string" ? [] : c.all()))]; }
  text(): string { return this.textContent + this.children.map((c) => (typeof c === "string" ? c : c.text())).join(""); }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

const agent = (id: string, extra: Record<string, unknown> = {}) => ({ id, command: id === "gemini" ? "agy" : id, installed: true, enabled: true, custom: false, ...extra });
const AGENTS = [agent("claude"), agent("codex"), agent("gemini"), agent("grok"), agent("opencode"), agent("copilot", { custom: true }), agent("kimi", { installed: false })];

const model = moduleFromSource("../src/app/usageModel.ts", {}, {}, "");

function page(can: boolean, usage: unknown[] = []) {
  const mod = moduleFromSource("../src/appearance/usagePage.ts", {
    "../ui/dom": { h },
    "../ui/controls": { smallButton: (text: string) => h("button", "ctl-btn", text) },
    "./agentIcon": { launchIcon: () => h("span", "agent-icon") },
    "../app/usage": { readUsage() {}, usage, usageError: null },
    "../app/hostCaps": { hostCan: (cap: string) => cap !== "agent_usage" || can },
    "../app/usageModel": model,
  }, {}, "");
  const root: El = mod.usageSection(AGENTS);
  return { root, count: mod.usageCount(AGENTS) as string, text: root.text(), bands: root.all().filter((e) => e.className === "set-band u-band").map((b) => b.text()) };
}

test("a Host without agent_usage shows its refusal only: no agent rows and no count", () => {
  const p = page(false);
  assert.match(p.text, /This Host does not offer agent usage\./);
  assert.deepEqual(p.bands, []);
  assert.doesNotMatch(p.text, /has no usage command/);
  assert.equal(p.root.all().filter((e) => e.className === "set-row set-cols").length, 0);
  assert.equal(p.count, "");
});

test("a Host with agent_usage: readers wait for their first read, the rest have no usage command", () => {
  const p = page(true);
  assert.doesNotMatch(p.text, /does not offer/);
  // Installed agents only, in Settings › Agents order; Kimi is not installed.
  assert.deepEqual(p.bands, ["Claude Codeclaude", "Codexcodex", "Antigravityagy", "Grokgrok", "OpenCodeopencode", "copilotcopilot"]);
  for (const name of ["Claude Code", "Codex", "Antigravity", "Grok"]) assert.doesNotMatch(p.text, new RegExp(`${name} has no usage command`));
  assert.match(p.text, /No usage read\. OpenCode has no usage command\./);
  assert.match(p.text, /No usage read\. copilot has no usage command\./);
  assert.equal(p.count, "0 of 6");
});

test("a read shows its windows as percent used, and the count names the agents with a reader", () => {
  const now = Date.now();
  const p = page(true, [{ agent: "codex", plan: "Pro Lite", read_at: now, windows: [{ minutes: 10080, used: 2, threshold: 80, resets_at: now + 3_600_000 }], credits: { balance: 75000 }, reading: false }]);
  assert.match(p.text, /Pro Lite · read \d\d:\d\d/);
  assert.match(p.text, /Weekly/);
  assert.match(p.text, /2%/);
  assert.equal(p.root.all().find((e) => e.className === "u-meter")?.title, "2% used · 98% left");
  assert.equal(p.count, "1 of 6");
});

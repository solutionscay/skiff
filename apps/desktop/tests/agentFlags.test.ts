import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real Flags cell of Settings › Agents. A flag Skiff lists is a menu. Any
// other flag stays in the command, and the row shows it read only.
class El {
  children: El[] = [];
  className: string;
  textContent: string;
  title = "";
  constructor(_tag: string, cls = "", text = "") { this.className = cls; this.textContent = text; }
  append(...kids: El[]) { this.children.push(...kids); }
  setAttribute() {}
  addEventListener() {}
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

function cell(agent: unknown) {
  const edits: Record<string, string>[] = [];
  const picks: { value: string; pick: (v: string) => void }[] = [];
  const m = moduleFromSource("../src/appearance/settingsAgents.ts", {
    "../ui/dom": { h },
    "../ui/controls": { picker: (o: (typeof picks)[number]) => (picks.push(o), h("button", "ctl-pick")), smallButton: () => h("button"), switchButton: () => h("button") },
    "./rowTable": { rowTable: () => h("div"), sectionHead: () => h("div") },
    "./programPicker": { pickProgram() {} },
  }, { Promise }, "");
  const el: El = m.flagsCell({ agents: () => [agent], table: {}, render() {}, edit: async (_c: string, a: Record<string, string>) => (edits.push(a), null) }, agent);
  return { el, picks, edits };
}

const claude = (command: string) => ({ id: "claude", command, installed: true, skip_approvals: ["--dangerously-skip-permissions"] });

test("the menu writes the listed flag and keeps every other flag", () => {
  const { el, picks, edits } = cell(claude("claude --verbose"));
  assert.equal(picks[0].value, "ask");
  assert.equal(el.all().find((e) => e.className.includes("c-flags-rest"))?.textContent, "--verbose");
  picks[0].pick("skip");
  assert.equal(edits[0].command, "claude --verbose --dangerously-skip-permissions");
});

test("an alias of the listed flag reads as on, and off drops it", () => {
  const { picks, edits } = cell({ id: "codex", command: "codex --yolo", installed: true, skip_approvals: ["--dangerously-bypass-approvals-and-sandbox", "--yolo"] });
  assert.equal(picks[0].value, "skip");
  picks[0].pick("ask");
  assert.equal(edits[0].command, "codex");
});

test("an agent with no listed flag has no menu: its flags show read only", () => {
  const { el, picks } = cell({ id: "aider", command: "aider --no-auto-commits", installed: true, custom: true });
  assert.equal(picks.length, 0);
  assert.equal(el.all().find((e) => e.className.includes("c-flags-rest"))?.textContent, "--no-auto-commits");
});

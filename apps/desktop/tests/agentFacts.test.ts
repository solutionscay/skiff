import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { moduleFromSource } from "./workspaceHarness.ts";

// The Rust tests in skiff-core `tasks` and skiff-client `identity` read the same file.
const facts = JSON.parse(readFileSync(new URL("../../../crates/skiff-core/tests/fixtures/agent-facts.json", import.meta.url), "utf8")) as {
  model_flag: Record<string, string | null>;
  continue: { agent: string; command: string; line: string | null }[];
};

test("each agent's model flag is the one in the shared table, and a custom agent has none", () => {
  const mod = moduleFromSource("../src/appearance/taskTypes.ts", { "../i18n": { t: (k: string) => k } }, {}, "\nexports.MODEL_FLAGS = MODEL_FLAGS;");
  for (const [id, flag] of Object.entries(facts.model_flag)) {
    assert.equal(mod.modelFlag({ id, custom: false }), flag, id);
    assert.equal(mod.modelFlag({ id, custom: true }), null, `${id}, custom`);
  }
  assert.equal(mod.modelFlag(undefined), null);
  // A flag added on this side only.
  const withFlag = Object.keys(facts.model_flag).filter((id) => facts.model_flag[id]).sort();
  assert.deepEqual(Object.keys(mod.MODEL_FLAGS).sort(), withFlag);
});

test("each agent's continue line is the one in the shared table", () => {
  const mod = moduleFromSource("../src/terminal/resume.ts", {}, {}, "\nexports.CONTINUE = CONTINUE;");
  for (const { agent, command, line } of facts.continue) assert.equal(mod.continueLine(agent, command), line, `${agent}: ${command}`);
  // A recipe added on this side only.
  const withLine = [...new Set(facts.continue.filter((c) => c.line).map((c) => c.agent))].sort();
  assert.deepEqual(Object.keys(mod.CONTINUE).sort(), withLine);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real usageModel.ts: the levels the Usage page and the top bar cell share (#276).
const U = moduleFromSource("../src/app/usageModel.ts", {}, {}, "");

const NOW = Date.UTC(2026, 10, 17, 11, 0);
const H = 3_600_000;
const win = (minutes: number, used: number, extra: Record<string, unknown> = {}) => ({ minutes, used, threshold: minutes <= 300 ? 90 : 80, resets_at: NOW + 20 * H, ...extra });
const agent = (id: string, windows: unknown[]) => ({ agent: id, windows, reading: false });

test("thresholds come from the window length: 90% for 5 hours or less, 80% for longer", () => {
  assert.equal(U.cellOf([agent("gemini", [win(300, 89)])], NOW), null);
  assert.equal(U.cellOf([agent("gemini", [win(300, 91)])], NOW).agent, "gemini");
  assert.equal(U.cellOf([agent("codex", [win(10080, 79)])], NOW), null);
  const near = U.cellOf([agent("codex", [win(10080, 82)])], NOW);
  assert.equal(U.level(near.w), "near");
  assert.equal(near.w.used, 82);
  assert.equal(near.more, 0);
});

test("at 100% the cell is red, and two agents give +1 with both in the tooltip", () => {
  const cell = U.cellOf([agent("codex", [win(10080, 84)]), agent("claude", [win(10080, 100)])], NOW);
  assert.equal(cell.agent, "claude");
  assert.equal(U.level(cell.w), "limit");
  assert.equal(cell.more, 1);
  assert.match(cell.tip, /^Codex weekly: 84% used · 16% left\. Resets /m);
  assert.match(cell.tip, /^Claude Code weekly: 100% used · 0% left\. Resets /m);
});

test("no reset time, a passed reset time and no windows", () => {
  // No reset time: no "Resets" part, and no pace tick.
  const cell = U.cellOf([agent("codex", [win(10080, 100, { resets_at: undefined })])], NOW);
  assert.equal(cell.tip, "Codex weekly: 100% used · 0% left.");
  assert.equal(U.pace(cell.w, NOW), null);
  // A window whose reset time has passed is stale: never in the cell.
  assert.equal(U.cellOf([agent("codex", [win(10080, 100, { resets_at: NOW - 1 })])], NOW), null);
  assert.equal(U.live(win(10080, 100, { resets_at: NOW - 1 }), NOW), false);
  // A read with no window adds nothing.
  assert.equal(U.cellOf([agent("gemini", [])], NOW), null);
});

test("names from the length, and the used and left terms", () => {
  assert.equal(U.windowName(win(300, 0)), "5-hour");
  assert.equal(U.windowName(win(1440, 0)), "Daily");
  assert.equal(U.windowName(win(10080, 0)), "Weekly");
  assert.equal(U.windowName(win(10080, 0, { credits: true })), "Weekly credits");
  assert.equal(U.windowName(win(2880, 0)), "2-day");
  assert.equal(U.inlineName(win(10080, 0), "Fable"), "Fable weekly");
  assert.equal(U.usedLeft(2), "2% used · 98% left");
  // Half a week gone.
  assert.equal(U.pace(win(10080, 0, { resets_at: NOW + 84 * H }), NOW), 0.5);
});

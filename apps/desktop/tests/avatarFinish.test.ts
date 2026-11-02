import { test } from "node:test";
import assert from "node:assert/strict";
import { admiralAvatar, parseFinish, skipperAvatar } from "../src/appearance/skipperAvatar.ts";

// The finish setting: flat unless a known finish is stored, and flat draws no filter.
test("an unknown or missing finish reads as flat", () => {
  for (const v of [null, undefined, "", "fur", "Gummy", 1]) assert.equal(parseFinish(v), "flat");
  for (const v of ["flat", "gummy", "plush"] as const) assert.equal(parseFinish(v), v);
});

test("each finish keeps the blink and turn hooks, and only gummy and plush filter the body", () => {
  for (const f of ["flat", "gummy", "plush"] as const) {
    for (const svg of [skipperAvatar("skiff", 3, f), admiralAvatar(f)]) {
      for (const cls of ["sk-turn", "sk-front", "sk-look", "sk-eyes"]) assert.ok(svg.includes(`class="${cls}"`), `${f}: ${cls}`);
      assert.equal(svg.includes("filter="), f !== "flat", f);
    }
  }
});

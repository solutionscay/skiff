import { test } from "node:test";
import assert from "node:assert/strict";
import * as avatar from "../src/appearance/skipperAvatar.ts";
import { admiralAvatar, FINISHES, parseFinish, skipperAvatar } from "../src/appearance/skipperAvatar.ts";
import { moduleFromSource } from "./workspaceHarness.ts";

// The finish setting: plush unless a known finish is stored, and flat draws no filter.
test("an unknown or missing finish reads as plush, and a stored one stays", () => {
  for (const v of [null, undefined, "", "fur", "Gummy", 1]) assert.equal(parseFinish(v), "plush");
  for (const v of ["flat", "gummy", "plush"] as const) assert.equal(parseFinish(v), v);
});

test("the pick lists plush, gummy, flat", () => {
  assert.deepEqual([...FINISHES], ["plush", "gummy", "flat"]);
});

test("each finish keeps the blink and turn hooks, and only gummy and plush filter the body", () => {
  for (const f of ["flat", "gummy", "plush"] as const) {
    for (const svg of [skipperAvatar("skiff", 3, f), admiralAvatar(f)]) {
      for (const cls of ["sk-turn", "sk-front", "sk-look", "sk-eyes"]) assert.ok(svg.includes(`class="${cls}"`), `${f}: ${cls}`);
      assert.equal(svg.includes("filter="), f !== "flat", f);
    }
  }
});

// Plush is the default, so every pick is stored: a Flat pick survives a restart.
test("a stored pick, flat too, survives a restart; nothing stored reads as plush", () => {
  const store = new Map<string, string>();
  const localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v), removeItem: (k: string) => void store.delete(k) };
  const start = () => moduleFromSource("../src/workspace/skipperFace.ts", { "../appearance/skipperAvatar": avatar, "../app/renderRequest": { render() {} } }, { localStorage }, "");
  assert.equal(start().avatarFinish(), "plush");
  start().setAvatarFinish("flat");
  assert.equal(store.get("skiff.avatarFinish"), "flat");
  assert.equal(start().avatarFinish(), "flat");
  start().setAvatarFinish("plush");
  assert.equal(start().avatarFinish(), "plush");
});

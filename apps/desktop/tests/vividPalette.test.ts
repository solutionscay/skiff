import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { isVivid, vividPalette } from "../src/appearance/colors.ts";

// The Finish sample faces wear only vivid theme colors: no grays, beiges or tans.
const THEMES = new URL("../../../crates/skiff-core/themes/", import.meta.url);
const mains = readdirSync(THEMES).filter((f) => f.endsWith(".toml")).flatMap((f) => {
  const m = /^main_color\s*=\s*"(#[0-9a-fA-F]{6})"/m.exec(readFileSync(new URL(f, THEMES), "utf8"));
  return m ? [m[1].toLowerCase()] : [];
});

test("grays, beiges and tans drop; bright colors stay", () => {
  for (const c of ["#c2c2c2", "#c8bcae", "#eee8d5", "#eccc94", "#e8bf5c", "#ced6d0", "#d2d0b0", "#e4d2ac"]) assert.equal(isVivid(c), false, c);
  for (const c of ["#ff3b3b", "#34d07c", "#4fd4ff", "#b06cff", "#ffcf3f", "#e8a838", "#ff7f2a", "#f4ee8c"]) assert.equal(isVivid(c), true, c);
  for (const c of ["#ffffff", "#000000", "#1a0505"]) assert.equal(isVivid(c), false, c);
});

test("most repo theme colors stay vivid", () => {
  const kept = mains.filter(isVivid);
  assert.ok(kept.length >= 20 && kept.length < mains.length, `${kept.length} of ${mains.length}`);
});

test("with no vivid color, the samples keep the colors they have", () => {
  assert.deepEqual(vividPalette(["#c2c2c2", "#ff3b3b"]), ["#ff3b3b"]);
  assert.deepEqual(vividPalette(["#c2c2c2", "#c8bcae"]), ["#c2c2c2", "#c8bcae"]);
  assert.deepEqual(vividPalette([]), []);
});

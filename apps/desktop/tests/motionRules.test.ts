import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const src = new URL("../src/", import.meta.url).pathname;
const files = (readdirSync(src, { recursive: true }) as string[]).map((f) => join(src, f));
const read = (ext: string) => files.filter((f) => f.endsWith(ext)).map((f) => [f.slice(src.length), readFileSync(f, "utf8")] as const);

test("the only one-off motion uses the tokens, with opacity and transform (#267)", () => {
  for (const [name, css] of read(".css")) {
    assert.doesNotMatch(css, /(^|[\s;{])transition\s*:(?!\s*none)/m, `${name}: no CSS transition. Use ui/motion.ts on a light layer, or no motion`);
    for (const [, value] of css.matchAll(/(?:^|[\s;{])animation\s*:\s*([^;}]+)/gm)) {
      if (value.trim() === "none" || /infinite/.test(value)) continue;
      assert.match(value, /var\(--motion-/, `${name}: "${value}" takes its time from the motion tokens`);
    }
    for (const [, keyframes, body] of css.matchAll(/@keyframes\s+([\w-]+)\s*\{((?:[^{}]*\{[^{}]*\})*)[^{}]*\}/g)) {
      for (const [, prop] of body.matchAll(/([\w-]+)\s*:/g)) {
        if (keyframes === "water") continue;
        assert.ok(["opacity", "transform"].includes(prop), `${name}: @keyframes ${keyframes} animates ${prop}`);
      }
    }
  }
});

test("no terminal, pane or panel animates from code (#267)", () => {
  const animated = read(".ts").filter(([, code]) => /\.animate\(/.test(code)).map(([name]) => name).sort();
  assert.deepEqual(animated, ["workspace/ledgerMotion.ts", "workspace/missionMotion.ts"]);
});

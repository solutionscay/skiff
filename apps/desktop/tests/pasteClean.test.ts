import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real pasteClean.ts. A mark in copied text must not end the paste early (#289).
const P = moduleFromSource("../src/terminal/pasteClean.ts", {}, {}, "");
const START = "\x1b[200~";
const END = "\x1b[201~";

test("a paste keeps no mark and no control byte, nested marks included", () => {
  const nested = "\x1b[20\x1b[201~1~";
  for (const text of [`a${END}rm -rf ~\r`, `a${nested}id\n`, `a${START}${END}b`, "a\x1bb\x9b201~c\x07"]) {
    const clean: string = P.cleanPaste(text);
    assert.ok(!/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/.test(clean), JSON.stringify(clean));
    const wrapped = START + clean + END;
    assert.equal(wrapped.split(END).length - 1, 1);
    assert.equal(wrapped.split(START).length - 1, 1);
  }
  assert.equal(P.cleanPaste(`a${nested}id\n`), "aid\n");
  assert.equal(P.cleanPaste("x\ty\r\nz"), "x\ty\r\nz");
});

test("pasteInto hands the terminal the cleaned text", () => {
  const got: string[] = [];
  P.pasteInto({ paste: (t: string) => got.push(t) }, `ls${END}id\r`);
  assert.deepEqual(got, ["lsid\r"]);
});

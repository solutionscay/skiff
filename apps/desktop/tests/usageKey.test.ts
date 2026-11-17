import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

test("the Usage key is Ctrl+Shift+period, matched on the key code because Shift turns the key into '>'", () => {
  for (const userAgent of ["Macintosh", "Linux"]) {
    const mac = userAgent === "Macintosh";
    const keys = moduleFromSource("../src/app/keys.ts", {}, { navigator: { userAgent } }, "");
    for (const key of [">", ":", "."]) {
      const event = { code: "Period", key, metaKey: mac, ctrlKey: !mac, altKey: false, shiftKey: true };
      assert.equal(keys.actionFor(event), "usage");
    }
    assert.equal(keys.keyLabel("usage"), mac ? "⌘+Shift+." : "Ctrl+Shift+.");
    assert.equal(keys.menuAccel("usage"), "CmdOrCtrl+Shift+Period");
  }
});

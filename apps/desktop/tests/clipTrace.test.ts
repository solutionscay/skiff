import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, moduleFromSource } from "./workspaceHarness.ts";

/** The clipboard trace (#280) logs lengths and hashes, never the text. */
test("the clipboard trace never writes the copied or pasted text", async () => {
  const lines: string[] = [];
  const secret = "hunter2-on-the-remote-Host";
  const mod = moduleFromSource(
    "../src/diagnostics/clipTrace.ts",
    {
      "@tauri-apps/api/core": { invoke: async () => secret },
      "../app/hosts": { isRemote: () => true },
      "./latency": { tracing: () => true, traceLine: (o: object) => lines.push(JSON.stringify(o)) },
    },
    { performance, crypto, window: { setTimeout: () => 0 } },
    "",
  );
  const term = { modes: { mouseTrackingMode: "none" }, buffer: { active: { type: "normal" } } };
  const copy = mod.traceCopy("s1", term, secret);
  copy.tr.path = "gtk";
  copy.end();
  const paste = mod.tracePaste("s1", "key");
  paste.tr.path = "gtk";
  paste.end(secret);
  mod.traceOsc52Refused("s1", secret);
  await flush();
  assert.equal(lines.length, 4, "copy, read-back, paste and the refused OSC 52");
  const readback = lines.map((l) => JSON.parse(l)).find((o) => o.clip === "readback");
  assert.equal(readback.same, true);
  assert.equal(readback.len, secret.length);
  for (const l of lines) assert.ok(!l.includes(secret) && !l.includes("hunter2"), l);
});

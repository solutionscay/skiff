import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real uploadProgress.ts with the real English catalog. A drop that ends
// within SHOW_MS shows nothing; a longer one shows the count and redraws at
// most every REDRAW_MS; it ends with the result line.
const MB = 1024 * 1024;

function rig() {
  let now = 0;
  let next = 1;
  const timers = new Map<number, { at: number; fn: () => void }>();
  const advance = (ms: number) => {
    const until = now + ms;
    for (;;) {
      const due = [...timers.entries()].filter(([, t]) => t.at <= until).sort((a, b) => a[1].at - b[1].at)[0];
      if (!due) break;
      timers.delete(due[0]);
      now = due[1].at;
      due[1].fn();
    }
    now = until;
  };
  const paints: string[] = [];
  const el = {
    className: "",
    parentElement: null as unknown,
    classList: { toggle() {} },
    setAttribute() {},
    remove() {},
    set textContent(v: string) {
      paints.push(v);
    },
  };
  const cell = {
    dataset: { session: "p1" },
    querySelector: () => null,
    append(e: typeof el) {
      e.parentElement = cell;
    },
  };
  const globals = {
    window: {
      setTimeout: (fn: () => void, ms: number) => (timers.set(next, { at: now + ms, fn }), next++),
      clearTimeout: (id?: number) => id !== undefined && timers.delete(id),
    },
    document: { querySelectorAll: () => [cell], createElement: () => el },
    Date: { now: () => now },
  };
  const M = moduleFromSource("../src/terminal/uploadProgress.ts", { "@tauri-apps/api/event": { listen: async () => {} }, "../appearance/skiffFolderDialog": { size: (n: number) => `${(n / MB).toFixed(1)} MB` } }, globals, "");
  return { M, advance, paints };
}

test("the upload line says which file goes, of all, and the bytes", () => {
  const { M } = rig();
  const u = { id: "x", done: 2, count: 6, sent: 4.2 * MB, size: 11 * MB };
  assert.equal(M.sendingLine(u), "Uploading 3 of 6 files (4.2 MB of 11.0 MB)…");
  assert.equal(M.sendingLine({ ...u, done: 0, count: 1, sent: 0, size: 2 * MB }), "Uploading 1 file (0.0 MB of 2.0 MB)…");
  assert.equal(M.doneLine(u), "Uploaded 6 files (11.0 MB).");
  assert.equal(M.failedLine(u), "Upload stopped after 2 of 6 files.");
});

test("a drop that ends within SHOW_MS paints nothing", () => {
  const { M, advance, paints } = rig();
  const line = new M.UploadLine("p1", 6);
  line.update({ id: line.id, done: 0, count: 6, sent: 0, size: 11 * MB });
  advance(M.SHOW_MS - 50);
  line.end(true);
  advance(10_000);
  assert.deepEqual(paints, []);
});

test("a long drop redraws at most every REDRAW_MS, then says it is done", () => {
  const { M, advance, paints } = rig();
  const line = new M.UploadLine("p1", 6);
  const at = (done: number) => ({ id: line.id, done, count: 6, sent: done * 2 * MB, size: 12 * MB });
  line.update(at(0));
  advance(M.SHOW_MS);
  assert.deepEqual(paints, ["Uploading 1 of 6 files (0.0 MB of 12.0 MB)…"]);
  // Three counts within one redraw interval: one redraw, with the last.
  line.update(at(1));
  line.update(at(2));
  line.update(at(3));
  advance(M.REDRAW_MS);
  assert.deepEqual(paints.slice(1), ["Uploading 4 of 6 files (6.0 MB of 12.0 MB)…"]);
  line.end(true);
  assert.equal(paints.at(-1), "Uploaded 6 files (12.0 MB).");
  assert.equal(paints.length, 3);
});

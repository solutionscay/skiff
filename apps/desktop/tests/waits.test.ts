import { test } from "node:test";
import assert from "node:assert/strict";
import { SAME_WAIT_MS, Waits } from "../src/app/waits.ts";

/** A session that waits gets one desktop notification per wait, and only
 *  when its pane does not have the keys. */

test("no notification when the pane has the keys", () => {
  const w = new Waits();
  assert.equal(w.moved("a", "working", 0, true), false);
  assert.equal(w.moved("a", "waiting", 100, true), false);
});

test("one notification per wait across a resize flicker", () => {
  const w = new Waits();
  w.moved("a", "working", 0, false);
  assert.equal(w.moved("a", "waiting", 1000, false), true);
  // A click shows the pane in a cell: the resize clears the prompt for a moment, three times.
  for (const at of [3000, 3500, 4000]) {
    assert.equal(w.moved("a", "working", at, false), false);
    assert.equal(w.moved("a", "waiting", at + 300, false), false);
  }
});

test("a new wait after more than the same-wait time notifies", () => {
  const w = new Waits();
  assert.equal(w.moved("a", "waiting", 0, false), true);
  w.moved("a", "working", 1000, false);
  assert.equal(w.moved("a", "waiting", 1000 + SAME_WAIT_MS + 1, false), true);
});

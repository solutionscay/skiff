import { test } from "node:test";
import assert from "node:assert/strict";
import { BadgeCount, SAME_WAIT_MS, Waits } from "../src/app/waits.ts";

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

/** The app icon's number (#275): a rise at once, a drop after the
 *  same-wait time, nothing without a live link. */
test("badge: rises at once, holds a drop, ignores a resize flicker", () => {
  const b = new BadgeCount();
  b.link(true, 0, 0);
  assert.deepEqual(b.step(0), { send: 0, wait: null });
  b.count(2, 100);
  assert.equal(b.step(100).send, 2);
  b.count(1, 200);
  assert.deepEqual(b.step(200), { send: null, wait: SAME_WAIT_MS });
  b.count(2, 500);
  assert.deepEqual(b.step(500), { send: null, wait: null });
  b.count(1, 1000);
  assert.equal(b.step(1000 + SAME_WAIT_MS - 1).send, null);
  assert.equal(b.step(1000 + SAME_WAIT_MS).send, 1);
});

test("badge: clears when the link drops, follows the list after", () => {
  const b = new BadgeCount();
  b.count(3, 0);
  assert.equal(b.step(0).send, 0);
  b.link(true, 3, 10);
  assert.equal(b.step(10).send, 3);
  b.link(false);
  assert.equal(b.step(20).send, 0);
  b.count(3, 30);
  assert.equal(b.step(30).send, null);
  b.link(true, 1, 40);
  assert.equal(b.step(40).send, 1);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import type { Place } from "../src/app/stored.ts";
import { moduleFromSource } from "./workspaceHarness.ts";

test("saving the focused pane on one Host preserves each other Host's place", () => {
  const legacy: Place = { project: null, session: "local-pane", group: null, picked: false, mode: null };
  const records = new Map([["skiff.place", JSON.stringify(legacy)]]);
  const localStorage = {
    getItem: (key: string) => records.get(key) ?? null,
    setItem: (key: string, value: string) => void records.set(key, value),
  };
  const page = () => moduleFromSource("../src/app/stored.ts", {}, { localStorage, JSON }, "");

  const hostA = page();
  const placeA: Place = { ...legacy, session: "a-pane", group: "a-group", mode: "maximize" };
  hostA.trackPlace("h_a");
  hostA.savePlace(placeA);

  const hostB = page();
  const placeB: Place = { ...legacy, session: "b-pane" };
  hostB.trackPlace("h_b");
  hostB.savePlace(placeB);

  const reopened = page();
  assert.deepEqual(reopened.loadPlace("h_a"), placeA);
  assert.deepEqual(reopened.loadPlace("h_b"), placeB);
  assert.equal(Object.keys(reopened.loadPlace("h_new")).length, 0);
  assert.deepEqual(reopened.loadPlace(null), legacy);
  records.set("skiff.place", JSON.stringify(placeB));
  assert.deepEqual(page().loadPlace(null), legacy);
});

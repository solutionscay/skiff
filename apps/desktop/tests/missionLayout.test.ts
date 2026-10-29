// Mission control's graph: the layout, the strings, the keys' moves and the key map.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as G from "../src/workspace/missionLayout.ts";

const project = (name: string, cards: string[], extra: Partial<G.GProject> = {}): G.GProject => ({ key: `project:${name}`, name, skipper: null, cards, plans: [], folded: false, ...extra });

/** Admiral, a fleet with its Skipper and two projects, a project with its Skipper, a plain project, Other. */
function graph(): G.Graph {
  return {
    admiral: "ad",
    fleets: [{ key: "fleet:Web", name: "Web", skipper: "sk-web", projects: [project("storefront", ["s1", "s2", "s3"], { plans: [{ uid: "p1", title: "Release", cards: ["s1", "s3"], done: 1 }] }), project("checkout", ["s4", "s5"])] }],
    projects: [project("skiff", ["s6", "s7", "s8"], { skipper: "sk-skiff" }), project("gesher", ["s9", "s10"]), project("other", ["s11"])],
  };
}

const overlap = (a: G.Rect, b: G.Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const inside = (a: G.Rect, b: G.Rect) => a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h;

test("the layout keeps cards apart, inside their clusters, under their Skippers", () => {
  const p = G.placeGraph(graph(), {});
  const cards = [...p.cards.entries()];
  for (const [a, ra] of cards) for (const [b, rb] of cards) if (a < b) assert.ok(!overlap(ra, rb), `${a} and ${b} overlap`);
  assert.equal(p.cards.size, 14, "every session, Skipper and Admiral has a card");
  // Each session card sits inside its project's frame; each project frame inside its fleet's.
  const g = graph();
  for (const pr of [...g.fleets[0].projects, ...g.projects]) for (const id of pr.cards) assert.ok(inside(p.cards.get(id)!, p.clusters.get(pr.key)!), `${id} inside ${pr.key}`);
  for (const pr of g.fleets[0].projects) assert.ok(inside(p.clusters.get(pr.key)!, p.clusters.get("fleet:Web")!), `${pr.key} inside the fleet`);
  // The project frames of one fleet do not overlap each other, and the top blocks do not overlap.
  assert.ok(!overlap(p.clusters.get("project:storefront")!, p.clusters.get("project:checkout")!));
  assert.ok(!overlap(p.clusters.get("fleet:Web")!, p.clusters.get("project:skiff")!));
  assert.ok(!overlap(p.clusters.get("project:skiff")!, p.clusters.get("project:gesher")!));
  // Admiral is above everything, a Skipper above what it owns.
  const ad = p.cards.get("ad")!;
  for (const [id, r] of p.cards) if (id !== "ad") assert.ok(r.y >= ad.y + ad.h, `${id} under Admiral`);
  assert.ok(p.cards.get("sk-web")!.y + G.CARD_H <= p.clusters.get("project:storefront")!.y);
  assert.ok(p.cards.get("sk-skiff")!.y + G.CARD_H <= p.clusters.get("project:skiff")!.y);
  // A plan caption sits above its first card, inside the cluster.
  const plan = p.plans.find((pl) => pl.plan.uid === "p1")!;
  assert.ok(plan.y + plan.h <= p.cards.get("s1")!.y);
  assert.ok(inside(plan, p.clusters.get("project:storefront")!));
});

test("a folded cluster is one card, and its sessions stand in by it", () => {
  const g = graph();
  g.projects[1].folded = true;
  const p = G.placeGraph(g, {});
  const c = p.clusters.get("project:gesher")!;
  assert.equal(c.folded, true);
  assert.deepEqual([c.w, c.h], [G.CARD_W, G.CARD_H]);
  assert.equal(p.cards.has("s9"), false);
  assert.equal(G.standIn(g, p, "s9"), "project:gesher");
  assert.equal(G.standIn(g, p, "s1"), "s1");
  assert.equal(G.navChildren(g, "project:gesher").length, 0, "a folded cluster has nothing under it");
});

test("a hand offset moves a card or a cluster with its contents, and nothing else", () => {
  const base = G.placeGraph(graph(), {});
  const moved = G.placeGraph(graph(), { "project:skiff": { dx: 100, dy: -30 }, s4: { dx: 7, dy: 7 } });
  for (const id of ["sk-skiff", "s6", "s7", "s8"]) {
    assert.equal(moved.cards.get(id)!.x, base.cards.get(id)!.x + 100, id);
    assert.equal(moved.cards.get(id)!.y, base.cards.get(id)!.y - 30, id);
  }
  assert.equal(moved.clusters.get("project:skiff")!.x, base.clusters.get("project:skiff")!.x + 100);
  assert.deepEqual(moved.cards.get("s4"), { ...base.cards.get("s4")!, x: base.cards.get("s4")!.x + 7, y: base.cards.get("s4")!.y + 7 });
  for (const id of ["ad", "sk-web", "s1", "s5", "s9", "s11"]) assert.deepEqual(moved.cards.get(id), base.cards.get(id), `${id} stays`);
});

test("the keys move along the tree: parent, children, siblings", () => {
  const g = graph();
  assert.deepEqual(G.navChildren(g, "ad"), ["sk-web", "sk-skiff", "project:gesher", "project:other"]);
  assert.deepEqual(G.navChildren(g, "sk-web"), ["project:storefront", "project:checkout"]);
  assert.deepEqual(G.navChildren(g, "sk-skiff"), ["project:skiff"]);
  assert.deepEqual(G.navChildren(g, "project:storefront"), ["s1", "s2", "s3"]);
  assert.equal(G.navParent(g, "s4"), "project:checkout");
  assert.equal(G.navParent(g, "project:checkout"), "sk-web");
  assert.equal(G.navParent(g, "sk-web"), "ad");
  assert.equal(G.navParent(g, "project:skiff"), "sk-skiff");
  assert.equal(G.navParent(g, "project:gesher"), "ad");
  assert.equal(G.navParent(g, "ad"), null);
  assert.equal(G.navSibling(g, "s1", 1), "s2");
  assert.equal(G.navSibling(g, "s1", -1), "s1", "the first stays at the start");
  assert.equal(G.navSibling(g, "s3", 1), "s3", "the last stays at the end");
  assert.equal(G.navSibling(g, "sk-skiff", 1), "project:gesher");
  assert.equal(G.navSibling(g, "project:storefront", 1), "project:checkout");
  // Up then Down comes back to the first child; every node's parent lists it.
  for (const id of ["s1", "s4", "s6", "s9", "s11", "project:storefront", "project:gesher", "sk-web", "sk-skiff"]) {
    const up = G.navParent(g, id)!;
    assert.ok(G.navChildren(g, up).includes(id), `${up} lists ${id}`);
  }
  // Without Admiral, the top blocks are each other's siblings and have no parent.
  const alone: G.Graph = { ...g, admiral: null };
  assert.equal(G.navFirst(alone), "sk-web");
  assert.equal(G.navParent(alone, "sk-web"), null);
  assert.equal(G.navSibling(alone, "sk-web", 1), "sk-skiff");
  // A fleet with no Skipper is its own node.
  const noSk: G.Graph = { ...g, fleets: [{ ...g.fleets[0], skipper: null }] };
  assert.deepEqual(G.navChildren(noSk, "ad")[0], "fleet:Web");
  assert.equal(G.navParent(noSk, "project:checkout"), "fleet:Web");
});

test("the key map: plain keys on the board, the caret's keys in the search, action keys everywhere", () => {
  const none = { ctrl: false, shift: false, alt: false, meta: false };
  const on = (key: string, action: string | null = null, inSearch = false, text = false) => G.boardCommand(key, action, inSearch, text, none);
  assert.equal(on("ArrowDown"), "child");
  assert.equal(on("ArrowUp"), "parent");
  assert.equal(on("ArrowRight"), "next");
  assert.equal(on("ArrowLeft"), "prev");
  assert.equal(on(" "), "fold");
  assert.equal(on("Enter"), "go");
  assert.equal(on("Home"), "fit");
  assert.equal(on("="), "zoom-in");
  assert.equal(on("-"), "zoom-out");
  assert.equal(on("Escape"), "close");
  assert.equal(on("Escape", null, false, true), "clear-search", "the first Esc clears the search");
  assert.equal(on("Escape", null, true, true), "clear-search");
  // In the search, Left, Right, Space and Home move the caret; Up and Down still move on the board.
  for (const k of ["ArrowLeft", "ArrowRight", " ", "Home", "-"]) assert.equal(on(k, null, true), null, k);
  assert.equal(on("ArrowDown", null, true), "child");
  assert.equal(on("Enter", null, true), "go");
  // The action keys keep their meaning in both places.
  const acts: [string, G.BoardCommand][] = [["palette", "search"], ["mission-control", "close"], ["maximize", "go-max"], ["focus-mode", "running"], ["next-waiting", "next-waiting"], ["ledger", "panel"], ["project-menu", "menu"], ["settings", "settings"], ["skiff-agent", "skipper"], ["session-next", "child"], ["session-prev", "parent"], ["list-project", "prev"], ["list-back", "next"]];
  for (const [a, cmd] of acts) {
    assert.equal(on("x", a), cmd, a);
    assert.equal(on("x", a, true), cmd, `${a} in the search`);
  }
  // A key with a modifier that is no action is not the board's.
  assert.equal(G.boardCommand("ArrowDown", null, false, false, { ...none, ctrl: true }), null);
  assert.equal(on("a"), null);
});

test("the view: fit, zoom at a point, center, the edge tick", () => {
  const box = { x: 100, y: 50, w: 2000, h: 500 };
  const v = G.fitView(box, 1000, 600, 0);
  assert.equal(v.z, 0.5, "the width limits the fit");
  assert.equal(Math.round(v.x + box.x * v.z), 0);
  assert.equal(Math.round(v.y + box.y * v.z), 175, "centered on the short axis");
  assert.equal(G.fitView({ x: 0, y: 0, w: 100, h: 100 }, 1000, 600, 0).z, 1, "never past 100%");
  // Zooming around a point keeps that point still.
  const z = G.zoomView({ x: 10, y: 20, z: 1 }, 2, 300, 200);
  assert.equal(z.z, 2);
  assert.equal(300 - (300 - 10) * 2, z.x);
  assert.equal(G.zoomView({ x: 0, y: 0, z: 1.9 }, 2, 0, 0).z, G.ZOOM_MAX);
  assert.equal(G.zoomView({ x: 0, y: 0, z: 0.25 }, 0.1, 0, 0).z, G.ZOOM_MIN);
  const c = G.centerOn({ x: 0, y: 0, z: 1 }, { x: 100, y: 100, w: 200, h: 100 }, 1000, 600);
  assert.deepEqual(c, { z: 1, x: 300, y: 150 });
  assert.equal(G.inView(c, { x: 100, y: 100, w: 200, h: 100 }, 1000, 600), true);
  assert.equal(G.inView(c, { x: -300, y: 100, w: 200, h: 100 }, 1000, 600), false);
  assert.equal(G.edgeTick(c, { x: 100, y: 100, w: 200, h: 100 }, 1000, 600), null, "a card in view has no tick");
  assert.deepEqual(G.edgeTick(c, { x: -2000, y: 100, w: 200, h: 100 }, 1000, 600), { x: 0, y: 295 }, "a card to the left ticks at the left edge");
  assert.deepEqual(G.edgeTick(c, { x: 100, y: 5000, w: 200, h: 100 }, 1000, 600), { x: 495, y: 590 });
});

test("time in state reads as minutes, hours and days", () => {
  const now = 1_700_000_000_000;
  assert.equal(G.ago(now - 20_000, now), "now");
  assert.equal(G.ago(now - 4 * 60_000, now), "4m");
  assert.equal(G.ago(now - 125 * 60_000, now), "2h");
  assert.equal(G.ago(now - 3 * 1440 * 60_000, now), "3d");
});

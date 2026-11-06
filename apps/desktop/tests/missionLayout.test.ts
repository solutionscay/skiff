// Mission control's graph: the layout, the connectors, the keys' moves and the key map.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as G from "../src/workspace/missionLayout.ts";
import * as Grid from "../src/workspace/missionControl/grid.ts";

const project = (name: string, cards: string[], extra: Partial<G.GProject> = {}): G.GProject => ({ key: `project:${name}`, name, skipper: null, cards, plans: [], ...extra });

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
  const g = graph();
  const p = Grid.placeGraph(g);
  const cards = [...p.cards.entries()];
  for (const [a, ra] of cards) for (const [b, rb] of cards) if (a < b) assert.ok(!overlap(ra, rb), `${a} and ${b} overlap`);
  assert.equal(p.cards.size, 14, "every session, Skipper and Admiral has a card");
  // Each session sits inside its project's frame; each project frame of a fleet inside the fleet's.
  for (const pr of [...g.fleets[0].projects, ...g.projects]) for (const id of pr.cards) assert.ok(inside(p.cards.get(id)!, p.clusters.get(pr.key)!), `${id} inside ${pr.key}`);
  for (const pr of g.fleets[0].projects) assert.ok(inside(p.clusters.get(pr.key)!, p.clusters.get("fleet:Web")!), `${pr.key} inside the fleet`);
  assert.ok(!overlap(p.clusters.get("project:storefront")!, p.clusters.get("project:checkout")!));
  // Admiral at the top left; a Skipper's window above the work that hangs under it, which is indented past its face.
  assert.deepEqual([p.cards.get("ad")!.x, p.cards.get("ad")!.y], [0, 0]);
  for (const [id, r] of p.cards) if (id !== "ad") assert.ok(r.y >= G.WINDOW.admiral.h, `${id} under Admiral`);
  const web = p.cards.get("sk-web")!;
  assert.ok(web.y + web.h <= p.clusters.get("project:storefront")!.y);
  assert.ok(p.cards.get("sk-skiff")!.y + G.WINDOW.project.h <= p.cards.get("s6")!.y);
  assert.ok(p.cards.get("s6")!.x > p.at.get("s6")!.rail, "the work hangs right of the rail");
  // A plan caption sits above its first card, inside the project.
  const plan = p.plans.find((pl) => pl.plan.uid === "p1")!;
  assert.ok(plan.y + plan.h <= p.cards.get("s1")!.y);
  assert.ok(inside(plan, p.clusters.get("project:storefront")!));
});

test("the clusters flow in a fixed grid: ceil(√n) to a row, at most 4, wrapping like text, in the tree's order", () => {
  assert.deepEqual([1, 2, 4, 5, 9, 10, 30].map(Grid.perRow), [1, 2, 2, 3, 3, 4, 4]);
  // Four clusters (a fleet, a project with a Skipper, a plain project, Other): two to a row, in that order.
  const p = Grid.placeGraph(graph());
  const c = (k: string) => p.clusters.get(k)!;
  const [web, skiff, gesher, other] = [c("fleet:Web"), c("project:skiff"), c("project:gesher"), c("project:other")];
  assert.equal(web.y, skiff.y, "a row top-aligns");
  assert.ok(skiff.x > web.x);
  assert.deepEqual([gesher.x, other.x], [web.x, skiff.x], "the next row starts at the left, on the same columns");
  assert.equal(gesher.y, web.y + Math.max(web.h, skiff.h) + 64, "a row is as tall as its tallest cluster");
  assert.ok([web, skiff, gesher, other].every((x) => x.w === web.w), "every cell is one width");
  // The trunk runs left of every cluster.
  assert.ok([web, skiff, gesher, other].every((x) => x.x > p.trunk!));
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
  assert.equal(on(" "), null);
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

test("a connector: down the trunk and the bus into a Skipper's face, down a rail into a card, or the bus to another cluster's rail", () => {
  const p = Grid.placeGraph(graph());
  const face = (id: string) => G.faceBox(p.cards.get(id)!, p.rungs.get(id)!);
  const mid = (id: string) => ({ x: face(id).x + face(id).w / 2, y: face(id).y + face(id).h / 2 });
  const ad = face("ad");
  const toWeb = Grid.route(p, "ad", "sk-web")!;
  assert.ok(toWeb.startsWith(`M${p.trunk} ${ad.y + ad.h - 4}`), "from the bottom of Admiral's face, on the trunk");
  assert.ok(toWeb.endsWith(`L${mid("sk-web").x} ${mid("sk-web").y}`), "into the middle of the Skipper's face");
  assert.ok(toWeb.includes(` ${p.at.get("sk-web")!.bus}`), "along the row's bus");
  const s4 = p.cards.get("s4")!;
  const own = Grid.route(p, "sk-web", "s4")!;
  assert.ok(own.startsWith(`M${mid("sk-web").x} `), "down the Skipper's own rail");
  assert.ok(own.endsWith(`L${s4.x} ${s4.y + s4.h / 2}`), "into the card's left edge");
  // Admiral's own task in a cluster with no Skipper: the bus, then that cluster's rail.
  assert.ok(Grid.route(p, "ad", "s9")!.includes(`${p.at.get("s9")!.rail} `));
  assert.equal(Grid.route(p, "ad", "nobody"), null);
  // Right angles only: every move is along x or along y, turned with a quarter curve.
  assert.ok(!/[CA]/.test(own + toWeb));
});

/** The corner points of a connector's path data: M, L and the control point of each Q. */
function corners(d: string): { x: number; y: number }[] {
  const n = d.match(/[MLQ][^MLQ]*/g)!.map((s) => ({ c: s[0], v: s.slice(1).trim().split(/[ ,]+/).map(Number) }));
  return n.map(({ v }) => ({ x: v[0], y: v[1] }));
}
/** A straight segment through the open inside of `r`. */
const through = (a: { x: number; y: number }, b: { x: number; y: number }, r: G.Rect) =>
  Math.max(a.x, b.x) > r.x && Math.min(a.x, b.x) < r.x + r.w && Math.max(a.y, b.y) > r.y && Math.min(a.y, b.y) < r.y + r.h;

test("a connector never crosses a window that is not its parent or its child", () => {
  // Admiral, a fleet with its Skipper and project Skippers, a project with its Skipper, plain projects: Admiral's own tasks reach every card.
  const g: G.Graph = {
    admiral: "ad",
    fleets: [
      { key: "fleet:Web", name: "Web", skipper: "sk-web", projects: [project("storefront", ["s1", "s2"], { skipper: "sk-store" }), project("checkout", ["s4", "s5"], { skipper: "sk-check" }), project("docs", ["s12"])] },
      { key: "fleet:Moss", name: "Moss", skipper: "sk-moss", projects: [project("gesher", ["s13", "s14"])] },
      { key: "fleet:Sage", name: "Sage", skipper: null, projects: [project("forecast", ["s15"], { skipper: "sk-fc" })] },
    ],
    projects: [project("skiff", ["s6", "s7", "s8"], { skipper: "sk-skiff" }), project("gesher2", ["s9", "s10"]), project("other", ["s11"])],
  };
  const p = Grid.placeGraph(g);
  const windows = [...p.rungs.keys()];
  let routes = 0;
  const check = (from: string, to: string) => {
    const d = Grid.route(p, from, to);
    if (!d) return;
    routes++;
    const pts = corners(d);
    for (const w of windows) {
      if (w === from || w === to) continue;
      for (let i = 1; i < pts.length; i++) assert.ok(!through(pts[i - 1], pts[i], p.cards.get(w)!), `${from} > ${to} crosses the window of ${w}`);
    }
  };
  for (const to of p.cards.keys()) {
    if (to === "ad") continue;
    check("ad", to);
    for (const owner of p.at.get(to)!.owners) check(owner, to);
  }
  assert.ok(routes > 30, "the repro layout has connectors to check");
});

/** The straight runs of a connector, as pairs of corner points. */
const segs = (d: string) => corners(d).slice(1).map((b, i, all) => [i ? all[i - 1] : corners(d)[0], b] as const);
/** Two runs on one line that share more than `min` px. */
function collinear(a: readonly [G.Rect | { x: number; y: number }, { x: number; y: number }], b: typeof a, min = 4) {
  const [a0, a1] = a as [{ x: number; y: number }, { x: number; y: number }];
  const [b0, b1] = b as [{ x: number; y: number }, { x: number; y: number }];
  if (a0.x === a1.x && b0.x === b1.x && a0.x === b0.x) return Math.min(Math.max(a0.y, a1.y), Math.max(b0.y, b1.y)) - Math.max(Math.min(a0.y, a1.y), Math.min(b0.y, b1.y)) > min;
  if (a0.y === a1.y && b0.y === b1.y && a0.y === b0.y) return Math.min(Math.max(a0.x, a1.x), Math.max(b0.x, b1.x)) - Math.max(Math.min(a0.x, a1.x), Math.min(b0.x, b1.x)) > min;
  return false;
}

test("no connector crosses a window, a card or a nested project at any level: Admiral's, a fleet Skipper's, a project Skipper's", () => {
  // Issue #255: a fleet with two project clusters, each with a project Skipper and cards, a fleet with its Skipper, two rows.
  // The upper project's Skipper has tasks in the lower row (Other), the way its line ran down through the lower Skipper's face.
  const g: G.Graph = {
    admiral: "ad",
    fleets: [
      { key: "fleet:Skiff", name: "Skiff", skipper: null, projects: [project("skiff", ["a1", "a2", "a3"], { skipper: "sk-skiff" }), project("web", ["b1", "b2"], { skipper: "sk-web" })] },
      { key: "fleet:Cay", name: "Cay", skipper: "sk-cay", projects: [project("vault", ["c1"], { skipper: "sk-vault" }), project("biz", ["c2", "c3"], { skipper: "sk-biz" }), project("tool", ["c4"])] },
      { key: "fleet:Moss", name: "Moss", skipper: "sk-moss", projects: [project("gesher", ["d1", "d2"])] },
    ],
    projects: [project("hsd", ["e1"], { skipper: "sk-hsd" }), project("other", ["o1", "o2"])],
  };
  const p = Grid.placeGraph(g);
  const nested = [...g.fleets.flatMap((f) => f.projects)].map((pr) => ({ key: pr.key, ids: new Set([pr.skipper, ...pr.cards].filter(Boolean)) }));
  const sources = [...p.rungs.keys()];
  const lines: { from: string; to: string; d: string }[] = [];
  for (const from of sources) for (const to of p.cards.keys()) {
    if (to === from || to === "ad") continue;
    const d = Grid.route(p, from, to);
    if (d) lines.push({ from, to, d });
  }
  const bad: string[] = [];
  for (const { from, to, d } of lines) {
    const s = segs(d);
    for (const [w, r] of p.cards) if (w !== from && w !== to && s.some(([a, b]) => through(a, b, r))) bad.push(`${from} > ${to} crosses ${w}`);
    for (const n of nested) if (!n.ids.has(from) && !n.ids.has(to) && s.some(([a, b]) => through(a, b, p.clusters.get(n.key)!))) bad.push(`${from} > ${to} runs through ${n.key}`);
  }
  assert.deepEqual(bad.slice(0, 8), [], `${bad.length} crossings`);
  // Lines that run together stay apart: an Admiral line and another Skipper's never share a run, but where they end in the same window.
  const ad = lines.filter((l) => l.from === "ad");
  const others = lines.filter((l) => l.from !== "ad");
  for (const o of others) for (const a of ad) {
    if (a.to === o.to) continue;
    const so = segs(o.d);
    const sa = segs(a.d);
    for (const x of so.slice(0, -1)) assert.ok(!sa.slice(0, -1).some((y) => collinear(x, y)), `${o.from} > ${o.to} runs on ${a.from} > ${a.to}`);
  }
  assert.ok(lines.length > 150, `the repro layout has connectors to check: ${lines.length}`);
});

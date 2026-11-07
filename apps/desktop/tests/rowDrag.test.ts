// The tree and Skippers-list drags, each driven through real mouse and key sequences:
// the real module over a small DOM, a window that keeps capture flags, and held timers.
import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

type Box = { top: number; bottom: number; left: number; right: number; height: number };
const boxAt = (top: number, height = 40): Box => ({ top, bottom: top + height, left: 0, right: 200, height });

class El {
  cls: Set<string>;
  dataset: Record<string, string> = {};
  children: El[] = [];
  parent: El | null = null;
  style: Record<string, string> = {};
  hidden = false;
  box = boxAt(0);
  classList = {
    add: (...c: string[]) => c.forEach((x) => this.cls.add(x)),
    remove: (...c: string[]) => c.forEach((x) => this.cls.delete(x)),
  };
  textContent: string;
  constructor(cls = "", text = "") {
    this.textContent = text;
    this.cls = new Set(cls.split(" ").filter(Boolean));
  }
  get parentElement() { return this.parent; }
  /** A ghost grows with a label that shows, so the label must be set before the ghost is placed. */
  get offsetHeight() { return this.children.some((c) => c.cls.has("tree-ghost-label") && !c.hidden) ? 40 : 20; }
  append(...kids: El[]) { for (const k of kids) { this.children.push(k); k.parent = this; } }
  appendChild(k: El) { this.append(k); return k; }
  remove() {
    if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this);
    this.parent = null;
  }
  getBoundingClientRect() { return this.box; }
  *all(): Generator<El> { for (const c of this.children) { yield c; yield* c.all(); } }
  /** The last class of each comma part: enough for the selectors these two modules write. */
  closest(sel: string): El | null {
    const want = sel.split(",").map((s) => s.trim().split(/[ >]+/).pop()!.slice(1));
    for (let el: El | null = this; el; el = el.parent) if (want.some((c) => el!.cls.has(c))) return el;
    return null;
  }
  querySelector(sel: string): El | null {
    const scoped = sel.match(/^:scope > \.(.+)$/);
    if (scoped) return this.children.find((c) => c.cls.has(scoped[1])) ?? null;
    return this.querySelectorAll(sel)[0] ?? null;
  }
  querySelectorAll(sel: string): El[] {
    const key = sel.match(/\[data-key="(.+)"\]/)?.[1];
    if (key) return [...this.all()].filter((e) => e.dataset.key === key);
    const kids = sel.match(/\[data-kids="(.+)"\]/)?.[1];
    if (kids) return [...this.all()].filter((e) => e.dataset.kids === kids);
    const want = sel.split(",").map((s) => s.trim().split(/[ >]+/).pop()!.slice(1));
    return [...this.all()].filter((e) => want.some((c) => e.cls.has(c)));
  }
}

const ev = (x: number, y: number, more: object = {}) => ({ clientX: x, clientY: y, button: 0, target: new El(), ...more });

/** A page: window listeners with their capture flags, held timers, and a log with the state at each call. */
function page() {
  const body = new El("body-el");
  const scroll = new El("sidebar");
  body.append(scroll);
  const listeners: { type: string; fn: (e: any) => void; capture: boolean }[] = [];
  const timers: { run: () => void; ms: number }[] = [];
  const treeDrag = { active: false };
  let under: El | null = null;
  const window = {
    addEventListener: (type: string, fn: (e: any) => void, capture = false) => void listeners.push({ type, fn, capture }),
    removeEventListener(type: string, fn: (e: any) => void, capture = false) {
      const i = listeners.findIndex((l) => l.type === type && l.fn === fn && l.capture === capture);
      if (i >= 0) listeners.splice(i, 1);
    },
  };
  const document = {
    body,
    elementFromPoint: () => under,
    querySelector: (s: string) => body.querySelector(s),
    querySelectorAll: (s: string) => body.querySelectorAll(s),
  };
  const fire = (type: string, e: any) => {
    const list = listeners.filter((l) => l.type === type);
    for (const l of [...list.filter((x) => x.capture), ...list.filter((x) => !x.capture)]) l.fn(e);
  };
  const ghost = () => [...body.all()].find((e) => e.cls.has("tree-ghost")) ?? null;
  const log: string[] = [];
  /** What a call sees of the gesture when it runs: render must come after the cleanup. */
  const note = (what: string) => log.push(`${what} active=${treeDrag.active} body=${body.cls.has("dragging-tree")} ghost=${!!ghost()} listeners=${listeners.filter((l) => l.type !== "click").length}`);
  const key = (k: string) => {
    const e = { key: k, prevented: 0, stopped: 0, preventDefault() { this.prevented++; }, stopPropagation() { this.stopped++; } };
    fire("keydown", e);
    return e;
  };
  /** A click after the release: true when it reaches the row. */
  const clickReaches = () => {
    let stopped = false;
    fire("click", { stopPropagation: () => (stopped = true) });
    return !stopped;
  };
  const clean = (source: El) => {
    timers.splice(0).forEach((t) => t.run());
    assert.deepEqual(listeners.map((l) => l.type), []);
    assert.equal(treeDrag.active, false);
    assert.equal(body.cls.has("dragging-tree"), false);
    assert.equal(source.cls.has("dragging"), false);
    assert.equal(ghost(), null);
    assert.deepEqual([...body.all()].filter((e) => ["drop-before", "drop-after", "drop-into"].some((c) => e.cls.has(c))), []);
  };
  const globals = {
    window,
    document,
    CSS: { escape: (s: string) => s },
    setTimeout: (run: () => void, ms: number) => timers.push({ run, ms }),
  };
  const dom = { h: (_tag: string, cls = "", text = "") => new El(cls, text) };
  return { body, scroll, listeners, timers, treeDrag, fire, key, clickReaches, clean, ghost, log, note, globals, dom, over: (el: El | null) => (under = el) };
}

// ---- The tree

function tree(o: { hostless?: boolean; made?: string | null } = {}) {
  const pg = page();
  const a = { name: "a" } as any;
  const b = { name: "b" } as any;
  const c = { name: "c", fleet: "F" } as any;
  const d = { name: "d", fleet: "G" } as any;
  const S = { projects: [a, b, c, d] };
  const row = (p: any, top: number) => {
    const block = new El("project-block");
    const head = new El("project-row");
    head.dataset.project = p.name;
    head.box = boxAt(top);
    block.append(head);
    pg.scroll.append(block);
    return { block, head };
  };
  const fleetHead = (name: string, top: number) => {
    const head = new El("tree-head");
    const pick = new El("fleet-pick");
    pick.dataset.key = `fleet:${name}`;
    head.append(pick);
    head.box = boxAt(top);
    pg.scroll.append(head);
    return head;
  };
  const projectsHead = new El("tree-head");
  projectsHead.box = boxAt(0);
  pg.scroll.append(projectsHead);
  const rows = { a: row(a, 40), b: row(b, 80) };
  const fHead = fleetHead("F", 120);
  const rc = row(c, 160);
  const gHead = fleetHead("G", 200);
  const rd = row(d, 240);
  const dropped: any[] = [];
  const mod = moduleFromSource("../src/workspace/treeDrag.ts", {
    "./fleets": {
      applyDrop: (moving: any[], drop: any) => {
        pg.note("applyDrop");
        dropped.push({ moving, drop });
        return o.made ?? null;
      },
      nameFleet: (name: string) => pg.note(`nameFleet ${name}`),
    },
    "./tree": { treeDrag: pg.treeDrag },
    "../app/state": { S },
    "../app/render": { render: (...args: unknown[]) => pg.note(`render(${args.join()})`) },
    "../ui/dom": pg.dom,
  }, pg.globals, "");
  const down = (y = 60, more: object = {}) => mod.dragTree(ev(10, y, more), rows.a.block, [a], null);
  return { ...pg, S, a, b, c, d, rows, rc, rd, fHead, gHead, projectsHead, mod, dropped, down };
}

test("tree: a press that does not move stays a click, and its release still renders", () => {
  const t = tree();
  t.down();
  assert.deepEqual(t.listeners.map((l) => [l.type, l.capture]), [["mousemove", false], ["mouseup", false], ["keydown", true]]);
  t.fire("mouseup", {});
  assert.deepEqual(t.log, ["render() active=false body=false ghost=false listeners=0"]);
  assert.equal(t.dropped.length, 0);
  assert.equal(t.timers.length, 0, "no click is swallowed");
  assert.equal(t.clickReaches(), true);
  t.clean(t.rows.a.block);
});

test("tree: a press that does not start: wrong button, one project, or a + button", () => {
  const t = tree();
  t.down(60, { button: 2 });
  t.down(60, { target: new El("wt-plus") });
  t.down(60, { target: new El("tree-head-add") });
  assert.equal(t.listeners.length, 0);
  t.S.projects = [t.a];
  t.down();
  assert.equal(t.listeners.length, 0);
});

test("tree: under five pixels of vertical move nothing starts, and five starts it", () => {
  const t = tree();
  t.down(60);
  t.over(t.rows.b.head);
  t.fire("mousemove", ev(500, 64));
  t.fire("mousemove", ev(500, 56));
  assert.equal(t.treeDrag.active, false, "4px each way, and any sideways move, is still a click");
  assert.equal(t.ghost(), null);
  assert.equal(t.body.cls.has("dragging-tree"), false);
  t.fire("mousemove", ev(10, 55));
  assert.equal(t.treeDrag.active, true);
  assert.equal(t.body.cls.has("dragging-tree"), true);
  assert.equal(t.rows.a.block.cls.has("dragging"), true);
  assert.ok(t.ghost());
  t.key("Escape");
  t.clean(t.rows.a.block);
});

test("tree: drop on a lone project makes a fleet: ghost, label, mark, then the drop after the cleanup", () => {
  const t = tree({ made: "New" });
  t.down();
  t.over(t.rows.b.head);
  t.fire("mousemove", ev(50, 100));
  const g = t.ghost()!;
  assert.equal(g.children[0].textContent, "a");
  assert.equal(g.style.left, "62px");
  assert.equal(g.style.top, "80px");
  assert.equal(g.children[1].textContent, "Make fleet");
  assert.equal(g.children[1].hidden, false);
  assert.equal(t.rows.b.head.cls.has("drop-into"), true);
  assert.equal(t.log.length, 0, "nothing runs until the release");
  t.fire("mouseup", {});
  assert.deepEqual(t.log, ["applyDrop active=false body=false ghost=false listeners=0", "nameFleet New active=false body=false ghost=false listeners=0"]);
  const { moving, drop } = t.dropped[0];
  assert.equal(moving[0], t.a);
  assert.equal(drop.anchor, t.b);
  assert.equal(drop.make, t.b);
  assert.equal(drop.fleet, null);
  assert.equal(drop.side, "after");
  assert.equal(t.rows.b.head.cls.has("drop-into"), false);
  // The click of the release stops at the window once, in the zero-delay window.
  assert.deepEqual(t.timers.map((x) => x.ms), [0]);
  assert.deepEqual(t.listeners.map((l) => [l.type, l.capture]), [["click", true]]);
  assert.equal(t.clickReaches(), false);
  t.clean(t.rows.a.block);
  assert.equal(t.clickReaches(), true);
});

test("tree: the other targets: into a fleet, before and after a row, out of a fleet, a fleet head", () => {
  const drive = (t: ReturnType<typeof tree>, el: El, y: number, moved = () => t.down()) => {
    moved();
    t.over(el);
    t.fire("mousemove", ev(50, y));
    const label = t.ghost()!.children[1];
    const seen = { label: label.textContent, hidden: label.hidden };
    t.fire("mouseup", {});
    return { ...seen, drop: t.dropped.at(-1).drop };
  };
  const t = tree();
  let r = drive(t, t.rc.head, 180);
  assert.equal(r.label, "Add to F");
  assert.equal(r.drop.fleet, "F");
  assert.equal(r.drop.mark, "drop-into");
  r = drive(t, t.rows.b.head, 85);
  assert.equal(r.drop.side, "before");
  assert.equal(r.hidden, true, "no label for a move inside the same place");
  r = drive(t, t.rows.b.head, 115);
  assert.equal(r.drop.side, "after");
  r = drive(t, t.fHead, 130);
  assert.equal(r.drop.anchor, t.c);
  assert.equal(r.drop.fleet, "F");
  // A project in a fleet goes out over the Projects head.
  r = drive(t, t.projectsHead, 10, () => t.mod.dragTree(ev(10, 180), t.rc.block, [t.c], null));
  assert.equal(r.label, "Take out of F");
  assert.equal(r.drop.fleet, null);
  assert.equal(r.drop.anchor, t.a);
  t.clean(t.rc.block);
});

test("tree: a fleet moves whole: before or after another, with its name on the ghost", () => {
  const t = tree();
  t.mod.dragTree(ev(10, 130), t.fHead, [t.c], "F");
  t.over(t.gHead);
  t.fire("mousemove", ev(50, 205));
  assert.equal(t.ghost()!.children[0].textContent, "F");
  t.fire("mouseup", {});
  assert.equal(t.dropped[0].drop.side, "before");
  assert.equal(t.dropped[0].drop.anchor, t.d);
  t.clean(t.fHead);
});

test("tree: a drop on nothing, or on itself, renders and changes nothing, and the click is still swallowed", () => {
  const t = tree();
  t.down();
  t.over(t.rows.b.head);
  t.fire("mousemove", ev(50, 100));
  assert.equal(t.rows.b.head.cls.has("drop-into"), true);
  t.over(null);
  t.fire("mousemove", ev(50, 400));
  assert.equal(t.rows.b.head.cls.has("drop-into"), false, "the mark goes when the target goes");
  assert.equal(t.ghost()!.children[1].hidden, true);
  assert.equal(t.ghost()!.style.top, "390px", "placed by its height with the label already gone");
  t.fire("mouseup", {});
  assert.deepEqual(t.log, ["render() active=false body=false ghost=false listeners=0"]);
  assert.equal(t.dropped.length, 0);
  assert.equal(t.clickReaches(), false);
  t.clean(t.rows.a.block);

  const u = tree();
  u.down();
  u.over(u.rows.a.head);
  u.fire("mousemove", ev(50, 70));
  u.fire("mouseup", {});
  assert.equal(u.dropped.length, 0);
  assert.equal(u.log.length, 1);
  u.clean(u.rows.a.block);
});

test("tree: Esc cancels at the capture stage, before and during the drag; another key does nothing", () => {
  const t = tree();
  t.down();
  t.over(t.rows.b.head);
  t.fire("mousemove", ev(50, 100));
  const other = t.key("a");
  assert.deepEqual([other.prevented, other.stopped], [0, 0]);
  assert.equal(t.treeDrag.active, true);
  const e = t.key("Escape");
  assert.deepEqual([e.prevented, e.stopped], [1, 1]);
  assert.deepEqual(t.log, ["render() active=false body=false ghost=false listeners=0"]);
  assert.equal(t.dropped.length, 0);
  assert.equal(t.clickReaches(), false);
  t.clean(t.rows.a.block);
  // Later events find no listener.
  t.fire("mouseup", {});
  assert.equal(t.log.length, 1);

  const u = tree();
  u.down();
  const early = u.key("Escape");
  assert.deepEqual([early.prevented, early.stopped], [1, 1]);
  assert.equal(u.log.length, 1);
  assert.equal(u.timers.length, 0, "no drag started, so no click is swallowed");
  assert.equal(u.clickReaches(), true);
  u.clean(u.rows.a.block);
});

// ---- The Skippers list

function skippers(o: { can?: boolean; scopes?: (string | null)[] } = {}) {
  const pg = page();
  const list = new El("sk-list");
  list.box = { top: 0, bottom: 400, left: 0, right: 200, height: 400 };
  pg.scroll.append(list);
  const rows = (o.scopes ?? [null, "fleet:F", "one", "two"]).map((scope, i) => {
    const r = new El("sk-row");
    if (scope) r.dataset.scope = scope;
    r.box = boxAt(i * 60, 60);
    list.append(r);
    return r;
  });
  const moved: unknown[][] = [];
  const mod = moduleFromSource("../src/workspace/skipperDrag.ts", {
    "../app/renderRequest": { render: (...args: unknown[]) => pg.note(`render(${args.join()})`) },
    "../app/hostCaps": { hostCan: (cap: string) => cap === "skipper_config" && o.can !== false },
    "../ui/dom": pg.dom,
    "./skipperOrder": { moveSkipper: (...args: unknown[]) => (pg.note("moveSkipper"), moved.push(args)) },
    "./tree": { treeDrag: pg.treeDrag },
  }, pg.globals, "");
  const down = (i = 1, y = 90, more: object = {}) => mod.dragSkipper(ev(10, y, more), rows[i], rows[i].dataset.scope, `name${i}`);
  return { ...pg, list, rows, moved, mod, down };
}

test("skippers: a press that does not move stays a click, and its release does not render", () => {
  const t = skippers();
  t.down();
  assert.deepEqual(t.listeners.map((l) => [l.type, l.capture]), [["mousemove", false], ["mouseup", false], ["keydown", true]]);
  t.fire("mouseup", {});
  assert.deepEqual(t.log, []);
  assert.equal(t.timers.length, 0, "no click is swallowed");
  assert.equal(t.clickReaches(), true);
  t.clean(t.rows[1]);
});

test("skippers: a press that does not start: wrong button, a Host without it, or one movable row", () => {
  const t = skippers();
  t.down(1, 90, { button: 1 });
  assert.equal(t.listeners.length, 0);
  const no = skippers({ can: false });
  no.down();
  assert.equal(no.listeners.length, 0);
  const lone = skippers({ scopes: [null, "only"] });
  lone.down();
  assert.equal(lone.listeners.length, 0);
});

test("skippers: under five pixels of vertical move nothing starts, and five starts it", () => {
  const t = skippers();
  t.down(1, 90);
  t.over(t.rows[2]);
  t.fire("mousemove", ev(500, 94));
  t.fire("mousemove", ev(500, 86));
  assert.equal(t.treeDrag.active, false);
  assert.equal(t.ghost(), null);
  t.fire("mousemove", ev(10, 95));
  assert.equal(t.treeDrag.active, true);
  assert.equal(t.body.cls.has("dragging-tree"), true);
  assert.equal(t.rows[1].cls.has("dragging"), true);
  t.key("Escape");
  t.clean(t.rows[1]);
});

test("skippers: a drop on a row moves it there, after the cleanup, with no render", () => {
  const t = skippers();
  t.down(1, 90);
  t.over(t.rows[3]);
  t.fire("mousemove", ev(50, 190));
  const g = t.ghost()!;
  assert.equal(g.children.length, 1, "this ghost has no label");
  assert.equal(g.children[0].textContent, "name1");
  assert.equal(g.style.left, "62px");
  assert.equal(g.style.top, "180px");
  assert.equal(t.rows[3].cls.has("drop-before"), true);
  t.fire("mousemove", ev(50, 230));
  assert.equal(t.rows[3].cls.has("drop-before"), false);
  assert.equal(t.rows[3].cls.has("drop-after"), true);
  t.fire("mouseup", {});
  assert.deepEqual(t.log, ["moveSkipper active=false body=false ghost=false listeners=0"]);
  assert.deepEqual(t.moved, [["fleet:F", "two", "after"]]);
  assert.deepEqual(t.timers.map((x) => x.ms), [0]);
  assert.equal(t.clickReaches(), false);
  t.clean(t.rows[1]);
  assert.equal(t.clickReaches(), true);
});

test("skippers: the Skiff agent's row and the empty list below the last row", () => {
  const t = skippers();
  t.down(3, 200);
  t.over(t.rows[0]);
  t.fire("mousemove", ev(50, 30));
  assert.equal(t.rows[0].cls.has("drop-after"), true, "over the Skiff agent's row the drag lands right under it");
  t.fire("mouseup", {});
  assert.deepEqual(t.moved, [["two", "fleet:F", "before"]]);

  const u = skippers();
  u.down(1, 90);
  u.over(null);
  u.fire("mousemove", ev(50, 300));
  assert.equal(u.rows[3].cls.has("drop-after"), true);
  u.fire("mouseup", {});
  assert.deepEqual(u.moved, [["fleet:F", "two", "after"]]);
});

test("skippers: a drop that changes nothing stores nothing and renders, and the click is still swallowed", () => {
  const cases: [(t: ReturnType<typeof skippers>) => void, string][] = [
    [(t) => t.over(t.rows[1]), "on itself"],
    [(t) => t.over(t.rows[2]), "just before the next row: its own place"],
    [(t) => t.over(null), "off the list"],
  ];
  for (const [aim, name] of cases) {
    const t = skippers();
    t.down(1, 90);
    aim(t);
    t.fire("mousemove", ev(name === "off the list" ? 500 : 50, 130));
    t.fire("mouseup", {});
    assert.deepEqual(t.log, ["render(skippers-order) active=false body=false ghost=false listeners=0"], name);
    assert.deepEqual(t.moved, [], name);
    assert.equal(t.clickReaches(), false, name);
    t.clean(t.rows[1]);
  }
});

test("skippers: a drop right after the row before it is its own place, so nothing is stored", () => {
  const t = skippers();
  t.down(2, 150);
  t.over(t.rows[1]);
  t.fire("mousemove", ev(50, 110));
  assert.equal(t.rows[1].cls.has("drop-after"), true, "the bottom half of the row before it");
  t.fire("mouseup", {});
  assert.deepEqual(t.moved, []);
  assert.deepEqual(t.log, ["render(skippers-order) active=false body=false ghost=false listeners=0"]);
  assert.equal(t.clickReaches(), false);
  t.clean(t.rows[2]);
});

test("skippers: Esc cancels at the capture stage, before and during the drag; another key does nothing", () => {
  const t = skippers();
  t.down(1, 90);
  t.over(t.rows[3]);
  t.fire("mousemove", ev(50, 190));
  const other = t.key("Enter");
  assert.deepEqual([other.prevented, other.stopped], [0, 0]);
  const e = t.key("Escape");
  assert.deepEqual([e.prevented, e.stopped], [1, 1]);
  assert.deepEqual(t.log, ["render(skippers-order) active=false body=false ghost=false listeners=0"]);
  assert.deepEqual(t.moved, []);
  assert.equal(t.clickReaches(), false);
  t.clean(t.rows[1]);

  const u = skippers();
  u.down(1, 90);
  const early = u.key("Escape");
  assert.deepEqual([early.prevented, early.stopped], [1, 1]);
  assert.deepEqual(u.log, ["render(skippers-order) active=false body=false ghost=false listeners=0"]);
  assert.equal(u.timers.length, 0);
  u.clean(u.rows[1]);
});

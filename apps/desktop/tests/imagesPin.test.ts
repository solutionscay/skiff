import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, moduleFromSource } from "./workspaceHarness.ts";

// The real images.ts and imagesPage.ts. Pin and Unpin move an image to its
// section at once; Insert does not reorder the open drop-down.
type Node = El | string;
class El {
  tag: string;
  className: string;
  textContent: string;
  type = "";
  value = "";
  placeholder = "";
  spellcheck = false;
  autocomplete = "";
  disabled = false;
  hidden = false;
  tabIndex = 0;
  scrollTop = 0;
  offsetWidth = 0;
  offsetHeight = 0;
  scrolled = 0;
  children: Node[] = [];
  dataset: Record<string, string> = {};
  style: Record<string, unknown> = { setProperty() {} };
  listeners = new Map<string, ((e: any) => void)[]>();
  classList = {
    toggle: (c: string, on?: boolean) => {
      const has = this.classes().includes(c);
      const want = on ?? !has;
      if (want && !has) this.className = [...this.classes(), c].join(" ");
      if (!want && has) this.className = this.classes().filter((x) => x !== c).join(" ");
    },
    add: (c: string) => this.classList.toggle(c, true),
    contains: (c: string) => this.classes().includes(c),
  };
  constructor(tag: string, cls = "", text = "") {
    this.tag = tag;
    this.className = cls;
    this.textContent = text;
  }
  classes() { return this.className.split(" ").filter(Boolean); }
  append(...kids: Node[]) { this.children.push(...kids); }
  prepend(...kids: Node[]) { this.children.unshift(...kids); }
  appendChild(k: El) { this.children.push(k); return k; }
  replaceChildren(...kids: Node[]) { this.children = kids; }
  setAttribute() {}
  addEventListener(ev: string, run: (e: any) => void) { this.listeners.set(ev, [...(this.listeners.get(ev) ?? []), run]); }
  fire(ev: string, e: any = {}) {
    for (const run of this.listeners.get(ev) ?? []) run({ preventDefault() {}, stopPropagation() {}, ...e });
  }
  focus() { doc.activeElement = this; }
  scrollIntoView() { this.scrolled++; }
  getBoundingClientRect() { return { top: 0, bottom: 0, left: 0, right: 0 }; }
  all(): El[] { return [this, ...this.children.flatMap((c) => (typeof c === "string" ? [] : c.all()))]; }
  querySelectorAll(sel: string) {
    const want = sel.split(".").filter(Boolean);
    return this.all().slice(1).filter((e) => want.every((c) => e.classes().includes(c)));
  }
  querySelector(sel: string) { return this.querySelectorAll(sel)[0] ?? null; }
  text(): string { return this.children.map((c) => (typeof c === "string" ? c : c.text())).join("") || this.textContent; }
}
const doc = {
  activeElement: null as El | null,
  body: new El("body"),
  addEventListener() {},
  querySelector: () => null,
  querySelectorAll: () => [],
};
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);
const button = (cls: string, label: string, run: () => void) => {
  const b = h("button", cls, label);
  b.addEventListener("click", run);
  return b;
};
const globals = {
  document: doc,
  window: { setTimeout, innerWidth: 1200, innerHeight: 800, addEventListener() {}, removeEventListener() {} },
  IntersectionObserver: class { observe() {} unobserve() {} disconnect() {} },
  queueMicrotask,
  setTimeout,
  clearTimeout,
};

async function app() {
  // a, b and c are recent; d is pinned. Newest first.
  const store = [
    { path: "/c/a.png", file: "a.png", name: "red cat.png", at: 400, pinned: false },
    { path: "/c/b.png", file: "b.png", name: "blue dog.png", at: 300, pinned: false },
    { path: "/c/c.png", file: "c.png", name: "red cow.png", at: 200, pinned: false },
    { path: "/c/d.png", file: "d.png", name: "green cat.png", at: 100, pinned: true },
  ].map((i) => ({ ...i, session: null, title: null, project: null, size: 1 }));
  let changed = () => {};
  const invoke = async (cmd: string, a: any) => {
    if (cmd === "images") return store.map((i) => ({ ...i })).sort((x, y) => y.at - x.at);
    if (cmd === "image_pin") for (const i of store) if (a.paths.includes(i.path)) i.pinned = a.on;
    if (cmd === "image_insert") for (const i of store) if (a.paths.includes(i.path)) i.at = 500;
    changed();
    return cmd === "image_insert" ? { text: "", warning: null, image: a.paths[0] } : null;
  };
  const S: any = { focused: "s1", projects: [] };
  const lib = moduleFromSource("../src/terminal/images.ts", {
    "@tauri-apps/api/core": { invoke },
    "@tauri-apps/api/event": { listen: async (_: string, f: () => void) => (changed = f) },
    "../app/render": { scheduleRender() {} },
    "../app/state": { S, sessions: new Map() },
    "../app/stateQueries": { accent: () => "#3366cc", place: () => null },
    "../platform/clipboard": { copyText: async () => {} },
    "../ui/alerts": { showError: (e: unknown) => { throw e; } },
    "../ui/dom": { h, button },
    "../ui/icons": { icon: () => h("svg") },
    "../ui/menu": { menuOpened() {} },
    "../workspace/view": { refocusTerminal() {} },
    "./fileDrop": {},
    "./insertSignal": { insertShown: () => null },
    "./paneActions": {},
    "./terminalClipboard": { typeFiles() {}, whoOf: () => ({}) },
    "./terminalState": { panes: new Map([["s1", {}]]) },
  }, globals, `
exports.__t = { openAt, rows, field, lit: () => lit };`);
  await flush();
  const t = lib.__t;
  const rows = t.rows as El;
  /** The drop-down as text: "PINNED d | RECENT a b c". */
  const menu = () => rows.children.map((c) => {
    const e = c as El;
    return e.classes().includes("lib-sec") ? (e.children[0] as string) : e.dataset.path.slice(3, 4);
  }).join(" ").replace(" RECENT", " | RECENT");
  const row = (p: string) => rows.querySelectorAll(".lib-row").find((r) => r.dataset.path === `/c/${p}.png`)!;
  const act = (p: string, label: string) => row(p).all().find((e) => e.tag === "button" && e.textContent === label)!;
  const key = (k: string) => rows.fire("keydown", { key: k });
  const settle = async () => { for (let n = 0; n < 4; n++) await flush(); };
  return { lib, t, rows, menu, row, act, key, settle, store };
}

test("drop-down: Pin moves the row to PINNED at once; the light and the query stay", async () => {
  const a = await app();
  a.t.openAt({ right: 0, bottom: 0 });
  assert.equal(a.menu(), "PINNED d | RECENT a b c");
  // The mouse: Pin on row c.
  a.act("c", "Pin").fire("click");
  await a.settle();
  assert.equal(a.menu(), "PINNED c d | RECENT a b");
  assert.equal(a.t.lit(), "/c/c.png");
  assert.ok(a.row("c").classes().includes("lit"));
  assert.ok(a.row("c").scrolled > 0);
  // The keys: P on the lit row unpins it, and it goes back to RECENT.
  a.key("p");
  await a.settle();
  assert.equal(a.menu(), "PINNED d | RECENT a b c");
  assert.equal(a.t.lit(), "/c/c.png");
  // With a query: the query stays, and the row moves in the filtered list.
  a.t.field.value = "red";
  a.t.field.fire("input");
  assert.equal(a.menu(), "a c");
  a.act("c", "Pin").fire("click");
  await a.settle();
  assert.equal(a.t.field.value, "red");
  assert.equal(a.menu(), "PINNED c | RECENT a");
  assert.equal(a.t.lit(), "/c/c.png");
});

test("drop-down: Insert does not reorder RECENT while it is open", async () => {
  const a = await app();
  a.t.openAt({ right: 0, bottom: 0 });
  a.act("c", "Insert").fire("click");
  await a.settle();
  assert.equal(a.store.find((i) => i.file === "c.png")!.at, 500);
  assert.equal(a.menu(), "PINNED d | RECENT a b c");
  assert.match(a.row("c").text(), /Inserted/);
  // The next open shows the new order.
  a.lib.closeImages(false);
  a.t.openAt({ right: 0, bottom: 0 });
  assert.equal(a.menu(), "PINNED d | RECENT c a b");
});

async function gallery() {
  const a = await app();
  const page = moduleFromSource("../src/appearance/imagesPage.ts", {
    "../app/keys": { isMac: false },
    "../app/state": { sessions: new Map() },
    "../app/stateQueries": { accent: () => "#3366cc", place: () => null },
    "../platform/clipboard": { copyText: async () => {} },
    "../ui/alerts": { showError: (e: unknown) => { throw e; } },
    "../ui/confirm": { confirmAction: async () => true },
    "../ui/dom": { h },
    "../ui/icons": { icon: () => h("svg") },
    "../workspace/model": { taskTitle: () => "" },
    "../terminal/images": a.lib,
    "./skiffFolderDialog": { size: () => "" },
  }, globals, "");
  const st = page.imagesState();
  let open = true;
  let main: El = page.imagesSection(st, () => (open = false));
  // As Settings does: a library change redraws the open gallery.
  a.lib.onImagesChanged(() => { if (open) main = page.imagesSection(st, () => (open = false)); });
  const scroll = () => main.querySelector("img-scroll")!;
  /** The gallery as text: "PINNED d | RECENT a b c". */
  const grid = () => scroll().children.map((c) => {
    const e = c as El;
    return e.classes().includes("lib-sec") ? (e.children[0] as string) : e.children.map((t) => (t as El).dataset.path.slice(3, 4)).join(" ");
  }).join(" ").replace(" RECENT", " | RECENT");
  const tile = (p: string) => main.querySelectorAll("img-tile").find((t) => t.dataset.path === `/c/${p}.png`)!;
  const pick = (p: string) => tile(p).fire("click");
  const btn = (label: string) => main.querySelectorAll("img-btn").find((b) => b.textContent === label)!;
  return { ...a, st, grid, tile, pick, btn, scroll, main: () => main, open: () => open };
}

test("gallery: Pin moves the tile to PINNED at once; the selection stays on it", async () => {
  const g = await gallery();
  assert.equal(g.grid(), "PINNED d | RECENT a b c");
  g.pick("c");
  g.btn("Pin").fire("click");
  await g.settle();
  assert.equal(g.grid(), "PINNED c d | RECENT a b");
  assert.deepEqual([...g.st.sel], ["/c/c.png"]);
  assert.equal(g.st.now, "/c/c.png");
  assert.ok(g.tile("c").classes().includes("sel"));
  assert.ok(g.tile("c").scrolled > 0);
  // P unpins it, and it goes back to RECENT.
  g.scroll().fire("keydown", { key: "p" });
  await g.settle();
  assert.equal(g.grid(), "PINNED d | RECENT a b c");
  assert.deepEqual([...g.st.sel], ["/c/c.png"]);
});

test("gallery: Insert closes Settings and does not reorder the gallery", async () => {
  const g = await gallery();
  g.pick("c");
  g.btn("Insert").fire("click");
  await g.settle();
  assert.equal(g.open(), false);
  assert.equal(g.grid(), "PINNED d | RECENT a b c");
});

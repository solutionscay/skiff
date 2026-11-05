import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real tree.ts, section heads, keyboard handler and workspace state. The Projects head folds as a fleet head does.
class El {
  className: string;
  textContent: string;
  tagName = "BUTTON";
  title = "";
  tabIndex = -1;
  children: El[] = [];
  dataset: Record<string, string> = {};
  attrs: Record<string, string> = {};
  listeners = new Map<string, ((e?: unknown) => void)[]>();
  classList = {
    toggle() {},
    add() {},
    contains: (c: string) => this.className.split(" ").includes(c),
  };
  constructor(cls = "", text = "") {
    this.className = cls;
    this.textContent = text;
  }
  append(...kids: (El | string)[]) {
    for (const k of kids) this.children.push(typeof k === "string" ? new El("", k) : k);
  }
  appendChild(k: El) {
    this.append(k);
    return k;
  }
  setAttribute(k: string, v: string) {
    this.attrs[k] = v;
  }
  hasAttribute(k: string) {
    return k in this.attrs;
  }
  addEventListener(ev: string, run: (e?: unknown) => void) {
    this.listeners.set(ev, [...(this.listeners.get(ev) ?? []), run]);
  }
  fire(ev: string, e?: unknown) {
    for (const run of this.listeners.get(ev) ?? []) run(e);
  }
  click() {
    this.fire("click");
  }
  focus() {
    focused.at = this;
  }
  all(): El[] {
    return [this, ...this.children.flatMap((c) => c.all())];
  }
  find(cls: string) {
    return this.all().find((e) => e.classList.contains(cls));
  }
}
const focused: { at: El | null } = { at: null };
const h = (_tag: string, cls = "", text = "") => new El(cls, text);
const button = (cls: string, text: string, run: () => void) => {
  const b = new El(cls, text);
  b.addEventListener("click", run);
  return b;
};
const stub: any = new Proxy({}, { get: (_, k) => (k === "__esModule" ? false : () => []) });
const withStubs = (imports: Record<string, unknown>) => new Proxy(imports, { get: (t, k: string) => t[k] ?? stub });

function app(stored = new Map<string, string>(), loose = ["web", "api"]) {
  const projects = [
    ...loose.map((name) => ({ name, path: `/${name}`, worktrees: [{ path: `/${name}`, branch: "main", is_main: true }] })),
    { name: "site", path: "/site", fleet: "Dock", worktrees: [{ path: "/site", branch: "main", is_main: true }] },
  ];
  const S: any = { projects, focused: null, roveKey: null, roveAt: null, groupPicked: null, selection: [], groups: [], closedProjects: [] };
  const sessions = new Map<string, any>([
    ["web-1", { id: "web-1", state: "working", cwd: "/web" }],
    ["web-2", { id: "web-2", state: "waiting", cwd: "/web" }],
    ["site-1", { id: "site-1", state: "working", cwd: "/site" }],
  ]);
  const place = (s: any) => {
    const project = projects.find((p) => s.cwd === p.path);
    return project ? { project, worktree: project.worktrees[0] } : null;
  };
  const globals = {
    localStorage: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => stored.set(k, v) },
    requestAnimationFrame: (run: () => void) => run(),
    performance: { now: () => 0 },
    console,
  };
  const state = { S, sessions, removing: new Set(), removeErrors: new Map(), collapsed: new Set(), selectedWorktree: new Map(), DEFAULT_ACCENT: "#888" };
  const stateQueries = { place, accent: () => "#888", awayPlaces: () => [] };
  const skiffAgent = new Proxy({ fleetSkipperOf: () => null, coordinatorOf: () => null, skiffAgentFolder: () => null }, { get: (t: any, k: string) => t[k] ?? stub[k] });
  const model = { isUnread: () => false, isFolder: () => false };
  const tree = moduleFromSource("../src/workspace/tree.ts", withStubs({
    "../app/skiffAgent": skiffAgent, "./model": model, "../app/stored": moduleFromSource("../src/app/stored.ts", {}, globals, ""),
    "../app/state": state, "../app/stateQueries": stateQueries, "../ui/dom": { h }, "../ui/icons": { icon: () => new El("svg") },
    "../app/hosts": { shownHost: () => null },
  }), globals, "");
  let rendered = 0;
  let added = 0;
  const sidebar = moduleFromSource("../src/workspace/sidebar.ts", withStubs({
    "../ui/dom": { h, button, $: () => new El("") },
    "../ui/icons": { icon: () => new El("svg"), chevron: (open: boolean) => new El(open ? "chev" : "chev closed"), plusIcon: () => new El("svg") },
    "./tree": tree, "../app/state": state, "../app/stateQueries": stateQueries, "../app/skiffAgent": skiffAgent, "./model": model,
    "../app/renderRequest": { render: () => rendered++ },
    "./profile": { workspaceOpen: () => false, shows: () => true, projectsPlus: () => added++ },
  }), { ...globals, document: { activeElement: null } }, "\nexports.sectionHead = sectionHead;");
  const projectsSec = () => tree.treeSections()[0];
  const head = () => sidebar.sectionHead(projectsSec(), null, !projectsSec().projects.length) as El;
  return { S, sessions, tree, head, stored, rendered: () => rendered, added: () => added };
}

test("a click on the Projects head folds and opens it, with the chevron, aria-expanded and the + of a fleet head", () => {
  const a = app();
  let head = a.head();
  assert.ok(head.classList.contains("fleet-head"));
  let pick = head.find("fleet-pick")!;
  assert.equal(pick.attrs["aria-expanded"], "true");
  assert.equal(pick.dataset.fold, pick.dataset.key);
  assert.ok(head.find("chev"));
  assert.equal(head.find("tree-roll"), undefined);

  pick.click();
  assert.equal(a.rendered(), 1);
  assert.deepEqual([...a.tree.visibleProjects().map((p: any) => p.name)], ["site"], "only the fleet's project still shows");

  head = a.head();
  pick = head.find("fleet-pick")!;
  assert.equal(pick.attrs["aria-expanded"], "false");
  assert.ok(head.find("chev")!.classList.contains("closed"));
  // The rollup counts the sessions under Projects, not the fleet's: one waits and one works.
  assert.deepEqual(head.find("tree-roll")!.children.map((c) => c.children.at(-1)!.textContent), ["1", "1"]);

  // The + opens its menu and does not fold.
  head.find("tree-head-add")!.click();
  assert.equal(a.added(), 1);
  assert.equal(a.tree.isOpen(pick.dataset.key), false);

  pick.click();
  assert.deepEqual([...a.tree.visibleProjects().map((p: any) => p.name)], ["web", "api", "site"]);
});

test("the Projects fold lasts across a restart, and the session on screen opens it", () => {
  const a = app();
  a.head().find("fleet-pick")!.click();
  const again = app(a.stored);
  assert.equal(again.head().find("fleet-pick")!.attrs["aria-expanded"], "false");
  again.S.focused = "web-1";
  again.tree.openToFocused();
  assert.equal(again.head().find("fleet-pick")!.attrs["aria-expanded"], "true");
});

test("a fleet and the Projects head fold apart", () => {
  const a = app();
  a.tree.toggleRow(a.tree.fleetKey("Dock"), () => {});
  assert.deepEqual([...a.tree.visibleProjects().map((p: any) => p.name)], ["web", "api"]);
  assert.equal(a.head().find("fleet-pick")!.attrs["aria-expanded"], "true");
});

test("a Projects head with no project under it has no fold, no chevron and no stop for the keys", () => {
  const a = app(new Map(), []);
  const head = a.head();
  assert.equal(head.find("fleet-pick"), undefined);
  assert.equal(head.find("chev"), undefined);
  assert.ok(head.classList.contains("projects-head"));
  head.find("tree-head-add")!.click();
  assert.equal(a.added(), 1);
});

test("Enter, Space, Left and Right fold the Projects head, and the head keeps the keys", () => {
  const a = app();
  const list: { keydown?: (e: unknown) => void } = {};
  const pick = a.head().find("fleet-pick")!;
  moduleFromSource("../src/app/keyboard.ts", withStubs({
    "./state": { S: a.S, sessions: a.sessions, collapsed: new Set() },
    "./keys": { isMac: false, actionFor: () => null },
    "../ui/dom": { modalOpen: () => false, $: () => ({ addEventListener: (ev: string, run: (e: unknown) => void) => { if (ev === "keydown") list.keydown = run; } }) },
    "../workspace/tree": a.tree,
    "../workspace/rowActs": { rowActs: () => undefined },
  }), {
    window: { addEventListener() {} },
    document: {
      body: { classList: { add() {}, remove() {} } },
      get activeElement() { return focused.at; },
      querySelector: () => null,
      querySelectorAll: (sel: string) => (sel.startsWith("#sidebar-scroll .fleet-pick") ? [pick] : []),
    },
    requestAnimationFrame: (run: () => void) => run(),
  }, "");
  const press = (key: string) => {
    pick.focus();
    list.keydown!({ key, shiftKey: false, preventDefault() {}, stopPropagation() {} });
    assert.equal(focused.at, pick);
    return a.tree.isOpen(pick.dataset.key);
  };
  assert.equal(press("ArrowRight"), true, "Right on an open head changes nothing");
  assert.equal(press("ArrowLeft"), false);
  assert.equal(press("ArrowLeft"), false, "Left on a folded head changes nothing");
  assert.equal(press("Enter"), true);
  assert.equal(press(" "), false);
  assert.equal(press("ArrowRight"), true);
  assert.equal(a.S.roveKey, pick.dataset.key, "the head is the current row");
});

test("in a workspace the Projects fold goes in folded_fleets, as an edit of that file only", () => {
  const a = app();
  const profileState = moduleFromSource("../src/workspace/profile/state.ts", withStubs({
    "../../app/state": { S: a.S }, "../tree": a.tree,
  }), {}, "");
  const ws = profileState.createWorkspaceState();
  const profile = { name: "w", projects: ["/web", "/site"], fleets: [], folded_projects: [], folded_fleets: [], selected: null };
  ws.doc = { path: "/w.skiffw", profile, base: profileState.canon(profile), revision: "r", disk: "ok", kept: false, answered: false };
  a.tree.useTreeView(ws.treeView);
  assert.deepEqual([...a.tree.treeSections()[0].projects.map((p: any) => p.name)], ["web"]);

  a.head().find("fleet-pick")!.click();
  assert.deepEqual([...profile.folded_fleets], [""]);
  assert.equal(ws.workspaceEdited(), true);
  assert.equal(a.head().find("fleet-pick")!.attrs["aria-expanded"], "false");
  assert.deepEqual([...ws.treeView.keys()], [a.tree.sectionKey(null)]);
  assert.equal(a.stored.get("skiff.tree.folded") ?? "[]", "[]", "All projects keeps its own folds");
  assert.deepEqual([...a.tree.visibleProjects().map((p: any) => p.name)], ["site"]);

  // The session on screen opens it with no edit to the file.
  a.S.focused = "web-1";
  a.tree.openToFocused();
  assert.equal(a.tree.isOpen(a.tree.sectionKey(null)), true);
  assert.deepEqual([...profile.folded_fleets], [""]);
  a.tree.useTreeView(null);
});

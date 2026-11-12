import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, moduleFromSource, workspaceHarness } from "./workspaceHarness.ts";

const values = (set: Set<string>) => [...set];

test("Edit → Apply preserves overlapping explicit project and fleet inclusions", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw", ["/a"], ["F"]);
  await h.profile.openWorkspace("/work.skiffw");
  h.profile.editWorkspace();
  h.button("Apply").click();
  assert.equal(h.profile.workspaceEdited(), false);
  h.profile.saveWorkspace();
  await flush();
  assert.deepEqual(h.files.get("/work.skiffw").profile.projects, ["/a"]);
  h.S.projects[0].fleet = null;
  assert.equal(h.profile.shows(h.S.projects[0]), true);
});

test("removing fleet coverage preserves explicit projects but not fleet-only projects", async () => {
  const h = workspaceHarness();
  h.S.projects[1].fleet = "F";
  h.fixture("/work.skiffw", ["/a"], ["F"]);
  await h.profile.openWorkspace("/work.skiffw");
  h.profile.editWorkspace();
  const fleet = h.elements().find((el) => el.tag === "label" && el.children.some((c) => c.textContent === "F"))!.children[0];
  fleet.checked = false;
  fleet.fire("change");
  h.button("Apply").click();
  assert.equal(h.profile.shows(h.S.projects[0]), true);
  assert.equal(h.profile.shows(h.S.projects[1]), false);
});

test("opening and creating a workspace reset All work; a rejected open preserves it", async () => {
  const h = workspaceHarness();
  h.fixture("/a.skiffw");
  h.fixture("/b.skiffw", ["/b"]);
  await h.profile.openWorkspace("/a.skiffw");
  h.profile.setLedgerAllWork(true);
  await h.profile.openWorkspace("/missing.skiffw");
  assert.equal(h.profile.ledgerScope(), null);
  await h.profile.openWorkspace("/b.skiffw");
  assert.equal(h.profile.ledgerAllWork(), false);
  assert.deepEqual(values(h.profile.ledgerScope().names), ["B"]);
  h.profile.setLedgerAllWork(true);
  h.profile.newWorkspace();
  await flush();
  assert.equal(h.profile.ledgerAllWork(), false);
  assert.deepEqual(values(h.profile.ledgerScope().names), []);
});

test("All work toggles preserve edits and never show the leave prompt", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw");
  await h.profile.openWorkspace("/work.skiffw");
  h.profile.editWorkspace();
  h.elements().find((el) => el.type === "text")!.value = "Edited";
  h.button("Apply").click();
  h.profile.setLedgerAllWork(true);
  h.profile.setLedgerAllWork(false);
  assert.equal(h.profile.workspaceEdited(), true);
  assert.equal(h.elements().some((el) => el.textContent === "Don't Save"), false);
});

for (const route of ["skiff:quit-requested", "window-close"]) {
  for (const decision of ["Cancel", "Don't Save", "Save", "failed Save", "cancelled Save As"]) {
    test(`${route}: ${decision} uses the workspace leave guard`, async () => {
      const h = workspaceHarness();
      await h.profile.startWorkspaceQuitGuard();
      await h.profile.startWorkspaces();
      assert.equal(h.calls.guarded, true);
      if (decision === "cancelled Save As") {
        h.profile.newWorkspace();
        await flush();
      } else {
        h.fixture("/work.skiffw");
        await h.profile.openWorkspace("/work.skiffw");
        h.profile.editWorkspace();
      }
      h.elements().find((el) => el.type === "text")!.value = "Edited";
      h.button("Apply").click();
      h.calls.failSave = decision === "failed Save";
      h.events.get(route)!();
      // A duplicate native/window request must not open another prompt or exit.
      h.events.get(route)!();
      assert.equal(h.elements().filter((el) => el.textContent === "Don't Save").length, 1);
      h.button(decision.includes("Save") && decision !== "Don't Save" ? "Save" : decision).click();
      await flush();
      const exits = decision === "Save" || decision === "Don't Save";
      assert.equal(h.calls.approvedExits, exits ? 1 : 0);
      assert.equal(h.profile.workspaceEdited(), decision !== "Save");
      assert.equal(h.calls.saves, decision === "Save" || decision === "failed Save" ? 1 : 0);
    });
  }
}

test("a clean native quit needs no prompt", async () => {
  const h = workspaceHarness();
  await h.profile.startWorkspaceQuitGuard();
  await h.profile.startWorkspaces();
  h.events.get("skiff:quit-requested")!();
  await flush();
  assert.equal(h.calls.approvedExits, 1);
  assert.equal(h.elements().some((el) => el.textContent === "Don't Save"), false);
});

test("a pick asks nothing on leave; Save writes it; a real change still asks", async () => {
  const h = workspaceHarness();
  await h.profile.startWorkspaceQuitGuard();
  await h.profile.startWorkspaces();
  h.fixture("/work.skiffw", ["/a", "/b"]);
  h.files.get("/work.skiffw").profile.selected = "/a";
  await h.profile.openWorkspace("/work.skiffw");
  // A pane in another project takes the keys: a click, or a session that ended.
  h.profile.noteSelected("B");
  assert.equal(h.profile.workspaceEdited(), false);
  h.events.get("skiff:quit-requested")!();
  await flush();
  assert.equal(h.elements().some((el) => el.textContent === "Don't Save"), false);
  assert.equal(h.calls.approvedExits, 1);
  h.profile.saveWorkspace();
  await flush();
  assert.equal(h.files.get("/work.skiffw").profile.selected, "/b");
  h.profile.editWorkspace();
  h.elements().find((el) => el.type === "text")!.value = "Renamed";
  h.button("Apply").click();
  assert.equal(h.profile.workspaceEdited(), true);
  h.events.get("skiff:quit-requested")!();
  assert.equal(h.elements().filter((el) => el.textContent === "Don't Save").length, 1);
});

test("the app start keeps the restored project; the workspace's saved pick does not move it", async () => {
  const h = workspaceHarness();
  await h.profile.startWorkspaces();
  h.fixture("/work.skiffw", ["/a", "/b"]);
  h.files.get("/work.skiffw").profile.selected = "/a";
  await h.profile.openWorkspace("/work.skiffw");
  assert.equal(h.S.selectedProject, "A");
  // The next start: the saved place put B on screen before the workspace opens.
  h.S.selectedProject = "B";
  await h.profile.startWorkspaces();
  assert.equal(h.S.selectedProject, "B");
});

/** The scope helpers of workspace/model.ts the Ledger uses. */
const fleetOf = (scope: string | null | undefined) => (scope?.startsWith("fleet:") ? scope.slice(6) : null);
const MODEL = { scopeFleet: fleetOf, scopeName: (scope: string) => fleetOf(scope) ?? scope, taskTitle: () => "" };

test("mixed-scope plans keep qualifying headings without excluded children", () => {
  const ledger = moduleFromSource("../src/workspace/ledger.ts", {
    "../app/skiffAgent": { skipperWord: () => "Skipper", coordinatorOf: () => null, projectSkipperOf: () => null, skiffAgentSession: () => null },
    "./model": MODEL,
    "./profile": { ledgerScope: () => ({ names: new Set(["A"]) }) },
    "../app/state": { S: { projects: [] }, sessions: new Map() },
    "../app/stateQueries": { shownIds: () => [], skipperSplit: () => null, accent: () => "" },
  }, {
    document: { addEventListener() {} }, window: { addEventListener() {} }, localStorage: { getItem: () => null },
  }, "\nimport { rows } from \"./ledgerStore\";\nexport const review = { setRows: (r: Entry[]) => { rows = r; }, units };\n");
  const plan = (uid: string, project: string) => ({ id: 1, uid, task: uid, kind: "plan", project, state: "pending", closed_at: null });
  const task = (uid: string, parent: string, project: string) => ({ id: 2, uid, kind: "task", project, parent, state: "done", closed_at: null });
  const rows = [
    plan("global", ""), task("hidden-global-child", "global", "B"),
    plan("included", "A"), task("hidden-included-child", "included", "B"),
    plan("context", "B"), task("shown", "context", "A"), task("hidden", "context", "B"),
    plan("excluded", "B"), task("excluded-child", "excluded", "B"),
  ];
  const before = structuredClone(rows);
  ledger.review.setRows(rows);
  const units = ledger.review.units();
  assert.deepEqual(Array.from(units, (u: any) => u.r.uid), ["global", "included", "context"]);
  assert.equal(units[0].kids.length, 0);
  assert.equal(units[1].kids.length, 0);
  assert.deepEqual(Array.from(units[2].kids, (k: any) => k.r.uid), ["shown"]);
  assert.deepEqual(rows, before, "filtering must not mutate canonical rows");
});

test("a hand-off nests under its plan with its tasks, and the filter keeps one project", () => {
  const ledger = moduleFromSource("../src/workspace/ledger.ts", {
    "../app/skiffAgent": { skipperWord: () => "Skipper", coordinatorOf: () => null, projectSkipperOf: () => null, skiffAgentSession: () => null },
    "./model": MODEL,
    "./profile": { ledgerScope: () => null },
    "../app/state": { S: { projects: [] }, sessions: new Map() },
    "../app/stateQueries": { shownIds: () => [], skipperSplit: () => null, accent: () => "" },
  }, {
    document: { addEventListener() {} }, window: { addEventListener() {} }, localStorage: { getItem: () => null },
  }, "\nimport { rows } from \"./ledgerStore\";\nexport const review = { setRows: (r: Entry[]) => { rows = r; }, units, ledgerCounts };\n");
  const row = (uid: string, kind: string, project: string, owner: string, parent: string | null, state = "sent") =>
    ({ id: 1, uid, kind, project, owner, parent, state, closed_at: null, pane_id: null, task: uid, title: null });
  ledger.review.setRows([
    row("ship", "plan", "", "grand", null),
    row("mine", "task", "skiff", "grand", "ship"),
    row("polish", "handoff", "gesher", "grand", "ship"),
    row("nav", "task", "gesher", "gesher", "polish", "blocked"),
    row("copy", "task", "gesher", "gesher", "polish", "done"),
  ]);
  const all = ledger.review.units(null);
  assert.deepEqual(Array.from(all, (u: any) => u.r.uid), ["ship"]);
  assert.deepEqual(Array.from(all[0].kids, (k: any) => k.r.uid), ["mine", "polish"]);
  assert.deepEqual(Array.from(all[0].kids[1].kids, (k: any) => k.r.uid), ["nav", "copy"]);
  // One project: the plan heads only gesher's rows.
  const one = ledger.review.units("gesher");
  assert.deepEqual(Array.from(one[0].kids, (k: any) => k.r.uid), ["polish"]);
  // The gesher Skipper's counts are its own tasks. A blocked task needs the person.
  assert.equal(ledger.review.ledgerCounts("gesher").need, 1);
});

test("every row files by its work's bucket: a closed hand-off's tasks show in Finished", () => {
  const ledger = moduleFromSource("../src/workspace/ledger.ts", {
    "../app/skiffAgent": { skipperWord: () => "Skipper", coordinatorOf: () => null, projectSkipperOf: () => null, skiffAgentSession: () => null },
    "./model": MODEL,
    "./profile": { ledgerScope: () => null },
    "../app/state": { S: { projects: [] }, sessions: new Map() },
    "../app/stateQueries": { shownIds: () => [], skipperSplit: () => null, accent: () => "" },
  }, {
    document: { addEventListener() {} }, window: { addEventListener() {} }, localStorage: { getItem: () => null },
  }, "\nimport { rows } from \"./ledgerStore\";\nexport const review = { setRows: (r: Entry[]) => { rows = r; }, units, sections };\n");
  const row = (id: number, uid: string, kind: string, owner: string, parent: string | null, bucket: string, closed: string | null = null) =>
    ({ id, uid, kind, project: "skiff", owner, parent, state: "done", bucket, closed_at: closed, pane_id: null, task: uid, title: null, updated_at: "u" });
  ledger.review.setRows([
    row(1, "db975634", "handoff", "grand", null, "finished", "2026-10-25T20:19:12Z"),
    row(2, "230ff854", "task", "skiff", "db975634", "finished"),
    // A task whose head is not among the rows still files by its bucket.
    row(3, "0d423aae", "task", "skiff", "1c89bc67", "finished"),
    row(4, "beef0001", "task", "grand", null, "open"),
  ]);
  const x = ledger.review.sections(ledger.review.units(null));
  assert.deepEqual(Array.from(x.open, (u: any) => u.r.uid), ["beef0001"]);
  assert.deepEqual(Array.from(x.finished, (u: any) => u.r.uid).sort(), ["0d423aae", "db975634"]);
  assert.deepEqual(Array.from(x.finished.find((u: any) => u.r.uid === "db975634").look.kids, (k: any) => k.r.uid), ["230ff854"]);
});

test("the fleet filter shows its Skipper's rows, the hand-offs to it and its projects' rows", () => {
  const projects = [{ name: "web", fleet: "Harbor" }, { name: "api", fleet: "Harbor" }, { name: "gesher" }];
  const ledger = moduleFromSource("../src/workspace/ledger.ts", {
    "../app/skiffAgent": { skipperWord: () => "Skipper", coordinatorOf: () => null, projectSkipperOf: () => null, skiffAgentSession: () => null },
    "./model": MODEL,
    "./profile": { ledgerScope: () => null },
    "../app/state": { S: { projects }, sessions: new Map() },
    "../app/stateQueries": { shownIds: () => [], skipperSplit: () => null, accent: () => "" },
  }, {
    document: { addEventListener() {} }, window: { addEventListener() {} }, localStorage: { getItem: () => null },
  }, "\nimport { rows } from \"./ledgerStore\";\nexport const review = { setRows: (r: Entry[]) => { rows = r; }, units, ledgerCounts, ledgerWorkTitle };\n");
  const row = (uid: string, kind: string, project: string, owner: string, parent: string | null, state = "sent") =>
    ({ id: 1, uid, kind, project, owner, parent, state, closed_at: null, pane_id: null, task: uid, title: null });
  ledger.review.setRows([
    row("release", "plan", "", "grand", null),
    row("across", "handoff", "fleet:Harbor", "grand", "release"),
    row("bump", "task", "api", "fleet:Harbor", "across"),
    row("client", "task", "web", "fleet:Harbor", "across", "blocked"),
    row("flaky", "task", "web", "grand", null, "pending"),
    row("polish", "task", "gesher", "gesher", null),
  ]);
  const fleet = ledger.review.units("fleet:Harbor");
  assert.deepEqual(Array.from(fleet, (u: any) => u.r.uid), ["release", "flaky"]);
  assert.deepEqual(Array.from(fleet[0].kids, (k: any) => k.r.uid), ["across"]);
  assert.deepEqual(Array.from(fleet[0].kids[0].kids, (k: any) => k.r.uid), ["bump", "client"]);
  // The fleet Skipper's row counts its own tasks: no pane is live here, so both need the person.
  assert.deepEqual({ ...ledger.review.ledgerCounts("fleet:Harbor") }, { need: 2, working: 0 });
  assert.equal(ledger.review.ledgerWorkTitle("fleet:Harbor"), "across");
  // A project out of the fleet: none of the fleet's rows.
  const gesher = ledger.review.units("gesher");
  assert.deepEqual(Array.from(gesher, (u: any) => u.r.uid), ["release", "polish"]);
  assert.equal(gesher[0].kids.length, 0);
});

test("a project taken out of a fleet the workspace names stays in by its folder", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw", [], ["F"]);
  await h.profile.openWorkspace("/work.skiffw");
  const a = h.S.projects[0];
  assert.equal(h.profile.shows(a), true);
  h.profile.keepInView(a, null);
  a.fleet = null;
  assert.equal(h.profile.shows(a), true);
  assert.equal(h.profile.workspaceEdited(), true);
});

test("a project added outside the app joins the open workspace; one it leaves out stays out (#229)", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw", ["/a"]);
  await h.profile.openWorkspace("/work.skiffw");
  // The first read only learns the folders: B was there before and stays out.
  h.profile.includeNew("", h.S.projects);
  assert.equal(h.profile.workspaceEdited(), false);
  // `skiff projects add`: C is new in projects.toml since the last read.
  h.S.projects.push({ name: "C", path: "/c", fleet: null });
  h.profile.includeNew("", h.S.projects);
  assert.equal(h.profile.shows(h.S.projects[2]), true);
  assert.equal(h.profile.shows(h.S.projects[1]), false, "B was left out on purpose");
  assert.equal(h.profile.workspaceEdited(), true);
  // The next read: nothing new, no second add.
  h.profile.includeNew("", h.S.projects);
  h.profile.saveWorkspace();
  await flush();
  assert.deepEqual([...h.files.get("/work.skiffw").profile.projects], ["/a", "/c"]);
});

test("a Host switch only learns its folders: none joins the workspace (#229)", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw", ["/a"]);
  await h.profile.openWorkspace("/work.skiffw");
  h.profile.includeNew("", h.S.projects);
  h.S.projects.push({ name: "C", path: "/c", fleet: null });
  h.profile.includeNew("remote-host", h.S.projects);
  assert.equal(h.profile.shows(h.S.projects[2]), false);
  assert.equal(h.profile.workspaceEdited(), false);
});

test("workspace history is separate for each Host", () => {
  const values = new Map<string, string>();
  let host: string | null = null;
  const storage = moduleFromSource("../src/workspace/profile/storage.ts", {
    "@tauri-apps/api/core": { invoke() {} },
    "../../app/hosts": { shownHost: () => host },
  }, {
    localStorage: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    },
  }, "");
  storage.remember("/local.skiffw");
  host = "remote-host";
  assert.equal(storage.last(), null);
  assert.deepEqual([...storage.recents()], []);
  storage.remember("/remote.skiffw");
  assert.equal(storage.last(), "/remote.skiffw");
  host = null;
  assert.equal(storage.last(), "/local.skiffw");
  assert.deepEqual([...storage.recents()], ["/local.skiffw"]);
});

test("with no workspace open, an outside add changes nothing (#229)", async () => {
  const h = workspaceHarness();
  h.profile.includeNew("", h.S.projects);
  h.S.projects.push({ name: "C", path: "/c", fleet: null });
  h.profile.includeNew("", h.S.projects);
  assert.equal(h.profile.workspaceOpen(), false);
  assert.equal(h.profile.workspaceEdited(), false);
  // A workspace opened later is as its file says.
  h.fixture("/work.skiffw", ["/a"]);
  await h.profile.openWorkspace("/work.skiffw");
  assert.equal(h.profile.shows(h.S.projects[2]), false);
});

test("Skipper split children by scope: project and fleet in tree order, the Skiff agent by its Ledger rows", () => {
  // Project A (worktrees a1, a2) alone; B and C in fleet F. "sh" is a shell opened by hand in A.
  const wt = (path: string) => ({ path });
  const a1 = wt("/a1"), a2 = wt("/a2"), b1 = wt("/b1"), c1 = wt("/c1");
  const S = { groups: [{ id: "g", layout: ["agent", "a1g"] }], projects: [{ name: "A", fleet: null, worktrees: [a1, a2] }, { name: "B", fleet: "F", worktrees: [b1] }, { name: "C", fleet: "F", worktrees: [c1] }] };
  const home: Record<string, unknown> = { agent: a1, a1g: a1, ask: a1, sh: a1, a1x: a1, a2x: a2, fsk: b1, bsk: b1, b1x: b1, c1x: c1 };
  const order = ["a1x", "sh", "a2x", "a1g", "agent", "ask", "fsk", "bsk", "b1x", "c1x"];
  const sessions = new Map<string, unknown>(order.map((id) => [id, { id, state: "working", started_at: String(order.indexOf(id)) }]));
  const scopeOf: Record<string, string> = { agent: "grand", ask: "A", bsk: "B", fsk: "fleet:F" };
  const coordinatorOf = (id: string) => scopeOf[id] ?? null;
  const skipperOf = (n: string) => sessions.get(Object.keys(scopeOf).find((id) => scopeOf[id] === n) ?? "") ?? null;
  const skiffAgent = {
    coordinatorOf, skiffAgentSession: () => sessions.get("agent"), projectSkippers: () => ["ask", "bsk", "fsk"].map((id) => sessions.get(id)), projectSkipperOf: skipperOf,
    skipperWord: () => "Skipper",
  };
  const lines = moduleFromSource("../src/workspace/sidebarLines.ts", {
    "./layout": { sessionsOf: (l: any) => l ?? [] },
    "./model": { byStart: (x: any, y: any) => x.started_at.localeCompare(y.started_at) },
    "../app/state": { S, sessions },
    "../app/stateQueries": { place: (s: any) => (home[s.id] ? { worktree: home[s.id] } : null), awayPlaces: () => [] },
    "./view": { groupHome: () => a1 },
    "../app/skiffAgent": skiffAgent,
  }, {}, "");
  const ledger = moduleFromSource("../src/workspace/ledger.ts", {
    "../app/skiffAgent": skiffAgent, "./model": MODEL, "./profile": { ledgerScope: () => null },
    "../app/state": { S: { projects: [] }, sessions },
    "../app/stateQueries": { shownIds: () => [], skipperSplit: () => null, accent: () => "" },
  }, {
    document: { addEventListener() {} }, window: { addEventListener() {} }, localStorage: { getItem: () => null },
  }, "\nimport { rows } from \"./ledgerStore\";\nimport { ownPanes } from \"./ledgerModel\";\nexport const review = { setRows: (r: Entry[]) => { rows = r; }, ownPanes };\n");
  let n = 0;
  const row = (uid: string, pane_id: string | null, state: string, o: { kind?: string; parent?: string; owner?: string; closed?: string } = {}) =>
    ({ id: ++n, uid, kind: o.kind ?? "task", project: "", owner: o.owner ?? "grand", parent: o.parent ?? null, state, bucket: o.closed ? "finished" : "open",
      closed_at: o.closed ?? null, updated_at: "x", pane_id, pane_gen: null, task: uid, title: null });
  ledger.review.setRows([
    row("own-done", "a1x", "done"),
    row("handoff", "bsk", "sent", { kind: "handoff" }),
    row("handed", "b1x", "sent", { parent: "handoff", owner: "B" }),
    row("closed", "c1x", "done", { closed: "2026-02-01" }),
    row("not-mine", "a2x", "sent", { owner: "A" }),
    row("self", "agent", "sent"),
  ]);
  let shown = "ask";
  const split = moduleFromSource("../src/workspace/skipperSplit.ts", {
    "./sidebarLines": lines, "./ledgerModel": { ownPanes: ledger.review.ownPanes },
    "../app/state": { S, sessions },
    "../app/stateQueries": { skipperSplit: () => ({ skipper: shown, child: "x", layout: {} }) },
    "./ledgerStore": { ledgerRows: () => [], ledgerTitle: () => "" },
    "../app/skiffAgent": skiffAgent, "./view": {}, "./layout": {}, "./layoutSlots": {}, "../app/renderRequest": {},
  }, {}, "");
  const kids = () => Array.from(split.splitChildren(), (c: any) => c.id);
  // Project: A's group (the Skiff agent left out), then its loose sessions with the shell, then a2. B's sessions are no child.
  assert.deepEqual(kids(), ["a1g", "a1x", "sh", "a2x"]);
  // Fleet: B's Skipper and sessions, then C's. Its own pane is no child.
  shown = "fsk";
  assert.deepEqual(kids(), ["bsk", "b1x", "c1x"]);
  // The Skiff agent: its own rows in Ledger order, the Skipper it handed off to included; others' rows are no child.
  shown = "agent";
  assert.deepEqual(kids(), ["bsk", "a1x", "c1x"]);
});

test("Skipper split per Skipper: each keeps its own child; back on a Skipper, its child returns, else the next child", () => {
  const sessions = new Map<string, unknown>(["ask", "a1", "a2", "bsk", "b1"].map((id) => [id, { id }]));
  const S: any = { single: null, activeGroup: null, focused: null, projects: [], skipperSplits: new Map() };
  const scopeOf: Record<string, string> = { ask: "A", bsk: "B" };
  const queries = moduleFromSource("../src/app/stateQueries.ts", { "./state": { S, sessions } }, {}, "");
  const split = moduleFromSource("../src/workspace/skipperSplit.ts", {
    "../app/state": { S, sessions }, "../app/stateQueries": queries,
    "./sidebarLines": { scopeOrder: (scope: string) => (scope === "A" ? ["ask", "a1", "a2"] : ["bsk", "b1"]).filter((id) => sessions.has(id)) },
    "./ledgerModel": { ownPanes: () => [] }, "./ledgerStore": { ledgerRows: () => [], ledgerTitle: () => "" },
    "../app/skiffAgent": { coordinatorOf: (id: string) => scopeOf[id] ?? null },
    "./layout": { leaf: (session: string) => ({ type: "pane", session }), sessionsOf: (l: any) => (l.type === "pane" ? [l.session] : [l.a.session, l.b.session]), replacePane: (l: any, id: string, f: any) => ({ ...l, a: l.a.session === id ? f(l.a) : l.a, b: l.b.session === id ? f(l.b) : l.b }) },
    "./layoutSlots": { isSlot: () => false }, "./view": { focusPane() {} }, "../app/renderRequest": { render() {} },
  }, {}, "");
  const go = (sk: string) => { S.single = sk; split.restoreSplit(sk); };
  go("ask"); split.showChild("a2");
  go("bsk"); assert.equal(split.splitChild(), null, "A's child does not follow to B");
  split.showChild("b1");
  go("ask"); assert.equal(split.splitChild(), "a2");
  go("bsk"); assert.equal(split.splitChild(), "b1");
  // A's child went while B was on screen: back on A, the next child shows.
  const after = split.splitLoses("a2"); sessions.delete("a2"); after();
  go("ask"); assert.equal(split.splitChild(), "a1");
  // Closed stays closed.
  split.closeSplit(); go("bsk"); go("ask"); assert.equal(split.splitChild(), null);
});

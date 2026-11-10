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

test("Skipper split children: every other live session, in tree order, each once", () => {
  // Projects A (worktrees a1, a2) and B in fleet F, a session in no project, and the Skiff agent.
  const wt = (path: string) => ({ path });
  const a1 = wt("/a1"), a2 = wt("/a2"), b1 = wt("/b1");
  const A = { name: "A", worktrees: [a1, a2] }, B = { name: "B", worktrees: [b1] };
  const home: Record<string, unknown> = { agent: null, ask: null, fsk: null, a1x: a1, a1y: a1, a2x: a2, bsk: b1, b1x: b1, b1g: b1, loose: null };
  const order = ["a1y", "a1x", "a2x", "agent", "ask", "fsk", "bsk", "b1x", "b1g", "loose"];
  const sessions = new Map<string, unknown>(order.map((id) => [id, { id, started_at: String(order.indexOf(id)) }]));
  const S = { groups: [{ id: "g", layout: { type: "pane", session: "b1g" } }] };
  const skipperOf: Record<string, string> = { A: "ask", B: "bsk", "fleet:F": "fsk" };
  const lines = moduleFromSource("../src/workspace/sidebarLines.ts", {
    "./layout": { sessionsOf: (l: any) => (l ? [l.session] : []) },
    "./model": { byStart: (x: any, y: any) => x.started_at.localeCompare(y.started_at) },
    "../app/state": { S, sessions },
    "../app/stateQueries": { place: (s: any) => (home[s.id] ? { worktree: home[s.id] } : null), awayPlaces: () => [] },
    "./view": { groupHome: () => b1 },
    "../app/skiffAgent": {
      skiffAgentSession: () => sessions.get("agent"), projectSkippers: () => ["ask", "bsk", "fsk"].map((id) => sessions.get(id)),
      projectSkipperOf: (n: string) => sessions.get(skipperOf[n]) ?? null, fleetSkipperOf: (f: string) => sessions.get(skipperOf[`fleet:${f}`]) ?? null,
    },
    "./tree": { treeSections: () => [{ fleet: null, projects: [A] }, { fleet: "F", projects: [B] }] },
  }, {}, "");
  const split = moduleFromSource("../src/workspace/skipperSplit.ts", {
    "./sidebarLines": lines,
    "../app/state": { S: {}, sessions },
    "../app/stateQueries": { skipperSplit: () => ({ skipper: "bsk", child: "a1x", layout: {} }) },
    "./ledgerStore": { ledgerRows: () => [{ pane_id: "a1x", title: "Old" }, { pane_id: "a1x", title: "New" }], ledgerTitle: (r: any) => r.title },
    "../app/skiffAgent": { coordinatorOf: () => null }, "./view": {}, "./layout": {}, "./layoutSlots": {}, "../app/renderRequest": {},
  }, {}, "");
  // The agent, then A's Skipper and worktrees, then fleet F's Skipper, B's grouped and loose sessions, then Other. The Skipper on screen is no child.
  const kids = split.splitChildren();
  assert.deepEqual(Array.from(kids, (c: any) => c.id), ["agent", "ask", "a1y", "a1x", "a2x", "fsk", "b1g", "b1x", "loose"]);
  assert.equal(kids.find((c: any) => c.id === "a1x").title, "New");
});

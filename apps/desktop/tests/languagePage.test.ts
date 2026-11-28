import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource, realControls } from "./workspaceHarness.ts";

// The real settings.ts: the page groups, and the ids of the pages that moved.
class El {
  className: string;
  textContent: string;
  type = "";
  id = "";
  hidden = false;
  children: El[] = [];
  parent: El | null = null;
  dataset: Record<string, string> = {};
  style: Record<string, unknown> = { setProperty() {} };
  classList = { toggle() {}, add() {}, contains: () => false };
  listeners = new Map<string, (() => void)[]>();
  constructor(_tag: string, cls = "", text = "") {
    this.className = cls;
    this.textContent = text;
  }
  private adopt(kids: El[]) {
    for (const k of kids) {
      k.parent?.children.splice(k.parent.children.indexOf(k), 1);
      k.parent = this;
    }
  }
  append(...kids: El[]) { this.adopt(kids); this.children.push(...kids); }
  appendChild(k: El) { this.append(k); return k; }
  replaceChildren(...kids: El[]) { for (const k of this.children) k.parent = null; this.children = []; this.append(...kids); }
  replaceWith(k: El) { const p = this.parent!; p.adopt([k]); p.children.splice(p.children.indexOf(this), 1, k); this.parent = null; }
  after(k: El) { const p = this.parent!; p.adopt([k]); p.children.splice(p.children.indexOf(this) + 1, 0, k); }
  remove() { this.parent?.children.splice(this.parent.children.indexOf(this), 1); this.parent = null; }
  setAttribute() {}
  addEventListener(ev: string, run: () => void) { this.listeners.set(ev, [...(this.listeners.get(ev) ?? []), run]); }
  scrollIntoView() {}
  focus() {}
  querySelector() { return null; }
  querySelectorAll() { return []; }
  contains(k: El) { return this.all().includes(k); }
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

function settingsPage(backend?: (cmd: string, args: any) => unknown, themes: any[] | (() => any[]) = [], o: { cards?: unknown; load?: () => Promise<void> } = {}) {
  const body = h("div");
  const picks: { names: string[]; label: string; shows: string; pick: (v: string) => void }[] = [];
  const controls = realControls(h);
  const picker = (o: { label: string; shows: string; pick: (v: string) => void; entries: () => { label: string }[] }) => {
    picks.push({ names: o.entries().map((e) => e.label), label: o.label, shows: o.shows, pick: o.pick });
    return h("button", "ctl-pick");
  };
  const invoke = async (cmd: string, args?: unknown) => (backend ? backend(cmd, args) : cmd === "get_appearance" ? { language: null } : null);
  const mod = moduleFromSource("../src/appearance/settings.ts", {
    "@tauri-apps/api/core": { invoke },
    "../ui/dom": { h },
    "../ui/dialogParts": { escapeButton: () => h("button", "set-x") },
    "./themeCards": o.cards ?? { themeBrowser: (items: any[]) => ({ el: h("div", "tc-grid", String(items.length)), update() {} }) },
    "./hostSettings": { createHostsTab: () => ({ draw: () => h("div"), load: async () => {}, open() {}, focusServer() {} }) },
    "./diskUse": { diskState: () => ({}) },
    "./imagesPage": { imagesState: () => ({}) },
    "./schedulesPage": { schedulesState: () => ({}), schedulesPage: () => h("div"), loadSchedulesPage: async () => {} },
    "../workspace/scheduleStore": { schedules: [] },
    "./usagePage": { usageCount: () => "", usageSection: () => h("div", "set-main") },
    "../app/usage": { onUsage() {}, usagePage() {} },
    "./daemonActivity": { activityState: () => ({}), stopActivity() {} },
    "../ui/controls": { ...controls, picker },
    "./openWithSettings": moduleFromSource("../src/appearance/openWithSettings.ts", { "../ui/dom": { h }, "../ui/controls": { ...controls, picker }, "@tauri-apps/plugin-dialog": {} }, {}, ""),
    "./settingsLanguage": moduleFromSource("../src/appearance/settingsLanguage.ts", { "../ui/dom": { h }, "../ui/controls": { ...controls, picker } }, { navigator: { language: "en-US" } }, ""),
    "./rowTable": { tableState: () => ({}), sectionHead: (title: string) => { const b = h("div", "set-band"); b.append(h("span", "set-band-title", title)); return b; } },
    "./skiffAgentSettings": { finishRow: () => h("div", "set-finish"), skippersPage: () => h("div", "g-skip"), taskTable: () => h("div", "g-tasks"), addTaskButton: () => h("button", "ctl-btn") },
    "../terminal/images": { onImagesChanged() {}, imageCount: () => 0 },
    "../app/hosts": { isRemote: () => false, hostName: () => "This computer (devbox)", knownHosts: () => [] },
    "../app/hostCaps": { hostCan: () => true, hostInvoke: async (_cap: string, cmd: string) => (cmd === "list_agents" ? [] : null) },
    "../app/state": { S: {}, sessions: new Map() },
  }, {
    document: { getElementById: () => body, activeElement: null, body },
    window: { addEventListener() {} },
    navigator: { language: "en-US" },
    HTMLElement: El, HTMLInputElement: class {}, HTMLButtonElement: class {}, HTMLTextAreaElement: class {},
  }, "");
  const look = { themes: () => (typeof themes === "function" ? themes() : themes), current: () => null, load: o.load ?? (async () => {}), set() {} };
  const settings = mod.createSettings(() => {}, () => {}, look);
  const root = body.children[0];
  return { settings, root, picks };
}

test("the old Language id opens Appearance, and its row picks the language", async () => {
  const s = settingsPage();
  await s.settings.open("language");
  assert.ok(s.root.all().some((e) => e.className === "set-h" && e.textContent === "Appearance"));
  assert.ok(s.picks.some((p) => p.names.join() === "Auto (English),English,Español,简体中文"));
});

test("Task types is its own page, and the nav names the Host and the App", async () => {
  const s = settingsPage();
  await s.settings.open("task-types");
  const names = s.root.all().filter((e) => e.className === "set-tab-name").map((e) => e.textContent);
  assert.equal(names.join(), "Agents,Usage,Skippers,Task types,Schedules,Sessions,Disk use,Files,Appearance,Open with,Hosts");
  assert.equal(s.root.all().filter((e) => e.className === "set-band-title").map((e) => e.textContent).slice(0, 2).join(), "This computer (devbox),App");
  assert.ok(s.root.all().some((e) => e.className === "set-h" && e.textContent === "Task types"));
});

test("a pick on Diffs or Images stores the command and shows it; the rows with a peek setting set it too", async () => {
  const open: Record<string, unknown> = { diff: "", text: "", markdown: "", html: "", image: "", default_diff: "git diff", peek: [] };
  const peekCalls: string[] = [];
  const s = settingsPage((cmd, args) => {
    if (cmd === "open_settings") return { ...open };
    if (cmd === "open_choices") return {};
    if (cmd === "set_open") { open[args.key] = args.command; return { ...open }; }
    if (cmd === "set_open_peek") {
      // As skiff_core::config::set_open_peek: these two have no peek setting.
      if (args.key === "diff" || args.key === "image") throw `unknown file kind: ${args.key}`;
      peekCalls.push(args.key);
      return { ...open };
    }
    return null;
  });
  await s.settings.open("open");
  for (const [label, key] of [["Diffs", "diff"], ["Images", "image"], ["Text files", "text"]]) {
    s.picks.findLast((p) => p.label === label)!.pick(`tool-${key}`);
    await new Promise((done) => setTimeout(done));
    assert.equal(s.root.all().find((e) => e.className === "set-error")?.textContent, undefined, label);
    assert.equal(s.picks.findLast((p) => p.label === label)!.shows, `tool-${key}`, label);
  }
  assert.equal(peekCalls.join(), "text");
});

test("Appearance shows cached theme cards before other settings reads finish", async () => {
  let finish!: (value: unknown) => void;
  const waiting = new Promise((resolve) => { finish = resolve; });
  const page = settingsPage((cmd) => cmd === "open_settings" ? waiting : null, [{ id: "cached" }]);
  const opening = page.settings.open("appearance");
  const count = page.root.all().find((e) => e.className === "tc-grid")?.textContent ?? "absent";
  finish(null);
  await opening;
  assert.equal(count, "1");
});

test("Appearance from its tab draws once: late reads change rows in place and keep the same grid and cards", async () => {
  const theme = (id: string) => ({ id, name: id, background: "#101010", foreground: "#e0e0e0", palette: Array(16).fill("#808080") });
  let themes = [theme("adriatic"), theme("ember"), theme("flamingo")];
  const cards = moduleFromSource("../src/appearance/themeCards.ts", {
    "./colors": { isLight: () => false, mix: (a: string) => a, readable: (c: string) => c },
    "./themeProfile": { COLOR_FAMILIES: ["Blue"], themeProfile: () => ({ family: "Blue", mode: "Dark", color: "#3366cc" }) },
    "../ui/dom": { h },
    "../ui/icons": { icon: () => h("svg") },
  }, {}, "");
  let builds = 0;
  const counted = { ...cards, themeBrowser: (...a: unknown[]) => { builds++; return cards.themeBrowser(...a); } };
  const late: (() => void)[] = [];
  let slow = false;
  const wait = <T>(value: T) => (slow ? new Promise<T>((done) => late.push(() => done(value))) : Promise.resolve(value));
  const page = settingsPage((cmd) => {
    if (cmd === "get_appearance") return wait({ language: slow ? "es" : null, time_zone: "UTC", clock: null });
    if (cmd === "menu_layout") return wait(null);
    return null;
  }, () => themes, {
    cards: counted,
    // The theme read answers new objects with the same colors.
    load: () => wait(undefined).then(() => { themes = themes.map((t) => ({ ...t })); }),
  });
  // Settings was open before: its last page stays drawn while it opens again.
  await page.settings.open("task-types");
  page.settings.close();
  slow = true;
  const opening = page.settings.open("task-types");
  // The person clicks Appearance in the Settings sidebar before the reads answer.
  const tab = page.root.all().find((e) => e.className.startsWith("set-tab") && e.children.some((c) => c.textContent === "Appearance"))!;
  tab.listeners.get("click")!.forEach((run) => run());
  const main = page.root.children[1];
  const grid = page.root.all().find((e) => e.className === "tc-grid")!;
  const before = [...grid.children];
  const langRow = main.children[1];
  assert.equal(before.length, 3);
  for (const done of late.splice(0)) done();
  await opening;
  assert.equal(page.root.children[1], main, "the page is not drawn again");
  assert.equal(page.root.all().find((e) => e.className === "tc-grid"), grid, "the same grid element");
  assert.deepEqual(grid.children, before, "the same card elements");
  assert.equal(builds, 1, "the grid is built once");
  assert.notEqual(main.children[1], langRow, "the language row shows the read in place");
  // Another tab and back: the same grid element.
  page.root.all().find((e) => e.className.startsWith("set-tab") && e.children.some((c) => c.textContent === "Task types"))!.listeners.get("click")!.forEach((run) => run());
  page.root.all().find((e) => e.className.startsWith("set-tab") && e.children.some((c) => c.textContent === "Appearance"))!.listeners.get("click")!.forEach((run) => run());
  assert.equal(page.root.all().find((e) => e.className === "tc-grid"), grid);
  assert.deepEqual(grid.children, before);
  assert.equal(builds, 1);
});

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
  dataset: Record<string, string> = {};
  style = {};
  listeners = new Map<string, (() => void)[]>();
  constructor(_tag: string, cls = "", text = "") {
    this.className = cls;
    this.textContent = text;
  }
  append(...kids: El[]) { this.children.push(...kids); }
  appendChild(k: El) { this.children.push(k); return k; }
  replaceChildren(...kids: El[]) { this.children = kids; }
  setAttribute() {}
  addEventListener(ev: string, run: () => void) { this.listeners.set(ev, [...(this.listeners.get(ev) ?? []), run]); }
  scrollIntoView() {}
  querySelector() { return null; }
  querySelectorAll() { return []; }
  contains() { return false; }
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

function settingsPage(backend?: (cmd: string, args: any) => unknown) {
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
    "./themeCards": { themeGrid: () => h("div", "tc-grid") },
    "./hostSettings": { createHostsTab: () => ({ draw: () => h("div"), load: async () => {}, open() {}, focusServer() {} }) },
    "./diskUse": { diskState: () => ({}) },
    "./imagesPage": { imagesState: () => ({}) },
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
    HTMLElement: El, HTMLInputElement: class {}, HTMLButtonElement: class {},
  }, "");
  const look = { themes: () => [], current: () => null, load: async () => {}, set() {} };
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
  assert.equal(names.join(), "Agents,Skippers,Task types,Sessions,Disk use,Images,Appearance,Open with,Hosts");
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

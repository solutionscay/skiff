// Execute the real UI modules without a webview, filesystem writes or a daemon.
import { existsSync, readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

class Element {
  tag: string;
  className = "";
  textContent = "";
  type = "";
  value = "";
  checked = false;
  disabled = false;
  indeterminate = false;
  children: Element[] = [];
  dataset: Record<string, string> = {};
  parent: Element | null = null;
  listeners = new Map<string, (() => void)[]>();
  document: { activeElement: Element | null };

  constructor(tag: string, document: { activeElement: Element | null }) {
    this.tag = tag;
    this.document = document;
  }
  append(...children: Element[]) {
    for (const child of children) {
      this.children.push(child);
      child.parent = this;
    }
  }
  setAttribute() {}
  removeAttribute() {}
  addEventListener(event: string, run: () => void) {
    this.listeners.set(event, [...(this.listeners.get(event) ?? []), run]);
  }
  fire(event: string) {
    for (const run of this.listeners.get(event) ?? []) run();
  }
  click() { this.fire("click"); }
  focus() { this.document.activeElement = this; }
  select() {}
  remove() {
    if (this.parent) this.parent.children = this.parent.children.filter((child) => child !== this);
    this.parent = null;
  }
  get isConnected(): boolean { return this.tag === "body" || !!this.parent?.isConnected; }
  querySelectorAll() { return []; }
}

// Split facades: `x.ts` keeps its public names and loads children. Tests load the facade alone,
// with an import map keyed by the facade's own specifiers. Each facade lists the children it may load.
// "folder": children live in `dir/<child>.ts` and are named `./dir/<child>` (their own `../y` is the facade's `./y`).
// "sibling": children are sibling files named `./<child>` (their specifiers equal the facade's).
// A child loads only if its file exists, so children can land one at a time.
type Split = { kind: "folder"; dir: string } | { kind: "sibling"; children: string[] };
const SPLIT: Record<string, Split> = {
  "../src/workspace/menus.ts": { kind: "folder", dir: "menus" },
  "../src/workspace/sidebar.ts": { kind: "sibling", children: ["sidebarFocus", "sessionRows", "skipperRows", "sidebarSig", "projectRows", "sectionRows"] },
  "../src/appearance/hostSettings.ts": { kind: "sibling", children: ["hostShared", "hostList", "hostCheck", "hostFixes", "hostForm"] },
  "../src/appearance/settings.ts": { kind: "sibling", children: ["settingsFocus", "settingsLanguage", "settingsAgents"] },
  "../src/terminal/peek.ts": { kind: "sibling", children: ["peekCard", "peekHostView", "peekTerminal"] },
};

/** A folder child's specifier as the facade would write it: `../y` is `./y`, `../../z` is `../z`. */
const asFacade = (name: string) => (!name.startsWith("../") ? name : name.startsWith("../../") ? name.slice(3) : `./${name.slice(3)}`);

export function moduleFromSource(path: string, imports: Record<string, unknown>, globals: Record<string, unknown>, expose: string) {
  const source = readFileSync(new URL(path, import.meta.url), "utf8") + expose;
  const code = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const find = (name: string) => (Object.hasOwn(imports, name) ? imports[name] : words(name) ?? pure(name) ?? imports[name] ?? {});
  const split = SPLIT[path];
  const folder = path.slice(0, path.lastIndexOf("/") + 1);
  const children = new Map<string, unknown>();
  // The file of a listed child, when it exists. Explicit import-map entries win.
  const fileOf = (name: string): string | undefined => {
    if (!split || Object.hasOwn(imports, name)) return undefined;
    const kid = split.kind === "folder" ? name.match(new RegExp(`^\\./${split.dir}/(\\w+)$`))?.[1] : split.children.find((c) => name === `./${c}`);
    const file = kid && `${folder}${split.kind === "folder" ? `${split.dir}/` : ""}${kid}.ts`;
    return file && existsSync(new URL(file, import.meta.url)) ? file : undefined;
  };
  // The real child, once per load, over the facade's import map and globals.
  const load = (file: string): unknown => {
    if (!children.has(file)) {
      const exported = {} as any;
      children.set(file, exported);
      const out = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
      const require = (name: string) => {
        const key = split!.kind === "folder" ? (name.startsWith("./") ? `./${(split as { dir: string }).dir}/${name.slice(2)}` : asFacade(name)) : name;
        const kid = fileOf(key);
        return kid ? load(kid) : find(key);
      };
      vm.runInNewContext(out, { exports: exported, require, ...globals }, { filename: file });
    }
    return children.get(file);
  };
  const require = (name: string) => {
    const file = fileOf(name);
    return file ? load(file) : find(name);
  };
  const context = { exports: {} as any, require, ...globals };
  vm.runInNewContext(code, context, { filename: path });
  return context.exports;
}

// The UI words: the real English catalog, unless a test passes its own.
let catalog: { en: any; i18n: any } | undefined;
function words(name: string) {
  if (!/(^|\/)i18n(\/en)?$/.test(name)) return undefined;
  if (!catalog) {
    const en = moduleFromSource("../src/i18n/en.ts", {}, {}, "");
    const es = moduleFromSource("../src/i18n/es.ts", {}, {}, "");
    const zh = moduleFromSource("../src/i18n/zh.ts", {}, {}, "");
    catalog = { en, i18n: moduleFromSource("../src/i18n/index.ts", { "./en": en, "./es": es, "./zh": zh }, { navigator: { language: "en-US" } }, "") };
  }
  return name.endsWith("/en") ? catalog.en : catalog.i18n;
}

// Pure modules with no Host or DOM access: the real one, unless a test passes its own.
const reports = new Map<string, unknown>();
function pure(name: string) {
  const found = /(^|\/)(programStatus|fileMenu|paths)$/.exec(name)?.[2];
  if (!found) return undefined;
  if (!reports.has(found)) reports.set(found, moduleFromSource(`../src/workspace/${found}.ts`, {}, {}, ""));
  return reports.get(found);
}

export const flush = () => new Promise<void>((resolve) => setImmediate(resolve));

export function workspaceHarness() {
  const document = {
    activeElement: null as Element | null,
    getElementById: () => null,
    querySelector: () => null,
    body: null as unknown as Element,
  };
  const h = (tag: string, cls = "", text = "") => {
    const el = new Element(tag, document);
    el.className = cls;
    el.textContent = text;
    return el;
  };
  document.body = h("body");
  const events = new Map<string, () => void>();
  const files = new Map<string, any>();
  const stored = new Map<string, string>();
  const S = {
    projects: [{ name: "A", path: "/a", fleet: "F" as string | null }, { name: "B", path: "/b", fleet: null as string | null }],
    closedProjects: [], selectedProject: null as string | null,
  };
  const calls = { approvedExits: 0, saves: 0, guarded: false, failSave: false };
  const menus: (() => void)[] = [];
  const window = {
    onCloseRequested: async (run: (e: { preventDefault(): void }) => void) => {
      events.set("window-close", () => run({ preventDefault() {} }));
      return () => {};
    },
  };
  let profile: any;
  const invoke = async (command: string, args: any) => {
    switch (command) {
      case "guard_quit": calls.guarded = events.has("skiff:quit-requested"); return;
      case "quit_app": calls.approvedExits++; return;
      case "workspace_launch_file": return null;
      case "workspace_read": {
        const value = files.get(args.path);
        if (!value) throw new Error("file missing");
        return structuredClone(value);
      }
      case "workspace_write":
        calls.saves++;
        if (calls.failSave) throw new Error("disk full");
        files.set(args.path, { path: args.path, profile: structuredClone(args.profile), revision: "saved" });
        return "saved";
      default: throw new Error(`Unexpected command: ${command}`);
    }
  };
  const tree = {
    fleetKey: (name: string) => `fleet:${name}`, projectKey: (name: string) => `project:${name}`,
    useTreeView() {}, treeSections: () => [{ projects: S.projects.filter((p) => profile.shows(p)) }],
  };
  const profileState = moduleFromSource("../src/workspace/profile/state.ts", {
    "../../app/renderRequest": { render() {} }, "../../app/state": { S }, "../tree": tree,
  }, {}, "");
  const profileMembership = moduleFromSource("../src/workspace/profile/membership.ts", {
    "../../app/state": { S }, "../../ui/contextMenu": { ctxMenu: { open(_x: number, _y: number, _title: string, _entries: unknown, done: () => void) { menus.push(done); } } },
  }, {}, "");
  const profileStorage = moduleFromSource("../src/workspace/profile/storage.ts", {
    "@tauri-apps/api/core": { invoke }, "./state": profileState,
  }, { localStorage: { getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value), removeItem: (key: string) => stored.delete(key) } }, "");
  const profileDialogs = moduleFromSource("../src/workspace/profile/dialogs.ts", {
    "../../ui/dialogParts": { dialogFrame: () => ({ overlay: h("div"), panel: h("div") }) },
    "../../ui/dom": { h, button: (cls: string, text: string, run: () => void) => { const b = h("button", cls, text); b.addEventListener("click", run); return b; } },
  }, { document, HTMLButtonElement: Element }, "");
  const imports = {
    "@tauri-apps/api/core": { invoke },
    "@tauri-apps/api/event": { listen: async (name: string, run: () => void) => { events.set(name, run); return () => {}; } },
    "@tauri-apps/api/window": { getCurrentWindow: () => window },
    "@tauri-apps/plugin-dialog": { save: async () => null },
    "../app/state": { S },
    "../app/renderRequest": { render() {} },
    "../ui/alerts": { showError() {} },
    "../ui/contextMenu": { ctxMenu: { open(_x: number, _y: number, _title: string, _entries: unknown, done: () => void) { menus.push(done); } } },
    "../ui/dom": { h, button: (cls: string, text: string, run: () => void) => {
      const b = h("button", cls, text); b.addEventListener("click", run); return b;
    } },
    "../ui/dialogParts": { dialogFrame: () => ({ overlay: h("div"), panel: h("div") }) },
    "./tree": tree,
    "./view": { selectProject: (name: string) => { S.selectedProject = name; } },
    "./profile/state": profileState,
    "./profile/membership": profileMembership,
    "./profile/storage": profileStorage,
    "./profile/dialogs": profileDialogs,
  };
  const globals = { document, requestAnimationFrame: (run: () => void) => run(), window: { addEventListener() {} }, setInterval() {},
    localStorage: { getItem: (key: string) => stored.get(key) ?? null,
      setItem: (key: string, value: string) => stored.set(key, value), removeItem: (key: string) => stored.delete(key) } };
  const childImports = {
    "@tauri-apps/api/core": imports["@tauri-apps/api/core"], "@tauri-apps/api/event": imports["@tauri-apps/api/event"],
    "@tauri-apps/api/window": imports["@tauri-apps/api/window"], "@tauri-apps/plugin-dialog": imports["@tauri-apps/plugin-dialog"],
    "../../app/state": imports["../app/state"], "../../app/renderRequest": imports["../app/renderRequest"],
    "../../ui/alerts": imports["../ui/alerts"], "../../ui/contextMenu": imports["../ui/contextMenu"], "../../ui/dom": imports["../ui/dom"],
    "../tree": tree, "../view": imports["./view"], "./state": profileState, "./storage": profileStorage, "./dialogs": profileDialogs,
  };
  const transitions = moduleFromSource("../src/workspace/profile/transitions.ts", childImports, globals, "");
  const status = moduleFromSource("../src/workspace/profile/status.ts", childImports, globals, "");
  Object.assign(imports, { "./profile/transitions": transitions, "./profile/status": status });
  profile = moduleFromSource("../src/workspace/profile.ts", imports, {
    document, requestAnimationFrame: (run: () => void) => run(),
    window: { addEventListener() {} }, setInterval() {},
    localStorage: { getItem: (key: string) => stored.get(key) ?? null,
      setItem: (key: string, value: string) => stored.set(key, value), removeItem: (key: string) => stored.delete(key) },
  }, "");
  const elements = (): Element[] => {
    const walk = (el: Element): Element[] => [el, ...el.children.flatMap(walk)];
    return walk(document.body);
  };
  const button = (label: string) => {
    const found = elements().find((el) => el.tag === "button" && el.textContent === label);
    if (!found) throw new Error(`No button: ${label}`);
    return found;
  };
  const fixture = (path: string, projects = ["/a"], fleets: string[] = []) => {
    files.set(path, { path, revision: "read", profile: {
      name: path, projects, fleets, folded_projects: [], folded_fleets: [], selected: null,
    } });
  };
  return { profile, S, calls, files, events, button, elements, fixture };
}

import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The Task types of the real skippersPage in skiffAgentSettings.ts, with the
// table stubbed to keep its spec. Each row's Instructions cell shows the file's
// size or none, and its end cell has Edit. No help text.
class El {
  children: (El | string)[] = [];
  listeners = new Map<string, () => void>();
  attrs: Record<string, string> = {};
  type = "";
  id = "";
  tag: string;
  className: string;
  textContent: string;
  constructor(tag: string, className = "", textContent = "") {
    this.tag = tag;
    this.className = className;
    this.textContent = textContent;
  }
  append(...kids: (El | string)[]) { this.children.push(...kids); }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  addEventListener(ev: string, run: () => void) { this.listeners.set(ev, run); }
  text(): string { return [this.textContent, ...this.children.map((c) => (typeof c === "string" ? c : c.text()))].join(""); }
  find(cls: string): El | undefined {
    for (const c of this.children) {
      if (typeof c === "string") continue;
      if (c.className.split(" ").includes(cls)) return c;
      const f = c.find(cls);
      if (f) return f;
    }
  }
}

function section(errors = new Map<string, string>()) {
  let spec: any = null;
  const edited: string[] = [];
  const after: string[] = [];
  const table = new El("div");
  (table as any).querySelector = (sel: string) => (sel.includes('"review"') ? { after: (m: El) => after.push(m.text()) } : null);
  const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);
  const m = moduleFromSource(
    "../src/appearance/skiffAgentSettings.ts",
    {
      "../ui/dom": { h },
      "../ui/controls": { smallButton: (text: string, run: () => void) => { const b = h("button", "ctl-btn", text); b.addEventListener("click", run); return b; }, picker: () => h("button", "ctl-pick"), switchButton: () => h("button", "ctl-switch"), textBox: () => h("input", "ctl-field") },
      "./rowTable": { rowTable: (s: any) => ((spec = s), table), sectionHead: (title: string) => { const b = h("div", "set-band", title); (b as any).classList = { add() {} }; return b; } },
      "./taskTypes": { fileSize: (n: number) => `${n} B`, modelFlag: (a: any) => (a?.id === "claude" ? "--model" : null), modelError: () => null, TASK_TYPE_NAME: /./, badTaskTypeName: () => "" },
      "../app/skiffAgent": { DEFAULT_NAME: "Admiral", mainName: (n?: string | null) => n?.trim() || "Admiral", skiffAgentChoice: () => null },
      "../app/hosts": { isRemote: () => false },
      "../app/state": { S: { projects: [] } },
      "../workspace/skipperFace": { avatarScopes: () => [], skipperFace: () => h("span", "skipper-face") },
    },
    { console, CSS: { escape: (s: string) => s } },
    "",
  );
  const t = (type: string, instructions: number | null) => ({ type, agent: "claude", model: null, file: `/home/me/skiff-agent/types/${type}.md`, instructions, command: "claude", error: null });
  const set = { types: "~/skiff-agent/types/", tasks: [t("review", 412), t("spec", null)], agent: "claude" };
  const agents = [{ id: "claude", enabled: true }, { id: "gemini", enabled: true }, { id: "opencode", enabled: true }];
  m.skippersPage(set, agents, { save() {}, saveTask: async () => null, redraw() {}, render() {}, tasksTable: {}, files: { files: [], errors: new Map(), editable: true, edit() {}, reset() {} }, types: { edit: (x: any) => edited.push(x.type), errors } });
  const col = spec.cols.find((c: any) => c.key === "instructions");
  return { spec, col, rows: set.tasks, edited, after };
}

test("a row shows the size or none, and Edit opens its file", () => {
  const { col, rows, edited, spec } = section();
  assert.equal(spec.cols.map((c: any) => c.key).join(","), "type,agent,model,instructions");
  assert.equal(col.draw(rows[0]).text(), "412 B");
  const none = col.draw(rows[1]);
  assert.equal(none.text(), "none");
  assert.ok(none.className.includes("none"));
  spec.status(rows[1]).listeners.get("click")();
  assert.deepEqual(edited, ["spec"]);
});

test("a model set by its agent does not refer to a command column", () => {
  const { spec } = section();
  const model = spec.cols.find((c: any) => c.key === "model");
  assert.equal(model.note({ type: "web-research", agent: "opencode", model: null }), "set by agent");
});

test("an Edit fault shows one line under its row", () => {
  const { after } = section(new Map([["review", "No app opens review.md."]]));
  assert.deepEqual(after, ["No app opens review.md."]);
});

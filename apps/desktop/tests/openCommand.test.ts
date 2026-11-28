import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// #303: Settings › Open with › Custom command… and Edit command… open an
// editor in the table, under the row: the command field and its buttons, no
// other choice. Save stores the program and its words, and keeps its place;
// a diff program with {target} takes the Git target there, one without it
// reads the patch. Cancel and Esc store nothing and give the keys back to
// the row's picker; Reset to default clears the row; the editor shows its
// errors. A saved pipeline that Skiff runs as Git and a renderer opens as the
// renderer.
class El {
  tag: string;
  className: string;
  textContent: string;
  children: El[] = [];
  parent: El | null = null;
  value = "";
  placeholder = "";
  hidden = false;
  disabled = false;
  attrs: Record<string, string> = {};
  on: Record<string, ((e: any) => void)[]> = {};
  enter?: () => void;
  click?: () => void;
  constructor(tag: string, cls = "", text = "") { this.tag = tag; this.className = cls; this.textContent = text; }
  append(...kids: El[]) { for (const k of kids) { k.parent = this; this.children.push(k); } }
  appendChild(k: El) { this.append(k); return k; }
  replaceChildren(...kids: El[]) { this.children = []; this.append(...kids); }
  after(k: El) { const p = this.parent!; k.parent = p; p.children.splice(p.children.indexOf(this) + 1, 0, k); }
  remove() { if (this.parent) this.parent.children.splice(this.parent.children.indexOf(this), 1); this.parent = null; }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  removeAttribute(k: string) { delete this.attrs[k]; }
  addEventListener(type: string, f: (e: any) => void) { (this.on[type] ??= []).push(f); }
  focus() { focused = this; }
  querySelector(sel: string): El | null { for (const k of this.children) { if (k.tag === sel) return k; const f = k.querySelector(sel); if (f) return f; } return null; }
  all(): El[] { return this.children.flatMap((k) => [k, ...k.all()]); }
}
let focused: El | null = null;
/** A value from the page's realm, as plain data. */
const plain = (x: unknown) => JSON.parse(JSON.stringify(x));
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);
const tick = () => new Promise((r) => setTimeout(r, 0));

type Pick = { label: string; value: string; entries: () => any[]; pick: (v: string) => void; el: El };

function page(stored: any) {
  const picks: Pick[] = [];
  const calls: string[] = [];
  const mod = moduleFromSource("../src/appearance/openWithSettings.ts", {
    "../ui/dom": { h },
    "../ui/controls": {
      picker: (o: Pick) => { o.el = h("button", "ctl-pick"); picks.push(o); return o.el; },
      textBox: (o: { value: string; placeholder?: string; enter: () => void }) => { const f = h("input", "ctl-field"); f.value = o.value; f.placeholder = o.placeholder ?? ""; f.enter = o.enter; return f; },
      smallButton: (text: string, run: () => void) => { const b = h("button", "ctl-btn", text); b.click = run; return b; },
    },
    "@tauri-apps/plugin-dialog": {},
  }, {}, "");
  const acts = {
    set() {}, setCommand() {}, setApp() {},
    // As set_open_command and set_open do: a program replaces the row's line, a reset clears both.
    async saveCommand(key: string, run: any) {
      calls.push("save");
      if (run.program === "nope") throw "program-missing";
      stored.commands = { ...stored.commands, [key]: run };
      stored[key] = "";
    },
    async saveLine(key: string, line: string) {
      calls.push("line");
      stored[key] = line;
    },
    async reset(key: string) {
      calls.push("reset");
      delete stored.commands?.[key];
      stored[key] = "";
    },
  };
  const main = mod.openWithPage(stored, {}, null, acts) as El;
  const picker = (label: string) => picks.find((p) => p.label === label)!;
  const item = (label: string, name: string) => picker(label).entries().find((e: any) => e.label === name);
  const editor = () => main.children.find((k) => k.className === "open-edit");
  const field = () => editor()!.querySelector("input")!;
  const button = (text: string) => editor()!.all().find((k) => k.tag === "button" && k.textContent === text)!;
  const shown = (cls: string) => editor()!.children.find((k) => k.className === cls && !k.hidden)?.textContent;
  const key = (k: string) => editor()!.on.keydown.forEach((f) => f({ key: k, preventDefault() {}, stopPropagation() {} }));
  return { main, calls, picker, item, editor, field, button, shown, key };
}

const settings = () => ({ diff: "", text: "code", markdown: "", html: "", image: "", default_diff: "git diff", peek: ["text"], commands: {} as Record<string, unknown> });

test("Custom command… opens under the row and saves the program and its words; {target} takes the Git target", async () => {
  const stored = settings();
  const p = page(stored);
  assert.equal(p.item("Diffs", "Edit command…"), undefined, "the default has nothing to edit");
  p.item("Diffs", "Custom command…").run();
  const rows = p.main.children;
  assert.equal(rows[rows.indexOf(p.editor()!) - 1].children[0].textContent, "Diffs", "the editor is in the table, under its row");
  assert.equal(focused, p.field());
  assert.deepEqual(p.editor()!.all().filter((k) => k.tag === "button").map((k) => k.textContent), ["Reset to default", "Cancel", "Save"]);
  assert.equal(p.field().placeholder, "");
  p.field().value = `difft --color=always '/my dir/x' {target}`;
  p.button("Save").click!();
  await tick();
  assert.deepEqual(plain(stored.commands.diff), { program: "difft", args: ["--color=always", "/my dir/x", "{target}"] });
  assert.equal(p.editor(), undefined, "Save closes the editor");
});

test("Cancel and Esc store nothing and give the keys back to the picker; Reset to default clears the row", async () => {
  const stored = settings();
  const p = page(stored);
  p.item("Text files", "Custom command…").run();
  p.field().value = "vim";
  p.button("Cancel").click!();
  assert.deepEqual([p.calls, stored.commands, stored.text, p.editor()], [[], {}, "code", undefined]);
  assert.equal(focused, p.picker("Text files").el);
  p.item("Text files", "Edit command…").run();
  p.field().value = "vim";
  p.key("Escape");
  assert.deepEqual([p.calls, stored.text, p.editor()], [[], "code", undefined]);
  assert.equal(focused, p.picker("Text files").el);
  p.item("Text files", "Edit command…").run();
  p.button("Reset to default").click!();
  await tick();
  assert.deepEqual([p.calls, stored.text, p.editor()], [["reset"], "", undefined]);
});

test("Edit command… opens with the current program, keeps its place, and Enter saves", async () => {
  const stored = settings();
  stored.commands.text = { program: "/opt/my ed", args: ["-n", "{path}"], opens: "app" };
  const p = page(stored);
  p.item("Text files", "Edit command…").run();
  assert.equal(p.field().value, "'/opt/my ed' -n {path}");
  p.field().value += " --wait";
  p.field().enter!();
  await tick();
  assert.deepEqual(plain(stored.commands.text), { program: "/opt/my ed", args: ["-n", "{path}", "--wait"], opens: "app" });
});

test("opening another row's editor closes the first one", () => {
  const p = page(settings());
  p.item("Diffs", "Custom command…").run();
  p.item("Markdown", "Custom command…").run();
  assert.equal(p.main.children.filter((k) => k.className === "open-edit").length, 1);
  assert.equal(p.picker("Markdown").label, "Markdown");
  const rows = p.main.children;
  assert.equal(rows[rows.indexOf(p.editor()!) - 1].children[0].textContent, "Markdown");
});

test("the editor shows an empty command, an open quote, a shell sign and a missing program under the field", async () => {
  const stored = settings();
  const p = page(stored);
  p.item("Diffs", "Custom command…").run();
  const fails = async (line: string) => { p.field().value = line; p.button("Save").click!(); await tick(); return p.shown("set-error"); };
  assert.equal(await fails("   "), "The command is empty. Type a program and its arguments.");
  assert.equal(await fails("delta 'x"), "A quote is not closed. Close the quote.");
  assert.equal(await fails("delta | less"), "Skiff runs the program without a shell. Remove |.");
  assert.equal(await fails("nope --x"), "nope is not found. Install it or type its full path.");
  assert.deepEqual([p.calls, stored.commands], [["save"], {}]);
});

test("a saved pipeline opens as the renderer and saves so with its Git options; {target} drops them", async () => {
  const stored = settings();
  stored.diff = "git diff --color=always {target} | delta --paging=never";
  const p = page(stored);
  p.item("Diffs", "Edit command…").run();
  assert.equal(p.field().value, "delta --paging=never");
  p.button("Save").click!();
  await tick();
  assert.deepEqual(plain(stored.commands.diff), { program: "delta", args: ["--paging=never"], git_args: ["--color=always"] });
  assert.equal(stored.diff, "");
  p.item("Diffs", "Edit command…").run();
  p.field().value = "difft {target}";
  p.button("Save").click!();
  await tick();
  assert.deepEqual(plain(stored.commands.diff), { program: "difft", args: ["{target}"] });
});

test("a line that needs the shell opens as it is, with no note, and saves unchanged", async () => {
  const stored = settings();
  stored.diff = "git diff {target} | delta | cat";
  const p = page(stored);
  p.item("Diffs", "Edit command…").run();
  assert.equal(p.field().value, "git diff {target} | delta | cat");
  assert.equal(p.editor()!.children.length, 3, "the command row, the error and the buttons");
  p.button("Save").click!();
  await tick();
  assert.deepEqual([p.calls, stored.diff, p.editor()], [["line"], "git diff {target} | delta | cat", undefined]);
});

test("a row shows the program that runs, with its Git options, and edits that one", () => {
  const stored = settings();
  stored.commands.diff = { program: "delta", args: ["--side-by-sude"] };
  const p = page(stored);
  assert.equal((p.picker("Diffs") as any).shows, "delta --side-by-sude");
  p.item("Diffs", "Edit command…").run();
  assert.equal(p.field().value, "delta --side-by-sude");
  const q = page({ ...settings(), commands: { diff: { program: "delta", args: [], git_args: ["--color=always"] } } });
  assert.equal((q.picker("Diffs") as any).shows, "git diff --color=always {target} | delta");
});

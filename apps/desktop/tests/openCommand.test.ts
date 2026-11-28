import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// #303: Settings › Open with › Custom command… and Edit command…: Save stores
// the program, its words, its input and its place; Cancel stores nothing;
// Reset to default clears the row; the editor shows its errors.
class El {
  className: string;
  textContent: string;
  children: El[] = [];
  value = "";
  placeholder = "";
  constructor(_tag: string, cls = "", text = "") { this.className = cls; this.textContent = text; }
  append(...kids: El[]) { this.children.push(...kids); }
  appendChild(k: El) { this.children.push(k); return k; }
  replaceChild(k: El, old: El) { this.children[this.children.indexOf(old)] = k; }
  get lastChild() { return this.children[this.children.length - 1]; }
  setAttribute() {}
}
/** A value from the page's realm, as plain data. */
const plain = (x: unknown) => JSON.parse(JSON.stringify(x));
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

type Pick = { label: string; value: string; entries: () => any[]; pick: (v: string) => void };
type Dialog = { title: string; content: El[]; submit: () => Promise<void>; also: { label: string; run: () => Promise<void> } };

function page(stored: any) {
  const picks: Pick[] = [];
  const dialogs: Dialog[] = [];
  const fields: El[] = [];
  const calls: string[] = [];
  const mod = moduleFromSource("../src/appearance/openWithSettings.ts", {
    "../ui/dom": { h },
    "../ui/controls": {
      picker: (o: Pick) => { picks.push(o); return h("button", "ctl-pick"); },
      textBox: (o: { value: string; placeholder: string }) => { const f = h("input", "ctl-field"); f.value = o.value; f.placeholder = o.placeholder; fields.push(f); return f; },
    },
    "../ui/dialogParts": { actionDialog: (o: Dialog) => { dialogs.push(o); return {}; } },
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
    async reset(key: string) {
      calls.push("reset");
      delete stored.commands?.[key];
      stored[key] = "";
    },
  };
  mod.openWithPage(stored, {}, null, acts);
  const menu = (label: string) => picks.find((p) => p.label === label)!.entries();
  const item = (label: string, name: string) => menu(label).find((e) => e.label === name);
  return { mod, picks, dialogs, fields, calls, item };
}

const settings = () => ({ diff: "", text: "code", markdown: "", html: "", image: "", default_diff: "git diff", peek: ["text"], commands: {} as Record<string, unknown> });

test("Custom command… saves the program, its words in order and the Git target input", async () => {
  const stored = settings();
  const p = page(stored);
  assert.equal(p.item("Diffs", "Edit command…"), undefined, "the default has nothing to edit");
  p.item("Diffs", "Custom command…").run();
  const d = p.dialogs[0];
  assert.equal(d.title, "Diffs: command");
  p.fields[0].value = `difft --color=always '/my dir/x' {target}`;
  p.picks.find((x) => x.label === "Input")!.pick("target");
  assert.equal(p.fields[0].placeholder, "difft {target}");
  await d.submit();
  assert.deepEqual(plain(stored.commands.diff), { program: "difft", args: ["--color=always", "/my dir/x", "{target}"], input: "target" });
});

test("Cancel stores nothing, and Reset to default clears the row", async () => {
  const stored = settings();
  const p = page(stored);
  p.item("Text files", "Custom command…").run();
  p.fields[0].value = "vim";
  // Cancel closes the dialog without its submit.
  assert.deepEqual([p.calls, stored.commands, stored.text], [[], {}, "code"]);
  await p.dialogs[0].also.run();
  assert.equal(p.dialogs[0].also.label, "Reset to default");
  assert.deepEqual([p.calls, stored.text], [["reset"], ""]);
});

test("Edit command… opens with the current program and keeps its place", async () => {
  const stored = settings();
  stored.commands.text = { program: "/opt/my ed", args: ["-n", "{path}"], input: "path", opens: "app" };
  const p = page(stored);
  p.item("Text files", "Edit command…").run();
  assert.equal(p.fields[0].value, "'/opt/my ed' -n {path}");
  assert.equal(p.picks.find((x) => x.label === "Opens in")!.value, "app");
  p.picks.find((x) => x.label === "Opens in")!.pick("peek");
  p.fields[0].value += " --wait";
  await p.dialogs[0].submit();
  assert.deepEqual(plain(stored.commands.text), { program: "/opt/my ed", args: ["-n", "{path}", "--wait"], input: "path", opens: "peek" });
});

test("the editor shows an empty command, an open quote, a shell sign and a missing program", async () => {
  const stored = settings();
  const p = page(stored);
  p.item("Diffs", "Custom command…").run();
  const d = p.dialogs[0];
  const fails = async (line: string) => { p.fields[0].value = line; return d.submit().then(() => "saved", (e) => String(e)); };
  assert.equal(await fails("   "), "The command is empty. Type a program and its arguments.");
  assert.equal(await fails("delta 'x"), "A quote is not closed. Close the quote.");
  assert.equal(await fails("delta | less"), "Skiff runs the program without a shell. Remove |.");
  assert.equal(await fails("nope --x"), "nope is not found. Install it or type its full path.");
  assert.deepEqual([p.calls, stored.commands], [["save"], {}]);
});

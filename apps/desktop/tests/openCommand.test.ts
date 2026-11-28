import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource, realControls } from "./workspaceHarness.ts";

// #303: each Settings › Open with row is one command field with a list. The
// field shows what runs. Enter saves, and blur saves changed text; Esc puts
// the saved text back. The list (▾, Alt+Down) picks a preset or the default,
// and a pick saves at once. A typed program saves with its words; a diff
// program with {target} takes the Git target there, one without it reads the
// patch. Errors show in the field's cell and save nothing.
class El {
  tag: string;
  className: string;
  textContent: string;
  children: El[] = [];
  parent: El | null = null;
  value = "";
  placeholder = "";
  type = "";
  hidden = false;
  disabled = false;
  spellcheck = true;
  autocomplete = "";
  dataset: Record<string, string> = {};
  attrs: Record<string, string> = {};
  on: Record<string, ((e: any) => void)[]> = {};
  classList = {
    toggle: (c: string, on: boolean) => { const has = this.className.split(" ").includes(c); if (on && !has) this.className += " " + c; if (!on && has) this.className = this.className.split(" ").filter((x) => x !== c).join(" "); },
    contains: (c: string) => this.className.split(" ").includes(c),
  };
  constructor(tag: string, cls = "", text = "") { this.tag = tag; this.className = cls; this.textContent = text ?? ""; }
  append(...kids: El[]) { for (const k of kids) { k.parent = this; this.children.push(k); } }
  appendChild(k: El) { this.append(k); return k; }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  removeAttribute(k: string) { delete this.attrs[k]; }
  addEventListener(type: string, f: (e: any) => void) { (this.on[type] ??= []).push(f); }
  fire(type: string, e: any = {}) { const ev = { preventDefault() { ev.prevented = true; }, stopPropagation() {}, prevented: false, ...e }; (this.on[type] ?? []).forEach((f) => f(ev)); return ev; }
  focus() { focused = this; }
  getBoundingClientRect() { return { left: 0, bottom: 0 }; }
  all(): El[] { return this.children.flatMap((k) => [k, ...k.all()]); }
}
let focused: El | null = null;
/** A value from the page's realm, as plain data. */
const plain = (x: unknown) => JSON.parse(JSON.stringify(x));
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);
const tick = () => new Promise((r) => setTimeout(r, 0));

function page(stored: any, found: any = {}) {
  const calls: string[] = [];
  let menu: any[] = [];
  const mod = moduleFromSource("../src/appearance/openWithSettings.ts", {
    "../ui/dom": { h },
    "../ui/controls": realControls(h, { open: (_x: number, _y: number, _title: string, items: any[]) => { menu = items; } }),
    "@tauri-apps/plugin-dialog": {},
  }, {}, "");
  const acts = {
    set(key: string, command: string, peek: boolean) { calls.push(`set ${key} ${JSON.stringify(command)} ${peek}`); },
    setCommand(key: string, run: any) { calls.push(`setCommand ${key} ${run.program}`); },
    setApp() {},
    // As set_open_command and set_open do: a program replaces the row's line, a line clears the program.
    async saveCommand(key: string, run: any) {
      calls.push("save");
      if (run.program === "nope") throw "program-missing";
      stored.commands = { ...stored.commands, [key]: run };
      stored[key] = "";
    },
    async saveLine(key: string, line: string, peek: boolean) {
      calls.push(`line ${JSON.stringify(line)} ${peek}`);
      delete stored.commands?.[key];
      stored[key] = line;
    },
  };
  const main = mod.openWithPage(stored, found, null, acts) as El;
  const row = (label: string) => main.children.find((r) => r.children[0]?.textContent === label)!;
  const combo = (label: string) => row(label).all().find((k) => k.classList.contains("ctl-combo"))!;
  const field = (label: string) => combo(label).children[0];
  const error = (label: string) => { const e = combo(label).children[1]; return e.hidden ? null : e.textContent; };
  const type = (label: string, text: string) => { field(label).value = text; field(label).fire("input"); };
  const key = (label: string, k: string, mods: any = {}) => field(label).fire("keydown", { key: k, ...mods });
  const list = (label: string) => { key(label, "ArrowDown", { altKey: true }); return menu; };
  return { main, calls, row, combo, field, error, type, key, list };
}

const settings = () => ({ diff: "", text: "code", markdown: "", html: "", image: "", default_diff: "git diff", peek: ["text"], commands: {} as Record<string, unknown> });

test("each row is one field with what runs; no editor row, no Custom or Edit entries", () => {
  const stored = settings();
  stored.commands.diff = { program: "delta", args: [], git_args: ["--color=always"] };
  const p = page(stored);
  assert.equal(p.field("Diffs").value, "git diff --color=always {target} | delta");
  assert.equal(p.field("Text files").value, "code");
  assert.deepEqual([p.field("Markdown").value, p.field("Markdown").placeholder], ["", "Default app"]);
  assert.equal(p.main.children.length, 2 + 5, "the head, the column heads and the five rows");
  assert.deepEqual(plain(p.list("Markdown").map((e: any) => e.label).filter(Boolean)), ["Default app", "Choose app…"]);
});

test("type and Enter saves the program and its words; {target} takes the Git target; blur saves only changed text", async () => {
  const stored = settings();
  const p = page(stored);
  p.type("Diffs", `difft --color=always '/my dir/x' {target}`);
  p.key("Diffs", "Enter");
  await tick();
  assert.deepEqual(plain(stored.commands.diff), { program: "difft", args: ["--color=always", "/my dir/x", "{target}"] });
  p.field("Text files").fire("blur");
  await tick();
  assert.deepEqual(p.calls, ["save"], "blur with the saved text saves nothing");
  p.type("Text files", "vim -n {path}");
  p.field("Text files").fire("blur");
  await tick();
  assert.deepEqual(plain(stored.commands.text), { program: "vim", args: ["-n", "{path}"], opens: "peek" });
});

test("Esc puts the saved text back and saves nothing", async () => {
  const stored = settings();
  const p = page(stored);
  p.type("Text files", "vim");
  assert.ok(p.combo("Text files").classList.contains("edited"), "Settings lets this Esc through");
  assert.ok(p.key("Text files", "Escape").prevented);
  assert.equal(p.field("Text files").value, "code");
  assert.ok(!p.combo("Text files").classList.contains("edited"), "the next Esc closes Settings");
  p.field("Text files").fire("blur");
  await tick();
  assert.deepEqual([p.calls, stored.text, stored.commands], [[], "code", {}]);
});

test("a preset from the list saves at once, and so does its text typed in the field", async () => {
  const delta = { label: "delta", command: "", run: { program: "delta", args: ["--paging=never"] } };
  const glow = { label: "Glow", command: "glow {path}" };
  const p = page(settings(), { diff: { peek: [delta], apps: [] }, markdown: { peek: [glow], apps: [] } });
  p.list("Diffs").find((e: any) => e.label === "delta").run();
  p.list("Markdown").find((e: any) => e.label === "Glow").run();
  assert.deepEqual(p.calls, ["setCommand diff delta", `set markdown "glow {path}" true`]);
  p.type("Markdown", "glow {path}");
  p.key("Markdown", "Enter");
  await tick();
  assert.equal(p.calls[2], `line "glow {path}" true`);
});

test("Default from the list clears the row; an empty field saves the default", async () => {
  const stored = settings();
  const p = page(stored);
  p.list("Text files").find((e: any) => e.label === "Default app").run();
  assert.deepEqual(p.calls, [`set text "" false`]);
  p.type("Text files", "  ");
  p.key("Text files", "Enter");
  await tick();
  assert.deepEqual([p.calls[1], p.error("Text files")], [`line "" false`, null]);
});

test("an open quote, a shell sign and a missing program show in the cell and save nothing", async () => {
  const stored = settings();
  const p = page(stored);
  const fails = async (line: string) => { p.type("Diffs", line); p.key("Diffs", "Enter"); await tick(); return p.error("Diffs"); };
  assert.equal(await fails("delta 'x"), "A quote is not closed. Close the quote.");
  assert.equal(p.field("Diffs").attrs["aria-invalid"], "true");
  assert.equal(p.field("Diffs").attrs["aria-description"], "A quote is not closed. Close the quote.");
  assert.equal(await fails("delta | less"), "Skiff runs the program without a shell. Remove |.");
  assert.equal(await fails("nope --x"), "nope is not found. Install it or type its full path.");
  p.field("Diffs").fire("blur");
  await tick();
  assert.deepEqual([p.calls, stored.commands], [["save"], {}], "blur does not try the failed text again");
  p.key("Diffs", "Escape");
  assert.deepEqual([p.field("Diffs").value, p.error("Diffs"), p.field("Diffs").attrs["aria-invalid"]], ["", null, undefined]);
});

test("a saved pipeline shows as it runs and saves with its Git options; without them, the program reads the patch", async () => {
  const stored = settings();
  stored.diff = "git diff --color=always {target} | delta --paging=never";
  const p = page(stored);
  p.type("Diffs", "git diff --color=always {target} | delta");
  p.key("Diffs", "Enter");
  await tick();
  assert.deepEqual(plain(stored.commands.diff), { program: "delta", args: [], git_args: ["--color=always"] });
  p.type("Diffs", "delta");
  p.key("Diffs", "Enter");
  await tick();
  assert.deepEqual(plain(stored.commands.diff), { program: "delta", args: [] });
});

test("a line that needs the shell shows as it is and saves as a line", async () => {
  const stored = settings();
  stored.diff = "git diff {target} | delta | cat";
  const p = page(stored);
  assert.equal(p.field("Diffs").value, "git diff {target} | delta | cat");
  p.type("Diffs", "git diff {target} | delta | less");
  p.key("Diffs", "Enter");
  await tick();
  assert.deepEqual([p.calls, stored.diff], [[`line "git diff {target} | delta | less" true`], "git diff {target} | delta | less"]);
});

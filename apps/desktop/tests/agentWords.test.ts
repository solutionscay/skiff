import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// Settings › Agents: agent titles and agent names are two plain text areas,
// one word per line. Leaving one saves it to the same localStorage list that
// new sessions read their call signs from.
class El {
  children: El[] = [];
  attrs: Record<string, string> = {};
  listeners: Record<string, (() => void)[]> = {};
  spellcheck = true;
  value = "";
  className: string;
  textContent: string;
  tag: string;
  constructor(tag: string, cls = "", text = "") { this.tag = tag; this.className = cls; this.textContent = text; }
  append(...kids: El[]) { this.children.push(...kids); }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  addEventListener(k: string, f: () => void) { (this.listeners[k] ??= []).push(f); }
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
  querySelector(sel: string) { return this.all().find((e) => `.${e.className}` === sel) ?? null; }
  blur() { for (const f of this.listeners.blur ?? []) f(); }
}
const h = (tag: string, cls = "", text = "") => new El(tag, cls, text);

function page(stored: Record<string, string> = {}) {
  const store = new Map(Object.entries(stored));
  const localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v), removeItem: (k: string) => void store.delete(k) };
  const names = moduleFromSource("../src/appearance/agentNames.ts", {}, { localStorage }, "");
  const sectionHead = (title: string, _id?: string, count?: string) => {
    const band = h("div", "set-band");
    band.append(h("span", "set-band-title", title), h("span", "set-band-count", count));
    return band;
  };
  const m = moduleFromSource("../src/appearance/settingsAgents.ts", {
    "../ui/dom": { h },
    "./rowTable": { rowTable: () => h("div"), sectionHead },
    "./agentNames": names,
    "./programPicker": { pickProgram() {} },
  }, { Promise }, "\nexports.callSigns = callSigns;");
  const box: El = m.callSigns();
  const [titles, nouns] = box.all().filter((e) => e.tag === "textarea");
  const count = (area: El) => box.children.find((c) => c.children.includes(area))!.querySelector(".set-band-count")!.textContent;
  return { store, names, titles, nouns, count };
}

test("each list shows as one word per line, with its count", () => {
  const p = page({ "skiff.agentNames.ranks": JSON.stringify(["Captain", "First Mate"]) });
  assert.equal(p.titles.value, "Captain\nFirst Mate");
  assert.equal(p.count(p.titles), "2");
  assert.equal(p.nouns.value.split("\n").length, 24);
  assert.equal(p.titles.attrs["aria-label"], "Agent titles");
  assert.equal(p.nouns.attrs["aria-label"], "Agent names");
});

test("leaving a text area saves its lines, trimmed and without blanks, as the stored list", () => {
  const p = page();
  p.titles.value = "Captain\n  Sea Dog  \n\nAdmiral\n";
  p.titles.blur();
  assert.equal(p.store.get("skiff.agentNames.ranks"), JSON.stringify(["Captain", "Sea Dog", "Admiral"]));
  assert.equal(p.titles.value, "Captain\nSea Dog\nAdmiral");
  assert.equal(p.count(p.titles), "3");
  assert.deepEqual([...p.names.agentNameList("ranks")], ["Captain", "Sea Dog", "Admiral"]);
  // The other list is not written.
  assert.equal(p.store.has("skiff.agentNames.nouns"), false);
});

test("an empty text area goes back to the default words", () => {
  const p = page({ "skiff.agentNames.nouns": JSON.stringify(["Kraken"]) });
  p.nouns.value = " \n";
  p.nouns.blur();
  assert.equal(p.store.has("skiff.agentNames.nouns"), false);
  assert.equal(p.nouns.value.split("\n").length, 24);
  assert.equal(p.count(p.nouns), "24");
});


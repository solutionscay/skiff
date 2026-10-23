import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, moduleFromSource } from "./workspaceHarness.ts";

/** A stand-in element: enough for the Hosts tab to build and be clicked. */
class El {
  children: El[] = [];
  attrs: Record<string, string> = {};
  listeners: Record<string, ((e: unknown) => void)[]> = {};
  classList = { toggle() {}, add() {} };
  value = "";
  placeholder = "";
  disabled = false;
  id = "";
  tag: string;
  className: string;
  textContent: string;
  constructor(tag: string, className = "", textContent = "") {
    this.tag = tag;
    this.className = className;
    this.textContent = textContent;
  }
  append(...c: El[]) { this.children.push(...c.filter((x) => x instanceof El)); }
  setAttribute(k: string, v: string) { this.attrs[k] = v; }
  getAttribute(k: string) { return this.attrs[k] ?? null; }
  addEventListener(k: string, f: (e: unknown) => void) { (this.listeners[k] ??= []).push(f); }
  focus() {}
  all(): El[] { return [this, ...this.children.flatMap((c) => c.all())]; }
  find(text: string) { return this.all().find((e) => e.textContent === text); }
  click() { for (const f of this.listeners.click ?? []) f({}); }
}

function tab() {
  const calls: [string, unknown][] = [];
  let prompt: { value?: string; submit: (v: string) => Promise<void> } | null = null;
  const hosts = [{ id: "h_dev", name: "devbox", destination: "devbox", user: "me", port: 2222, socket: "/run/user/1000/skiff/skiffd.sock" }];
  const invoke = async (c: string, a?: unknown) => {
    calls.push([c, a]);
    if (c === "resolve_ssh") return { hostname: "devbox.lan", user: "me", port: 2222, jump: "" };
    if (c === "ssh_aliases") return [{ name: "devbox", goes_to: "me@devbox.lan" }];
  };
  const h = (tag: string, cls?: string, text?: string) => new El(tag, cls, text);
  const mod = moduleFromSource(
    "../src/appearance/hostSettings.ts",
    {
      "@tauri-apps/api/core": { invoke },
      "../ui/dom": { h, button: (cls: string, text: string, run: () => void) => { const b = h("button", cls, text); b.addEventListener("click", run); return b; } },
      "../ui/runes": { rune: () => new El("svg") },
      "../ui/confirm": { confirmAction: async () => true, promptAction: (o: typeof prompt) => void (prompt = o) },
      "../app/hosts": { knownHosts: () => hosts, loadHosts: async () => {}, localHostName: () => "This computer", shownHost: () => null },
    },
    { document: { getElementById: () => null, querySelector: () => null }, setTimeout, clearTimeout, Promise, Map },
    "",
  );
  let page: El = new El("div");
  const t = mod.createHostsTab(() => (page = t.draw()));
  return { t, calls, page: () => page, prompt: () => prompt!, redraw: () => (page = t.draw()) };
}

test("Rename sends only the new name, for the Host's id", async () => {
  const x = tab();
  await x.t.load();
  x.redraw();
  assert.equal(x.page().find("me@devbox.lan:2222")?.className, "c-server mono", "the Server column shows what ssh -G resolves");
  x.page().find("Rename…")!.click();
  assert.equal(x.prompt().value, "devbox");
  await x.prompt().submit("Dev box");
  assert.equal(JSON.stringify(x.calls.find(([c]) => c === "rename_host")), JSON.stringify(["rename_host", { id: "h_dev", name: "Dev box" }]));
});

test("Edit opens the form with the Host's values and saves them under its id", async () => {
  const x = tab();
  await x.t.load();
  x.redraw();
  x.page().find("Edit…")!.click();
  assert.ok(x.page().find("Edit devbox"));
  // User and port were set, so More is open and shows them.
  const port = x.page().all().find((e) => e.attrs["aria-label"] === "Port")!;
  assert.equal(port.value, "2222");
  port.value = "2200";
  for (const f of port.listeners.input) f({});
  x.page().find("Save")!.click();
  await flush();
  await flush();
  const edit = x.calls.find(([c]) => c === "edit_host")!;
  assert.equal(JSON.stringify(edit), JSON.stringify(["edit_host", { id: "h_dev", form: { destination: "devbox", name: "devbox", user: "me", port: "2200", socket: "/run/user/1000/skiff/skiffd.sock" } }]));
  assert.ok(x.page().find("Hosts"), "back on the list after a save");
});

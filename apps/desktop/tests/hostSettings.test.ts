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
  const hosts = [{ id: "h_dev", name: "devbox", destination: "devbox", user: "me", port: 2222, socket: "/run/user/1000/skiff/skiffd.sock", socket_override: false, state: "attention" }];
  const invoke = async (c: string, a?: unknown) => {
    calls.push([c, a]);
    if (c === "resolve_ssh") return { hostname: "devbox.lan", user: "me", port: 2222, jump: "" };
    if (c === "ssh_aliases") return [{ name: "devbox", goes_to: "me@devbox.lan" }];
    if (c === "check_host") return { ok: true, rows: [{ row: "reach", title: "Reach the server", state: "passed", text: "devbox.lan answers. 12 ms." }] };
  };
  const h = (tag: string, cls?: string, text?: string) => new El(tag, cls, text);
  const mod = moduleFromSource(
    "../src/appearance/hostSettings.ts",
    {
      "@tauri-apps/api/core": { invoke },
      "../ui/dom": { h, button: (cls: string, text: string, run: () => void) => { const b = h("button", cls, text); b.addEventListener("click", run); return b; } },
      "../ui/runes": { rune: () => new El("svg") },
      "../ui/confirm": { confirmAction: async () => true, promptAction: (o: typeof prompt) => void (prompt = o) },
      "../app/hosts": { knownHosts: () => hosts, loadHosts: async () => {}, localHostName: () => "This computer", shownHost: () => null, pick: (id: string) => void calls.push(["pick", id]) },
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
  x.redraw();
  // The socket was found, not set: its field is empty and says what was found.
  const socket = x.page().all().find((e) => e.attrs["aria-label"] === "Socket on the server")!;
  assert.equal(socket.value, "");
  assert.equal(socket.placeholder, "Found: /run/user/1000/skiff/skiffd.sock");
  assert.equal(x.page().find("Save"), undefined, "a changed port needs Connect");
  x.page().find("Connect")!.click();
  for (let i = 0; i < 6; i++) await flush();
  const edit = x.calls.find(([c]) => c === "edit_host")!;
  assert.equal(JSON.stringify(edit), JSON.stringify(["edit_host", { id: "h_dev", form: { destination: "devbox", name: "devbox", user: "me", port: "2200", socket: "" } }]));
  // Connect checks the Host at once, and a passed check offers the switch.
  assert.ok(x.calls.some(([c, a]) => c === "check_host" && JSON.stringify(a) === '{"id":"h_dev"}'));
  assert.ok(x.page().find("devbox.lan answers. 12 ms."));
  x.page().find("Switch to devbox")!.click();
  assert.ok(x.calls.some(([c, a]) => c === "pick" && a === "h_dev"));
});

test("a Host that needs attention opens its check list from the menu link", async () => {
  const x = tab();
  await x.t.load();
  x.t.open({ check: "h_dev" });
  x.redraw();
  assert.ok(x.page().find("Check devbox"));
  assert.ok(x.page().find("Checking…"), "the first row runs while the rest wait");
  for (let i = 0; i < 4; i++) await flush();
  x.redraw();
  assert.ok(x.page().find("Reach the server"));
  assert.equal(x.page().find("Needs attention"), undefined);
});

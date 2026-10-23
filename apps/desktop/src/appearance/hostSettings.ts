import { invoke } from "@tauri-apps/api/core";
import { h, button } from "../ui/dom";
import { rune } from "../ui/runes";
import { confirmAction, promptAction } from "../ui/confirm";
import { type HostsPage, type KnownHost, knownHosts, loadHosts, localHostName, shownHost } from "../app/hosts";

/**
 * Settings › Hosts: the known Hosts, and the form that adds or edits one.
 * The list is this computer's own (`hosts.toml`), so the tab works the same
 * while any Host is shown. The form needs one field, Server. Name, user and
 * port sit under More: ssh config gives them, and the line under Server
 * says what `ssh -G` makes of the three, with no network.
 */

interface Resolved {
  hostname: string;
  user: string;
  port: number;
  jump: string;
}

interface Alias {
  name: string;
  goes_to: string;
}

type Draft = { destination: string; name: string; user: string; port: string; socket: string };

/** Until the probe finds it (a later step), the socket is typed. Linux's first account is the default. */
const DEFAULT_SOCKET = "/run/user/1000/skiff/skiffd.sock";

type Form = { id: string | null; draft: Draft; more: boolean; error: string | null; busy: boolean };

export function createHostsTab(redraw: () => void) {
  let form: Form | null = null;
  let error: string | null = null;
  let aliases: Alias[] = [];
  /** `ssh -G` per Host id, for the Server column. */
  const resolved = new Map<string, Resolved>();
  /** The form's resolve line, and the input it answers. */
  let line: { for: string; text: string; bad: boolean } = { for: "", text: "", bad: false };
  let timer: ReturnType<typeof setTimeout> | undefined;

  const server = (r: Resolved) => `${r.user ? `${r.user}@` : ""}${r.hostname}${r.port !== 22 ? `:${r.port}` : ""}`;
  const typed = (x: KnownHost) => `${x.user ? `${x.user}@` : ""}${x.destination}${x.port ? `:${x.port}` : ""}`;
  const resolve = (destination: string, user: string, port: string) =>
    invoke<Resolved>("resolve_ssh", { destination, user: user ?? "", port: port ?? "" });

  /** Reads ssh config and resolves each Host. Local only. */
  async function load() {
    aliases = await invoke<Alias[]>("ssh_aliases").catch(() => []);
    await Promise.all(
      knownHosts().map(async (x) => {
        const r = await resolve(x.destination, x.user ?? "", x.port ? String(x.port) : "").catch(() => null);
        if (r) resolved.set(x.id, r);
        else resolved.delete(x.id);
      }),
    );
  }

  function open(page: HostsPage) {
    error = null;
    form = page.add ? newForm(null) : null;
  }

  function newForm(x: KnownHost | null): Form {
    const draft: Draft = x
      ? { destination: x.destination, name: x.name, user: x.user ?? "", port: x.port ? String(x.port) : "", socket: x.socket }
      : { destination: "", name: "", user: "", port: "", socket: DEFAULT_SOCKET };
    line = { for: "", text: "", bad: false };
    return { id: x?.id ?? null, draft, more: !!x && (!!x.user || !!x.port || x.name !== defaultName(x.destination)), error: null, busy: false };
  }

  const defaultName = (d: string) => d.split("@").pop() ?? d;

  function draw(): HTMLElement {
    return form ? drawForm(form) : drawList();
  }

  function drawList(): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head set-head-row");
    const words = h("div", "set-head-words");
    const title = h("div", "set-h", "Hosts");
    title.id = "set-h-hosts";
    words.append(title, h("div", "set-sub", "Computers whose sessions Skiff can show. Pick one from the Host menu in the top bar."));
    const add = button("set-add ready", "Add SSH Host…", () => {
      form = newForm(null);
      redraw();
      focusServer();
    });
    head.append(words, add);
    main.append(head);
    const table = h("div", "set-table g-hosts");
    table.setAttribute("role", "table");
    table.setAttribute("aria-labelledby", "set-h-hosts");
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "c-lead"), h("span", "", "NAME"), h("span", "", "SERVER"), h("span", ""));
    table.append(cols, hostRow(null));
    for (const x of knownHosts()) table.append(hostRow(x));
    main.append(table);
    if (!knownHosts().length) main.append(h("div", "set-note", "Add a server to see and run its sessions here. Pick it from your SSH config, or type its name or address."));
    if (error) main.append(alert(error));
    return main;
  }

  /** One Host. `null`: this computer, which has nothing to set. */
  function hostRow(x: KnownHost | null): HTMLElement {
    const row = h("div", "set-row set-host");
    row.setAttribute("role", "row");
    const shown = x ? shownHost() === x.id : shownHost() === null;
    const lead = h("span", "c-lead");
    if (shown) lead.append(rune("indicators-check"));
    lead.setAttribute("aria-label", shown ? "Shown in this window" : "");
    const r = x && resolved.get(x.id);
    const acts = h("span", "c-end");
    if (x) {
      acts.append(
        button("set-rm", "Edit…", () => {
          form = newForm(x);
          redraw();
          focusServer();
        }),
        button("set-rm", "Rename…", () => rename(x)),
        button("set-rm c-remove", "Remove…", () => void remove(x)),
      );
    }
    row.append(
      lead,
      h("span", "c-name", x ? x.name : localHostName()),
      h("span", "c-server mono", x ? (r ? server(r) : typed(x)) : "On this computer"),
      acts,
    );
    return row;
  }

  function rename(x: KnownHost) {
    promptAction({
      title: `Rename ${x.name}`,
      body: "The name shows in the Host menu, in Settings and in the window title. The server stays the same.",
      placeholder: "a name",
      value: x.name,
      action: "Rename",
      submit: async (name) => {
        await invoke("rename_host", { id: x.id, name });
        await loadHosts();
        redraw();
      },
    });
  }

  async function remove(x: KnownHost) {
    const ok = await confirmAction({
      title: `Remove ${x.name}?`,
      body: `Skiff removes it from this computer's Host list. Its sessions keep running on ${x.name}.`,
      action: "Remove Host",
    });
    if (!ok) return;
    error = null;
    await invoke("remove_host", { id: x.id }).catch((e) => (error = String(e)));
    await loadHosts().catch(() => {});
    redraw();
  }

  function drawForm(f: Form): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head");
    const back = button("set-back", "‹ Hosts", () => {
      form = null;
      redraw();
    });
    const editing = f.id ? knownHosts().find((x) => x.id === f.id) : null;
    head.append(
      back,
      h("div", "set-h", editing ? `Edit ${editing.name}` : "Add SSH Host"),
      h("div", "set-sub", "Pick a server from your SSH config, or type its name or address."),
    );
    main.append(head);

    const list = h("datalist");
    list.id = "set-ssh-aliases";
    for (const a of aliases) {
      const o = h("option");
      o.value = a.name;
      if (a.goes_to) o.label = a.goes_to;
      list.append(o);
    }
    const dest = field(f, "destination", "Server", "devbox, devbox.lan or 10.0.0.5");
    dest.setAttribute("list", list.id);
    dest.addEventListener("input", () => lookup(f));
    main.append(fieldRow("Server", dest, list));
    const said = h("div", "set-note set-resolve" + (line.bad ? " bad" : ""), line.text || "Skiff reads the user and port from your SSH config.");
    said.id = "set-resolve";
    main.append(said);

    const toggle = button("set-rm", f.more ? "Less" : "More", () => {
      f.more = !f.more;
      redraw();
    });
    toggle.setAttribute("aria-expanded", String(f.more));
    if (f.more) {
      const r = line.for === key(f) ? lastResolved : null;
      const name = field(f, "name", "Name", defaultName(f.draft.destination.trim()) || "the server's name");
      const user = field(f, "user", "User", r?.user || "from SSH config");
      const port = field(f, "port", "Port", r ? String(r.port) : "from SSH config");
      port.inputMode = "numeric";
      user.addEventListener("input", () => lookup(f));
      port.addEventListener("input", () => lookup(f));
      main.append(fieldRow("Name", name), fieldRow("User", user), fieldRow("Port", port));
    }
    const socket = field(f, "socket", "Socket on the server", DEFAULT_SOCKET);
    main.append(fieldRow("Socket on the server", socket));

    const acts = h("div", "set-form-acts");
    const save = button("set-add ready", f.busy ? "…" : "Save", () => void submit(f));
    save.disabled = f.busy;
    acts.append(toggle, h("span", "set-fill"), button("set-rm", "Cancel", () => ((form = null), redraw())), save);
    main.append(acts);
    if (f.error) main.append(alert(f.error));
    if (!line.for && f.draft.destination.trim()) lookup(f);
    return main;
  }

  let lastResolved: Resolved | null = null;
  const key = (f: Form) => `${f.draft.destination.trim()}\n${f.draft.user.trim()}\n${f.draft.port.trim()}`;

  /** The resolve line follows the fields, 250 ms after the last key. */
  function lookup(f: Form) {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const k = key(f);
      if (!f.draft.destination.trim()) {
        line = { for: "", text: "", bad: false };
        return paint();
      }
      try {
        const r = await resolve(f.draft.destination.trim(), f.draft.user.trim(), f.draft.port.trim());
        if (k !== key(f)) return;
        lastResolved = r;
        const jump = r.jump ? `, through ${r.jump}` : "";
        line = { for: k, text: `Skiff connects as ${r.user} to ${r.hostname}, port ${r.port}${jump}.`, bad: false };
      } catch (e) {
        if (k !== key(f)) return;
        line = { for: k, text: String(e), bad: true };
      }
      paint();
    }, 250);
  }

  /** The line alone: a redraw would move the caret. */
  function paint() {
    const el = document.getElementById("set-resolve");
    if (!el) return;
    el.textContent = line.text || "Skiff reads the user and port from your SSH config.";
    el.classList.toggle("bad", line.bad);
  }

  async function submit(f: Form) {
    if (f.busy) return;
    f.error = null;
    f.busy = true;
    redraw();
    try {
      if (f.id) await invoke("edit_host", { id: f.id, form: f.draft });
      else await invoke<string>("add_host", { form: f.draft });
      await loadHosts();
      await load();
      form = null;
    } catch (e) {
      f.error = String(e);
    } finally {
      f.busy = false;
      redraw();
    }
  }

  function field(f: Form, k: keyof Draft, label: string, placeholder: string): HTMLInputElement {
    const input = h("input", "c-cmd mono");
    input.type = "text";
    input.spellcheck = false;
    input.autocomplete = "off";
    input.value = f.draft[k];
    input.placeholder = placeholder;
    input.setAttribute("aria-label", label);
    input.addEventListener("input", () => (f.draft[k] = input.value));
    input.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      e.preventDefault();
      void submit(f);
    });
    return input;
  }

  function fieldRow(label: string, ...content: HTMLElement[]): HTMLElement {
    const row = h("div", "set-row set-open set-host-field");
    row.append(h("span", "c-name", label), ...content);
    return row;
  }

  function focusServer() {
    document.querySelector<HTMLInputElement>('#settings input[aria-label="Server"]')?.focus();
  }

  function alert(text: string): HTMLElement {
    const err = h("div", "set-error", text);
    err.setAttribute("role", "alert");
    return err;
  }

  return { load, open, draw, focusServer, get editing() { return form !== null; } };
}

import { invoke } from "@tauri-apps/api/core";
import { h, button } from "../ui/dom";
import { rune } from "../ui/runes";
import { confirmAction, promptAction } from "../ui/confirm";
import { type HostsPage, type KnownHost, knownHosts, loadHosts, localHostName, pick, shownHost } from "../app/hosts";

/**
 * Settings › Hosts: the known Hosts, the form that adds or edits one, and
 * a Host's check list. The list is this computer's own (`hosts.toml`), so
 * the tab works the same while any Host is shown. The form needs one field,
 * Server. Name, user, port and socket sit under More: ssh config gives the
 * first three, the line under Server says what `ssh -G` makes of them, with
 * no network, and the check finds the socket. Connect saves, then checks.
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

type Form = { id: string | null; draft: Draft; more: boolean; error: string | null; busy: boolean };

/** One row of the check list, as Rust's `check_host` answers it. */
interface CheckLine {
  row: string;
  title: string;
  state: "passed" | "failed" | "skipped";
  text: string;
}
interface Report {
  rows: CheckLine[];
  ok: boolean;
}
/** The rows, in order, before an answer. */
const CHECK_ROWS = ["Reach the server", "Host key", "Log in without a prompt", "Sessions service (skiffd)", "Socket forwarding", "Skiff identity and version"];

type Check = { id: string; report: Report | null; error: string | null };

export function createHostsTab(redraw: () => void) {
  let form: Form | null = null;
  let check: Check | null = null;
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
    form = "add" in page ? newForm(null) : null;
    check = null;
    if ("check" in page) void runCheck(page.check);
  }

  /** Runs the read-only check list of a Host and shows it. */
  async function runCheck(id: string) {
    form = null;
    const c: Check = { id, report: null, error: null };
    check = c;
    redraw();
    try {
      c.report = await invoke<Report>("check_host", { id });
      await loadHosts();
      await load();
    } catch (e) {
      c.error = String(e);
    }
    if (check === c) redraw();
  }

  function newForm(x: KnownHost | null): Form {
    const draft: Draft = x
      ? { destination: x.destination, name: x.name, user: x.user ?? "", port: x.port ? String(x.port) : "", socket: x.socket_override ? x.socket ?? "" : "" }
      : { destination: "", name: "", user: "", port: "", socket: "" };
    line = { for: "", text: "", bad: false };
    const more = !!x && (!!x.user || !!x.port || x.socket_override || x.name !== defaultName(x.destination));
    return { id: x?.id ?? null, draft, more, error: null, busy: false };
  }

  const defaultName = (d: string) => d.split("@").pop() ?? d;

  function draw(): HTMLElement {
    return form ? drawForm(form) : check ? drawCheck(check) : drawList();
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
    cols.append(h("span", "c-lead"), h("span", "", "NAME"), h("span", "", "SERVER"), h("span", "", "STATE"), h("span", ""));
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
        button("set-rm", "Check", () => void runCheck(x.id)),
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
      stateCell(x),
      acts,
    );
    return row;
  }

  /** Dot and word, from the last check or connection in this app run. */
  function stateCell(x: KnownHost | null): HTMLElement {
    const [cls, word] = !x
      ? ["ok", "Connected"]
      : x.state === "ok"
        ? ["ok", "Connected"]
        : x.state === "attention"
          ? ["bad", "Needs attention"]
          : ["none", "Not checked"];
    const cell = h("span", `c-state ${cls}`);
    cell.append(h("span", "sq"), word);
    return cell;
  }

  function drawCheck(c: Check): HTMLElement {
    const x = knownHosts().find((k) => k.id === c.id);
    const name = x?.name ?? "this Host";
    const main = h("div", "set-main");
    const head = h("div", "set-head");
    head.append(
      button("set-back", "‹ Hosts", () => ((check = null), redraw())),
      h("div", "set-h", `Check ${name}`),
      h("div", "set-sub", `Skiff checks each step over ssh, top to bottom. It changes nothing on ${name}.`),
    );
    main.append(head);
    const table = h("div", "set-table g-check");
    table.setAttribute("role", "table");
    table.setAttribute("aria-label", `Check ${name}`);
    const rows: CheckLine[] = c.report?.rows ?? CHECK_ROWS.map((title, i) => ({ row: String(i), title, state: "skipped", text: i === 0 ? "Checking…" : "Waiting" }));
    for (const r of rows) {
      const row = h("div", `set-row set-check ${c.report ? r.state : "waiting"}`);
      row.setAttribute("role", "row");
      const mark = h("span", "c-lead");
      if (c.report && r.state === "passed") mark.append(rune("indicators-check"));
      else mark.append(h("span", "sq"));
      mark.setAttribute("aria-label", c.report ? r.state : "waiting");
      row.append(mark, h("span", "c-name", r.title), h("span", "c-text", r.text));
      table.append(row);
    }
    main.append(table);
    if (c.error) main.append(alert(c.error));
    const acts = h("div", "set-form-acts");
    const running = !c.report && !c.error;
    const again = button("set-rm", running ? "Checking…" : "Check again", () => void runCheck(c.id));
    again.disabled = running;
    acts.append(again, h("span", "set-fill"));
    if (c.report?.ok && x && shownHost() !== x.id) acts.append(button("set-add ready", `Switch to ${name}`, () => pick(x.id)));
    acts.append(button("set-rm", "Done", () => ((check = null), redraw())));
    main.append(acts);
    return main;
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
      // The check finds the socket. A value here replaces what it finds.
      const found = editing?.socket && !editing.socket_override ? `Found: ${editing.socket}` : "Found by Skiff";
      const socket = field(f, "socket", "Socket on the server", found);
      main.append(fieldRow("Name", name), fieldRow("User", user), fieldRow("Port", port), fieldRow("Socket on the server", socket));
    }

    const acts = h("div", "set-form-acts");
    const save = button("set-add ready", f.busy ? "…" : "Connect", () => void submit(f));
    save.disabled = f.busy;
    acts.append(toggle, h("span", "set-fill"), button("set-rm", "Cancel", () => ((form = null), redraw())));
    // Only a new name needs no check.
    if (editing && onlyName(f, editing)) acts.append(button("set-rm", "Save", () => void submit(f, false)));
    acts.append(save);
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

  const onlyName = (f: Form, x: KnownHost) => {
    const d = f.draft;
    return d.destination.trim() === x.destination && d.user.trim() === (x.user ?? "") && d.port.trim() === (x.port ? String(x.port) : "") && d.socket.trim() === (x.socket_override ? x.socket ?? "" : "");
  };

  /** Saves the form. Connect then runs the check list: it finds the socket. */
  async function submit(f: Form, connect = true) {
    if (f.busy) return;
    f.error = null;
    f.busy = true;
    redraw();
    let id = f.id;
    // Save is for a new name only: any other change checks again.
    const before = f.id ? knownHosts().find((k) => k.id === f.id) : undefined;
    connect ||= !before || !onlyName(f, before);
    try {
      if (id) await invoke("edit_host", { id, form: f.draft });
      else id = await invoke<string>("add_host", { form: f.draft });
      await loadHosts();
      await load();
      form = null;
    } catch (e) {
      f.error = String(e);
      id = null;
    } finally {
      f.busy = false;
    }
    if (id && connect) return void runCheck(id);
    redraw();
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

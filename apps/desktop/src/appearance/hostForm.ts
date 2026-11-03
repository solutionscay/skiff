import { h, button } from "../ui/dom";
import { type Draft, type Form, type HostRow, type HostsEnv, type HostsState, type Resolved, alert, defaultName } from "./hostShared";

/** The add/edit form of a Host, its resolve line and its save. The facade
 *  gives it the Host calls through `env`. */
export function createHostForm(env: HostsEnv, st: HostsState, deps: { runCheck(id: string): Promise<void>; reload(): Promise<void> }) {
  function open(x: HostRow | null) {
    const draft: Draft = x
      ? { destination: x.destination, name: x.name, user: x.user ?? "", port: x.port ? String(x.port) : "" }
      : { destination: "", name: "", user: "", port: "" };
    st.line = { for: "", text: "", bad: false };
    const more = !!x && (!!x.user || !!x.port || x.name !== defaultName(x.destination));
    st.form = { id: x?.id ?? null, draft, more, error: null, busy: false };
  }

  const resolve = (destination: string, user: string, port: string) =>
    env.ask<Resolved>("resolve_ssh", { destination, user: user ?? "", port: port ?? "" });

  function drawForm(f: Form): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head");
    const back = button("set-back", "‹ Hosts", () => {
      st.form = null;
      env.redraw();
    });
    const editing = f.id ? env.hosts.knownHosts().find((x) => x.id === f.id) : null;
    head.append(
      back,
      h("div", "set-h", editing ? `Edit ${editing.name}` : "Add SSH Host"),
    );
    main.append(head);

    const list = h("datalist");
    list.id = "set-ssh-aliases";
    for (const a of st.aliases) {
      const o = h("option");
      o.value = a.name;
      if (a.goes_to) o.label = a.goes_to;
      list.append(o);
    }
    const dest = field(f, "destination", "Server", "devbox, devbox.lan or 10.0.0.5");
    dest.setAttribute("list", list.id);
    dest.addEventListener("input", () => lookup(f));
    main.append(fieldRow("Server", dest, list));
    const said = h("div", "set-note set-line set-resolve" + (st.line.bad ? " bad" : ""), st.line.text);
    said.id = "set-resolve";
    main.append(said);

    const toggle = button("set-rm", f.more ? "Less" : "More", () => {
      f.more = !f.more;
      env.redraw();
    });
    toggle.setAttribute("aria-expanded", String(f.more));
    if (f.more) {
      const r = st.line.for === key(f) ? st.lastResolved : null;
      const name = field(f, "name", "Name", defaultName(f.draft.destination.trim()) || "the server's name");
      const user = field(f, "user", "User", r?.user || "from SSH config");
      const port = field(f, "port", "Port", r ? String(r.port) : "from SSH config");
      port.inputMode = "numeric";
      user.addEventListener("input", () => lookup(f));
      port.addEventListener("input", () => lookup(f));
      main.append(fieldRow("Name", name), fieldRow("User", user), fieldRow("Port", port));
    }

    const acts = h("div", "set-form-acts");
    const save = button("set-add ready", f.busy ? "…" : "Connect", () => void submit(f));
    save.disabled = f.busy;
    acts.append(toggle, h("span", "set-fill"), button("set-rm", "Cancel", () => ((st.form = null), env.redraw())));
    // Only a new name needs no check.
    if (editing && onlyName(f, editing)) acts.append(button("set-rm", "Save", () => void submit(f, false)));
    acts.append(save);
    main.append(acts);
    if (f.error) main.append(alert(f.error));
    if (!st.line.for && f.draft.destination.trim()) lookup(f);
    return main;
  }

  const key = (f: Form) => `${f.draft.destination.trim()}\n${f.draft.user.trim()}\n${f.draft.port.trim()}`;

  /** The resolve line follows the fields, 250 ms after the last key. */
  function lookup(f: Form) {
    clearTimeout(st.timer);
    st.timer = setTimeout(async () => {
      const k = key(f);
      if (!f.draft.destination.trim()) {
        st.line = { for: "", text: "", bad: false };
        return paint();
      }
      try {
        const r = await resolve(f.draft.destination.trim(), f.draft.user.trim(), f.draft.port.trim());
        if (k !== key(f)) return;
        st.lastResolved = r;
        const jump = r.jump ? `, through ${r.jump}` : "";
        st.line = { for: k, text: `${r.user}@${r.hostname}, port ${r.port}${jump}.`, bad: false };
      } catch (e) {
        if (k !== key(f)) return;
        st.line = { for: k, text: String(e), bad: true };
      }
      paint();
    }, 250);
  }

  /** The line alone: a redraw would move the caret. */
  function paint() {
    const el = document.getElementById("set-resolve");
    if (!el) return;
    el.textContent = st.line.text || "User and port come from your SSH config.";
    el.classList.toggle("bad", st.line.bad);
  }

  const onlyName = (f: Form, x: HostRow) => {
    const d = f.draft;
    return d.destination.trim() === x.destination && d.user.trim() === (x.user ?? "") && d.port.trim() === (x.port ? String(x.port) : "");
  };

  /** Saves the form. Connect then runs the check list: it finds the socket. */
  async function submit(f: Form, connect = true) {
    if (f.busy) return;
    f.error = null;
    f.busy = true;
    env.redraw();
    let id = f.id;
    // Save is for a new name only: any other change checks again.
    const before = f.id ? env.hosts.knownHosts().find((k) => k.id === f.id) : undefined;
    connect ||= !before || !onlyName(f, before);
    try {
      if (id) await env.ask("edit_host", { id, form: f.draft });
      else id = await env.ask<string>("add_host", { form: f.draft });
      await env.hosts.loadHosts();
      await deps.reload();
      st.form = null;
    } catch (e) {
      f.error = String(e);
      id = null;
    } finally {
      f.busy = false;
    }
    if (id && connect) return void deps.runCheck(id);
    env.redraw();
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

  return { open, draw: drawForm, focusServer };
}

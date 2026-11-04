import { h, button } from "../ui/dom";
import { smallButton, textBox } from "../ui/controls";
import { t } from "../i18n";
import { type Draft, type Form, type HostRow, type HostsEnv, type HostsState, type Resolved, alert, defaultName } from "./hostShared";

/** The add/edit form of a Host, its resolve line and its save. The facade
 *  gives it the Host calls through `env`. */
export function createHostForm(env: HostsEnv, st: HostsState, deps: { runCheck(id: string): Promise<void>; reload(): Promise<void> }) {
  function open(x: HostRow | null) {
    const draft: Draft = x
      ? { destination: x.destination, name: x.name, user: x.user ?? "", port: x.port ? String(x.port) : "" }
      : { destination: "", name: "", user: "", port: "" };
    st.line = { for: "", text: "", bad: false };
    st.form = { id: x?.id ?? null, draft, typing: false, error: null, busy: false };
  }

  const resolve = (destination: string, user: string, port: string) =>
    env.ask<Resolved>("resolve_ssh", { destination, user: user ?? "", port: port ?? "" });

  /** The head of the form and of the list before it: the way back, then the title. */
  function formHead(title: string): HTMLElement {
    const head = h("div", "set-head");
    head.append(button("set-back", t("hosts.back"), () => {
      st.form = null;
      env.redraw();
    }), h("div", "set-h", title));
    return head;
  }

  /** Add SSH Host: the hosts in SSH config, one Connect for each. Other server… opens the fields. */
  function drawPick(f: Form): HTMLElement {
    const main = h("div", "set-main");
    main.append(formHead(t("hosts.addTitle")));
    const table = h("div", "set-table g-pick");
    table.setAttribute("role", "table");
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "", t("hosts.colName")), h("span", "", t("hosts.colGoesTo")), h("span"));
    table.append(cols);
    const known = new Set(env.hosts.knownHosts().map((x) => x.destination));
    for (const a of st.aliases) {
      const row = h("div", "set-row set-agent on");
      const acts = h("span", "c-acts");
      if (known.has(a.name)) acts.append(h("span", "c-dim", t("hosts.added")));
      else {
        const go = smallButton(t("hosts.connect"), () => {
          f.draft.destination = a.name;
          f.draft.name = a.name;
          void submit(f);
        }, { label: t("hosts.connectTo", { name: a.name }) });
        go.disabled = f.busy;
        acts.append(go);
      }
      row.append(h("span", "c-name", a.name), h("span", "c-server mono", a.goes_to), acts);
      table.append(row);
    }
    const other = h("div", "set-row set-other");
    other.append(button("set-other-b", t("hosts.other"), () => {
      f.typing = true;
      env.redraw();
      focusServer();
    }));
    table.append(other);
    main.append(table);
    if (f.error) main.append(alert(f.error));
    return main;
  }

  function drawForm(f: Form): HTMLElement {
    // SSH config holds no host: the fields are all there is to show.
    if (!f.id && !f.typing && st.aliases.length) return drawPick(f);
    const main = h("div", "set-main");
    const editing = f.id ? env.hosts.knownHosts().find((x) => x.id === f.id) : null;
    main.append(formHead(editing ? t("hosts.editTitle", { name: editing.name }) : t("hosts.addTitle")));

    const dest = field(f, "destination", t("hosts.server"), t("hosts.serverPlaceholder"));
    dest.addEventListener("input", () => lookup(f));
    main.append(fieldRow(t("hosts.server"), dest));
    // What `ssh -G` makes of the fields. Nothing to say: no line.
    const said = h("div", "set-note set-line set-resolve" + (st.line.bad ? " bad" : ""), st.line.text);
    said.id = "set-resolve";
    said.hidden = !st.line.text;
    main.append(said);

    const r = st.line.for === key(f) ? st.lastResolved : null;
    const name = field(f, "name", t("hosts.fieldName"), defaultName(f.draft.destination.trim()) || t("hosts.namePlaceholderForm"));
    const user = field(f, "user", t("hosts.fieldUser"), r?.user || t("hosts.fromConfig"));
    const port = field(f, "port", t("hosts.fieldPort"), r ? String(r.port) : t("hosts.fromConfig"));
    port.inputMode = "numeric";
    user.addEventListener("input", () => lookup(f));
    port.addEventListener("input", () => lookup(f));
    main.append(fieldRow(t("hosts.fieldName"), name), fieldRow(t("hosts.fieldUser"), user), fieldRow(t("hosts.fieldPort"), port));

    const acts = h("div", "set-form-acts");
    const save = button("set-form-go", f.busy ? "…" : t("hosts.connect"), () => void submit(f));
    save.disabled = f.busy;
    acts.append(h("span", "set-fill"), button("set-form-act", t("dialog.cancel"), () => ((st.form = null), env.redraw())));
    // Only a new name needs no check.
    if (editing && onlyName(f, editing)) acts.append(button("set-form-act", t("hosts.save"), () => void submit(f, false)));
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
        const jump = r.jump ? t("hosts.through", { jump: r.jump }) : "";
        st.line = { for: k, text: t("hosts.resolved", { user: r.user, host: r.hostname, port: r.port, jump }), bad: false };
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
    el.textContent = st.line.text;
    el.hidden = !st.line.text;
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
    return textBox({ label, value: f.draft[k], placeholder, mono: true, input: (v) => (f.draft[k] = v), enter: () => void submit(f) });
  }

  function fieldRow(label: string, ...content: HTMLElement[]): HTMLElement {
    const row = h("div", "set-row set-open set-host-field");
    const cell = h("span", "c-field");
    cell.append(...content);
    row.append(h("span", "c-name", label), cell);
    return row;
  }

  function focusServer() {
    document.querySelector<HTMLElement>('#settings .set-host-field input, #settings .g-pick .ctl-btn')?.focus();
  }

  return { open, draw: drawForm, focusServer };
}

import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { h, button } from "../ui/dom";
import { rune } from "../ui/runes";
import { ctxMenu } from "../ui/contextMenu";
import type { MenuEntry } from "../ui/menu";
import { promptAction } from "../ui/confirm";
import { actionDialog } from "../ui/dialogParts";
import { runDialog } from "./hostProgress";
import { copyText } from "../platform/clipboard";
import { createHostFixes } from "./hostFixes";
import { createHostForm } from "./hostForm";
import {
  type Alias, type Check, type CheckLine, type Fix, type HostsEnv, type HostsState,
  type Report, type RowResult, CHECK_ROWS, FIX_LABEL, alert, typed,
} from "./hostShared";
import { type HostsPage, type KnownHost, knownHosts, loadHosts, localHostName, pick, shownHost } from "../app/hosts";

/**
 * Settings › Hosts: the known Hosts, the form that adds or edits one, and
 * a Host's check list. The list is this computer's own (`hosts.toml`), so
 * the tab works the same while any Host is shown. The form needs one field,
 * Server. Name, user and port sit under More: ssh config gives them, the
 * line under Server says what `ssh -G` makes of them, with no network, and
 * the check finds the socket: each server has one skiffd, on
 * `<runtime dir>/skiff/skiffd.sock`. Connect saves, then checks.
 * A failed row has at most one fix. Each fix asks first, names what it
 * changes, and then checks again. A password or passphrase goes to Rust,
 * which gives it to ssh through askpass only; the page keeps none.
 */

export function createHostsTab(redraw: () => void) {
  const st: HostsState = {
    form: null,
    check: null,
    error: null,
    aliases: [],
    version: "",
    line: { for: "", text: "", bad: false },
    timer: undefined,
    lastResolved: null,
  };
  const env: HostsEnv = {
    ask: invoke,
    redraw,
    hosts: { knownHosts, loadHosts, localHostName, shownHost, pick },
    copyText,
    runDialog,
    openMenu: (left, top, title, entries, onClose) => ctxMenu.open(left, top, title, entries, onClose),
  };
  const form = createHostForm(env, st, { runCheck, reload: load });
  const fixes = createHostFixes(env, st, { runCheck });

  /** Reads ssh config and resolves each Host. Local only. */
  async function load() {
    st.version = await getVersion().catch(() => "");
    st.aliases = await invoke<Alias[]>("ssh_aliases").catch(() => []);
  }

  function open(page: HostsPage) {
    st.error = null;
    st.form = null;
    if ("add" in page) form.open(null);
    st.check = null;
    if ("check" in page) void (page.reinstall || page.update ? checkThenUpdate(page.check, !!page.reinstall) : runCheck(page.check));
  }

  /** Runs the read-only check list of a Host and shows it. */
  /** `result`: what a run just did, kept on its row through this check. */
  async function runCheck(id: string, result?: RowResult) {
    st.form = null;
    const c: Check = { id, report: null, error: null, result };
    st.check = c;
    redraw();
    try {
      c.report = await invoke<Report>("check_host", { id });
      await loadHosts();
      await load();
    } catch (e) {
      c.error = String(e);
    }
    if (st.check === c) redraw();
  }


  function draw(): HTMLElement {
    // The Host it was about left the list: back to the list.
    const known = (id: string) => knownHosts().some((x) => x.id === id);
    if (st.check && !known(st.check.id)) st.check = null;
    if (st.form?.id && !known(st.form.id)) st.form = null;
    return st.form ? form.draw(st.form) : st.check ? drawCheck(st.check) : drawList();
  }

  function drawList(): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head set-head-row");
    const words = h("div", "set-head-words");
    const title = h("div", "set-h", "Hosts");
    title.id = "set-h-hosts";
    words.append(title);
    const add = button("set-add ready", "Add SSH Host…", () => {
      form.open(null);
      redraw();
      form.focusServer();
    });
    head.append(words, add);
    main.append(head);
    const table = h("div", "set-table g-hosts");
    table.setAttribute("role", "table");
    table.setAttribute("aria-labelledby", "set-h-hosts");
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "c-lead"), h("span", "", "NAME"), h("span", "", "SERVER"), h("span", "", "SKIFF"), h("span", "", "STATE"), h("span", ""));
    table.append(cols, hostRow(null));
    for (const x of knownHosts()) table.append(hostRow(x));
    main.append(table);
    if (!knownHosts().length) main.append(h("div", "set-note set-line", "No servers yet."));
    if (st.error) main.append(alert(st.error));
    return main;
  }

  /** One Host. `null`: this computer, which has nothing to set: no menu,
   *  no click, no hover. A Host row opens its check list on a click or
   *  Enter; its other actions are in the row menu (⋯, right-click, the
   *  menu key). */
  function hostRow(x: KnownHost | null): HTMLElement {
    const row = h("div", x ? "set-row set-host act" : "set-row set-host");
    row.setAttribute("role", "row");
    const shown = x ? shownHost() === x.id : shownHost() === null;
    const lead = h("span", "c-lead");
    if (shown) lead.append(rune("indicators-check"));
    lead.setAttribute("aria-label", shown ? "Shown in this window" : "");
    const skiff = h("span", "c-skiff mono" + (x?.update ? " update" : ""), x ? (x.skiff ?? "—") : st.version || "—");
    const more = h("span", "c-more");
    row.append(lead, h("span", "c-name", x ? x.name : localHostName()), h("span", "c-server mono", x ? typed(x) : localHostName()), skiff, stateCell(x), more);
    if (!x) return row;
    row.tabIndex = 0;
    row.setAttribute("aria-label", `${x.name}: open its check list`);
    const menu = button("c-menu", "⋯", () => {});
    menu.tabIndex = -1;
    menu.title = "More";
    menu.setAttribute("aria-label", `More for ${x.name}`);
    const openMenu = (at: { x: number; y: number }) => hostMenu(x, at.x, at.y, row);
    menu.addEventListener("click", (e: MouseEvent) => {
      e.stopPropagation();
      const b = menu.getBoundingClientRect();
      openMenu({ x: b.left, y: b.bottom });
    });
    more.append(menu);
    row.addEventListener("click", () => void runCheck(x.id));
    row.addEventListener("contextmenu", (e: MouseEvent) => {
      e.preventDefault();
      openMenu({ x: e.clientX, y: e.clientY });
    });
    row.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter") void runCheck(x.id);
      else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
        const b = menu.getBoundingClientRect();
        openMenu({ x: b.left, y: b.bottom });
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const rows = [...(row.parentElement?.querySelectorAll<HTMLElement>(".set-host.act") ?? [])];
        rows[rows.indexOf(row) + (e.key === "ArrowDown" ? 1 : -1)]?.focus();
      } else return;
      e.preventDefault();
    });
    return row;
  }

  /** The row menu: Check, Update Skiff… when offered, Edit…, Rename…, Remove…. */
  function hostMenu(x: KnownHost, left: number, top: number, back: HTMLElement) {
    const entries: MenuEntry[] = [
      { icon: "indicators-check", label: "Check", run: () => void runCheck(x.id) },
      ...(x.update ? [{ icon: "schedule-refresh-cw", label: "Update Skiff…", run: () => void checkThenUpdate(x.id) }] : []),
      {
        icon: "tools-pencil",
        label: "Edit…",
        run: () => {
          form.open(x);
          redraw();
          form.focusServer();
        },
      },
      { label: "Rename…", run: () => rename(x) },
      { sep: true },
      { icon: "indicators-x", label: "Remove…", danger: true, run: () => void remove(x) },
    ];
    ctxMenu.open(left, top, x.name, entries, () => back.focus());
  }

  /** Update Skiff… from the list: the check list first, for the version
   *  and platform the dialog names, then the dialog. `reinstall`: this
   *  app's build goes in even when the server has it. */
  async function checkThenUpdate(id: string, reinstall = false) {
    await runCheck(id);
    const x = knownHosts().find((k) => k.id === id);
    if (x && st.check?.id === id && st.check.report) fixes.updateSkiff(st.check, x, reinstall);
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
      button("set-back", "‹ Hosts", () => ((st.check = null), redraw())),
      h("div", "set-h", `Check ${name}`),
    );
    main.append(head);
    const table = h("div", "set-table g-check");
    table.setAttribute("role", "table");
    table.setAttribute("aria-label", `Check ${name}`);
    const rows: CheckLine[] = c.report?.rows ?? CHECK_ROWS.map((title, i) => ({ row: String(i), title, state: "skipped", text: i === 0 ? "Checking…" : "Waiting", fix: null }));
    for (const r of rows) {
      // The row a run just changed shows what the run did, until the next check.
      const res = c.result && r.title === CHECK_ROWS[4] ? c.result : null;
      const state = res ? (res.failed ? "failed" : "passed") : c.report ? r.state : "waiting";
      const row = h("div", `set-row set-check ${state}`);
      row.setAttribute("role", "row");
      const mark = h("span", "c-lead");
      if (state === "passed") mark.append(rune("indicators-check"));
      else mark.append(h("span", "sq"));
      mark.setAttribute("aria-label", state);
      const text = h("span", "c-text", res ? res.text : r.text);
      if (res?.secs !== undefined && !res.failed) text.append(h("span", "c-secs", `${res.secs} s`));
      const fix = res ? res.fix : c.report && (r.state === "failed" || r.state === "warning") ? r.fix : null;
      if (fix && x) text.append(button("set-rm set-fix", res?.failed ? "Try again" : FIX_LABEL[fix], () => void runFix(c, x, fix)));
      if (res?.output) {
        const out = res.output;
        text.append(button("set-rm set-fix", "Copy output", () => void copyText(out)));
      }
      row.append(mark, h("span", "c-name", r.title), text);
      table.append(row);
    }
    main.append(table);
    if (x) main.append(autoUpdate(x));
    if (c.error) main.append(alert(c.error));
    if (c.note) main.append(h("div", "set-note set-line", c.note));
    const acts = h("div", "set-form-acts");
    const running = !c.report && !c.error;
    const again = button("set-rm", running ? "Checking…" : "Check again", () => void runCheck(c.id));
    again.disabled = running;
    acts.append(again, h("span", "set-fill"));
    if (c.report?.ok && x && shownHost() !== x.id) acts.append(button("set-add ready", `Switch to ${name}`, () => pick(x.id)));
    acts.append(button("set-rm", "Done", () => ((st.check = null), redraw())));
    main.append(acts);
    return main;
  }

  /** One fix, after its confirm, then the check again. */
  async function runFix(c: Check, x: KnownHost, fix: Fix) {
    const again = () => void runCheck(x.id);
    c.note = undefined;
    try {
      if (fix === "trust_key" || fix === "review_key") return await fixes.trustKey(x, again);
      if (fix === "make_key") return fixes.makeKey(x, again);
      if (fix === "unlock_key") return fixes.secretDialog(x, "unlock", again);
      if (fix === "install_key") return fixes.secretDialog(x, "install", again);
      if (fix === "fix_permissions") return fixes.secretDialog(x, "fix", again);
      if (fix === "install_skiff") return fixes.installSkiff(c, x);
      if (fix === "start_skiff") {
        c.note = `Starting Skiff on ${x.name}…`;
        redraw();
        await invoke("start_skiff", { id: x.id });
        return again();
      }
      if (fix === "turn_on_linger") return fixes.turnOnLinger(x, again);
      if (fix === "update_skiff") return fixes.updateSkiff(c, x);
      if (fix === "copy_public_key") {
        await copyText(await invoke<string>("public_key", { id: x.id }));
        c.note = `Public key copied. Send it to the admin of ${x.name}, then check again.`;
        redraw();
      }
    } catch (e) {
      c.error = String(e);
      redraw();
    }
  }


  /** The Host's own setting: on, an older install Skiff made is updated as
   *  the app connects, when every session survives. */
  function autoUpdate(x: KnownHost): HTMLElement {
    const row = h("div", "set-row set-agent set-auto-update" + (x.auto_update ? " on" : ""));
    const sw = h("button", "c-on");
    sw.type = "button";
    sw.setAttribute("role", "switch");
    sw.setAttribute("aria-checked", String(x.auto_update));
    sw.setAttribute("aria-label", `Update Skiff here automatically ${x.auto_update ? "on" : "off"}`);
    sw.append(h("span", "switch"));
    const flip = async () => {
      sw.disabled = true;
      try {
        await invoke("set_host_auto_update", { id: x.id, on: !x.auto_update });
        await loadHosts();
        redraw();
      } catch (e) {
        sw.disabled = false;
        fixes.alertCheck(e);
      }
    };
    sw.addEventListener("click", () => void flip());
    const label = h("span", "c-text", "Update Skiff here automatically");
    label.addEventListener("click", () => void flip());
    row.append(sw, label);
    return row;
  }


  function rename(x: KnownHost) {
    promptAction({
      title: `Rename ${x.name}`,
      body: "The server stays the same.",
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

  /**
   * Remove forgets the Host here. When Skiff installed Skiff there, a box
   * also stops and uninstalls it. The box shows only after the server says
   * so, and the copy names the sessions that end and what stays.
   */
  function remove(x: KnownHost) {
    const kept = `Its sessions keep running on ${x.name}.`;
    const body = h("div", "confirm-body", kept);
    const extra = h("div", "set-remove-box");
    const status = h("div", "set-sub", `Checking ${x.name}…`);
    extra.append(status);
    let uninstall = false;
    let ends = "";
    const box = h("button", "set-check-label");
    box.type = "button";
    box.setAttribute("role", "switch");
    box.setAttribute("aria-checked", "false");
    box.addEventListener("click", () => {
      uninstall = !uninstall;
      box.setAttribute("aria-checked", String(uninstall));
      box.classList.toggle("on", uninstall);
      body.textContent = uninstall
        ? `Skiff uninstalls itself from ${x.name}. ${ends}Your other files stay.`
        : kept;
    });
    actionDialog({
      title: `Remove ${x.name}?`,
      content: [body, extra],
      action: "Remove Host",
      tone: "danger",
      submit: async () => {
        st.error = null;
        await invoke("remove_host", { id: x.id, uninstall });
        await loadHosts().catch(() => {});
        redraw();
      },
    });
    invoke<{ managed: boolean; sessions: string[] }>("uninstall_preview", { id: x.id }).then(
      (p) => {
        if (!p.managed) return void (status.textContent = `Skiff on ${x.name} is its own install. It stays.`);
        const n = p.sessions.length;
        ends = n === 0 ? "" : `This ends: ${p.sessions.join(", ")}. `;
        box.append(h("span", "switch"), h("span", "", `Also uninstall Skiff on ${x.name}. ${ends}`.trim()));
        status.replaceWith(box);
      },
      () => void (status.textContent = `${x.name} does not answer. Skiff removes it here only.`),
    );
  }


  return { load, open, draw, focusServer: form.focusServer, get editing() { return st.form !== null; } };
}

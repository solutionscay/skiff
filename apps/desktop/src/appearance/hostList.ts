import { h, button } from "../ui/dom";
import { rune } from "../ui/runes";
import type { MenuEntry } from "../ui/menu";
import { promptAction } from "../ui/confirm";
import { actionDialog } from "../ui/dialogParts";
import { type HostRow, type HostsEnv, type HostsState, alert, typed } from "./hostShared";

/** The list of known Hosts: a row for each, its menu, Rename and Remove.
 *  `openForm` shows the add/edit form. */
export function createHostList(
  env: HostsEnv,
  st: HostsState,
  deps: { runCheck(id: string): Promise<void>; checkThenUpdate(id: string, reinstall?: boolean): Promise<void>; openForm(x: HostRow | null): void },
) {
  function draw(): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head set-head-row");
    const words = h("div", "set-head-words");
    const title = h("div", "set-h", "Hosts");
    title.id = "set-h-hosts";
    words.append(title);
    const add = button("set-add ready", "Add SSH Host…", () => deps.openForm(null));
    head.append(words, add);
    main.append(head);
    const table = h("div", "set-table g-hosts");
    table.setAttribute("role", "table");
    table.setAttribute("aria-labelledby", "set-h-hosts");
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "c-lead"), h("span", "", "NAME"), h("span", "", "SERVER"), h("span", "", "SKIFF"), h("span", "", "STATE"), h("span", ""));
    table.append(cols, hostRow(null));
    for (const x of env.hosts.knownHosts()) table.append(hostRow(x));
    main.append(table);
    if (!env.hosts.knownHosts().length) main.append(h("div", "set-note set-line", "No servers yet."));
    if (st.error) main.append(alert(st.error));
    return main;
  }

  /** One Host. `null`: this computer, which has nothing to set: no menu,
   *  no click, no hover. A Host row opens its check list on a click or
   *  Enter; its other actions are in the row menu (⋯, right-click, the
   *  menu key). */
  function hostRow(x: HostRow | null): HTMLElement {
    const row = h("div", x ? "set-row set-host act" : "set-row set-host");
    row.setAttribute("role", "row");
    const shown = x ? env.hosts.shownHost() === x.id : env.hosts.shownHost() === null;
    const lead = h("span", "c-lead");
    if (shown) lead.append(rune("indicators-check"));
    lead.setAttribute("aria-label", shown ? "Shown in this window" : "");
    const skiff = h("span", "c-skiff mono" + (x?.update ? " update" : ""), x ? (x.skiff ?? "—") : st.version || "—");
    const more = h("span", "c-more");
    row.append(lead, h("span", "c-name", x ? x.name : env.hosts.localHostName()), h("span", "c-server mono", x ? typed(x) : env.hosts.localHostName()), skiff, stateCell(x), more);
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
    row.addEventListener("click", () => void deps.runCheck(x.id));
    row.addEventListener("contextmenu", (e: MouseEvent) => {
      e.preventDefault();
      openMenu({ x: e.clientX, y: e.clientY });
    });
    row.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter") void deps.runCheck(x.id);
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
  function hostMenu(x: HostRow, left: number, top: number, back: HTMLElement) {
    const entries: MenuEntry[] = [
      { icon: "indicators-check", label: "Check", run: () => void deps.runCheck(x.id) },
      ...(x.update ? [{ icon: "schedule-refresh-cw", label: "Update Skiff…", run: () => void deps.checkThenUpdate(x.id) }] : []),
      {
        icon: "tools-pencil",
        label: "Edit…",
        run: () => deps.openForm(x),
      },
      { label: "Rename…", run: () => rename(x) },
      { sep: true },
      { icon: "indicators-x", label: "Remove…", danger: true, run: () => void remove(x) },
    ];
    env.openMenu(left, top, x.name, entries, () => back.focus());
  }

  /** Dot and word, from the last check or connection in this app run. */
  function stateCell(x: HostRow | null): HTMLElement {
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


  function rename(x: HostRow) {
    promptAction({
      title: `Rename ${x.name}`,
      body: "The server stays the same.",
      placeholder: "a name",
      value: x.name,
      action: "Rename",
      submit: async (name) => {
        await env.ask("rename_host", { id: x.id, name });
        await env.hosts.loadHosts();
        env.redraw();
      },
    });
  }

  /**
   * Remove forgets the Host here. When Skiff installed Skiff there, a box
   * also stops and uninstalls it. The box shows only after the server says
   * so, and the copy names the sessions that end and what stays.
   */
  function remove(x: HostRow) {
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
        await env.ask("remove_host", { id: x.id, uninstall });
        await env.hosts.loadHosts().catch(() => {});
        env.redraw();
      },
    });
    env.ask<{ managed: boolean; sessions: string[] }>("uninstall_preview", { id: x.id }).then(
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

  return { draw };
}

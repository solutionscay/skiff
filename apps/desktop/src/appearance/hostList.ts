import { h, button } from "../ui/dom";
import { smallButton, switchButton } from "../ui/controls";
import { t } from "../i18n";
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
    const title = h("div", "set-h", t("settings.tab.hosts"));
    title.id = "set-h-hosts";
    words.append(title);
    const acts = h("div", "set-head-acts");
    acts.append(smallButton(t("hosts.addSsh"), () => deps.openForm(null), { go: true }));
    head.append(words, acts);
    main.append(head);
    const table = h("div", "set-table g-hosts");
    table.setAttribute("role", "table");
    table.setAttribute("aria-labelledby", "set-h-hosts");
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "c-lead"), h("span", "", t("hosts.colName")), h("span", "", t("hosts.colServer")), h("span", "", t("hosts.colSkiff")), h("span", "", t("hosts.colAuto")), h("span", "", t("hosts.colState")), h("span", ""));
    table.append(cols, hostRow(null));
    for (const x of env.hosts.knownHosts()) table.append(hostRow(x));
    main.append(table);
    if (!env.hosts.knownHosts().length) main.append(h("div", "set-note set-line", t("hosts.none")));
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
    lead.setAttribute("aria-label", shown ? t("hosts.shown") : "");
    const skiff = h("span", "c-skiff" + (x?.update ? " update" : ""));
    skiff.append(h("span", "mono", x ? (x.skiff ?? "—") : st.version || "—"));
    const auto = h("span", "c-auto");
    if (x) {
      if (x.update) {
        const up = smallButton(t("hosts.updateShort"), () => void deps.checkThenUpdate(x.id), { label: t("hosts.updateSkiff") });
        up.tabIndex = -1;
        up.addEventListener("click", (e) => e.stopPropagation());
        skiff.append(up);
      }
      const sw = switchButton({ label: t("hosts.autoOf", { name: x.name }), on: x.auto_update, toggle: () => void flipAuto(x) });
      sw.tabIndex = -1;
      sw.addEventListener("click", (e) => e.stopPropagation());
      auto.append(sw);
    }
    const more = h("span", "c-more");
    row.append(lead, h("span", "c-name", x ? x.name : env.hosts.localHostName()), h("span", "c-server mono", x ? typed(x) : ""), skiff, auto, stateCell(x), more);
    if (!x) return row;
    row.tabIndex = 0;
    row.setAttribute("aria-label", t("hosts.openCheck", { name: x.name }));
    const menu = button("c-menu", "⋯", () => {});
    menu.tabIndex = -1;
    menu.title = t("hosts.more");
    menu.setAttribute("aria-label", t("hosts.moreFor", { name: x.name }));
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

  /** Update Skiff on this Host automatically: an older install Skiff made is updated as the app connects, when every session survives. */
  async function flipAuto(x: HostRow) {
    try {
      await env.ask("set_host_auto_update", { id: x.id, on: !x.auto_update });
      await env.hosts.loadHosts();
    } catch (e) {
      st.error = String(e);
    }
    env.redraw();
  }

  /** The row menu: Check, Update Skiff… when offered, the auto update switch, Edit…, Rename…, Remove…. */
  function hostMenu(x: HostRow, left: number, top: number, back: HTMLElement) {
    const entries: MenuEntry[] = [
      { icon: "indicators-check", label: t("hosts.check"), run: () => void deps.runCheck(x.id) },
      ...(x.update ? [{ icon: "schedule-refresh-cw", label: t("hosts.updateSkiff"), run: () => void deps.checkThenUpdate(x.id) }] : []),
      { ...(x.auto_update ? { icon: "indicators-check" } : {}), label: t("hosts.colAuto"), run: () => void flipAuto(x) },
      {
        icon: "tools-pencil",
        label: t("hosts.edit"),
        run: () => deps.openForm(x),
      },
      { label: t("hosts.rename"), run: () => rename(x) },
      { sep: true },
      { icon: "indicators-x", label: t("hosts.remove"), danger: true, run: () => void remove(x) },
    ];
    env.openMenu(left, top, x.name, entries, () => back.focus());
  }

  /** Dot and word, from the last check or connection in this app run. */
  function stateCell(x: HostRow | null): HTMLElement {
    const [cls, word] = !x
      ? ["ok", t("hosts.connected")]
      : x.state === "ok"
        ? ["ok", t("hosts.connected")]
        : x.state === "attention"
          ? ["bad", t("hosts.needsAttention")]
          : ["none", t("hosts.notChecked")];
    const cell = h("span", `c-state ${cls}`);
    cell.append(h("span", "sq"), word);
    return cell;
  }


  function rename(x: HostRow) {
    promptAction({
      title: t("hosts.renameTitle", { name: x.name }),
      body: t("hosts.renameBody"),
      placeholder: t("hosts.namePlaceholder"),
      value: x.name,
      action: t("hosts.renameAction"),
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
    const kept = t("hosts.removeKept", { name: x.name });
    const body = h("div", "confirm-body", kept);
    const extra = h("div", "set-remove-box");
    const status = h("div", "set-sub", t("hosts.removeChecking", { name: x.name }));
    extra.append(status);
    let uninstall = false;
    let ends = "";
    const flip = () => {
      uninstall = !uninstall;
      box?.setAttribute("aria-checked", String(uninstall));
      body.textContent = uninstall ? t("hosts.removeUninstalls", { name: x.name, ends }) : kept;
    };
    let box: HTMLButtonElement | null = null;
    actionDialog({
      title: t("hosts.removeTitle", { name: x.name }),
      content: [body, extra],
      action: t("hosts.removeAction"),
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
        if (!p.managed) return void (status.textContent = t("hosts.removeOwnInstall", { name: x.name }));
        ends = p.sessions.length ? t("hosts.removeEnds", { names: p.sessions.join(", ") }) : "";
        box = switchButton({ label: t("hosts.removeAlso", { name: x.name }), text: `${t("hosts.removeAlso", { name: x.name })} ${ends}`.trim(), on: false, danger: true, toggle: flip });
        extra.replaceChildren(box);
      },
      () => void (status.textContent = t("hosts.removeNoAnswer", { name: x.name })),
    );
  }

  return { draw };
}

import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { ctxMenu } from "../ui/contextMenu";
import { runDialog } from "./hostProgress";
import { copyText } from "../platform/clipboard";
import { createHostCheck } from "./hostCheck";
import { createHostForm } from "./hostForm";
import { createHostList } from "./hostList";
import { type Alias, type HostsEnv, type HostsState } from "./hostShared";
import { type HostsPage, knownHosts, loadHosts, localHostName, pick, shownHost } from "../app/hosts";

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
  const check = createHostCheck(env, st, { reload: load });
  const form = createHostForm(env, st, { runCheck: check.runCheck, reload: load });
  const list = createHostList(env, st, {
    runCheck: check.runCheck,
    checkThenUpdate: check.checkThenUpdate,
    openForm: (x) => {
      form.open(x);
      redraw();
      form.focusServer();
    },
  });

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
    if ("check" in page) void (page.reinstall || page.update ? check.checkThenUpdate(page.check, !!page.reinstall) : check.runCheck(page.check));
  }

  function draw(): HTMLElement {
    // The Host it was about left the list: back to the list.
    const known = (id: string) => knownHosts().some((x) => x.id === id);
    if (st.check && !known(st.check.id)) st.check = null;
    if (st.form?.id && !known(st.form.id)) st.form = null;
    return st.form ? form.draw(st.form) : st.check ? check.drawCheck(st.check) : list.draw();
  }

  return { load, open, draw, focusServer: form.focusServer, get editing() { return st.form !== null; } };
}

import { connection, connectEndpoint, orderedConnections, retry } from "./connections";
import { openHostsPanel, hostIcon } from "./hostsPanel";
import { pageRoute, invokeAt, accepts, type HostRoute } from "../platform/invoke";
/** Saved SSH endpoints and controls for the focused Host. */
import { invoke } from "../platform/invoke";
import { listen } from "@tauri-apps/api/event";
import { confirmAction } from "../ui/confirm";
import { h } from "../ui/dom";
import { showError } from "../ui/alerts";
import { t } from "../i18n";

interface KnownHost {
  id: string;
  name: string;
  destination: string;
  user: string | null;
  port: number | null;
  /** Update Skiff on this server automatically. */
  auto_update: boolean;
  /** The last check or connection in this app run. */
  state: "ok" | "attention" | "unknown";
  /** The managed install's Skiff version from the last probe. `null`: not known. */
  skiff: string | null;
  /** That version is older than this app's: Update Skiff… is offered. */
  update: boolean;
}

interface HostList {
  /** The shown Host's id. `null`: this Mac. */
  selected: string | null;
  /** This computer's hostname; empty if the system gives none. */
  localName: string;
  hosts: KnownHost[];
  /** The shown remote Host's clipboard permission, for the menu: given to
   *  the identity this window accepted. Each write asks Rust again. */
  clipboard: boolean;
  /** This window shows a remote Host that is not in the list any more:
   *  its destination. */
  removed: string | null;
}

let list: HostList = { selected: null, localName: "", hosts: [], clipboard: false, removed: null };
let connected = true;
const changed = new Set<() => void>();
/** Refresh the endpoint list and permissions for the focused Host. */
export async function loadHosts(): Promise<void> {
  list = await invoke<HostList>("list_hosts");
  renderHostLabel();
  for (const run of changed) run();
}

/** Runs after each read of the Host list, such as one hosts.toml changed. */
export function onHostsChanged(run: () => void) {
  changed.add(run);
}

/** Follows hosts.toml as it changes, and shows the note a removed shown
 *  Host left before the reload. */
export function watchHosts() {
  void listen("skiff:hosts-changed", () => void loadHosts().catch(showError));

}

/** The window shows a remote Host: this Mac's projects, files, ledger and
 *  Skipper are not its data, and are not shown. */
export const isRemote = () => shownHost() !== null;

/** The local Host's label, on every OS: "This computer (thr0w-deb)". */
export const localHostName = () => (list.localName ? t("hosts.thisComputerName", { name: list.localName }) : t("hosts.thisComputer"));

export const hostLabel = (endpoint: string | null) => list.hosts.find((h) => h.id === endpoint)?.name ?? endpoint ?? localHostName();
export const hostPrefix = (endpoint: string | null) => endpoint ? hostLabel(endpoint) : list.localName || localHostName();
export const hostName = () => hostLabel(shownHost());

/** The known remote Hosts, as last loaded. */
export const knownHosts = (): readonly KnownHost[] => list.hosts;

/** The shown Host's id. `null`: this Mac. */
export const shownHost = () => pageRoute() ? pageRoute()!.endpoint : list.selected;

/** What a Host menu link opens in Settings › Hosts: its Add form, a Host's
 *  check list, or the list. */
export type HostsPage = { add: true } | { check: string; reinstall?: true; update?: true } | Record<string, never>;
let manage: (page: HostsPage) => void = () => {};

/** Opens the shown Host's check list in Settings › Hosts. */
export const checkShownHost = () => {
  if (list.selected) manage({ check: list.selected });
};

/** Opens the shown Host's check list, then Update Skiff…: the confirm,
 *  and the red one when the update would end sessions. */
export const updateShownHost = () => {
  if (list.selected) manage({ check: list.selected, update: true });
};

/** Opens the shown Host's check list, then Update Skiff…, which installs
 *  this app's build there even when the server has that build already. */
export const reinstallShownHost = () => {
  if (list.selected) manage({ check: list.selected, reinstall: true });
};

/** Settings registers how it opens on the Hosts tab. */
export function onManageHosts(open: (page: HostsPage) => void) {
  manage = open;
}

/** The menu's view: clipboard writes are allowed from the shown Host. */
export const clipboardAllowed = () => !isRemote() || list.clipboard;

/** A program in a terminal may write the clipboard now: always on this Mac,
 *  and on a remote Host only after the person allows it for the identity
 *  this window's connection accepted. Rust decides on every write, against
 *  the live connection and the file as it is now, never from what the page
 *  loaded: another window may have accepted another identity since. */
export async function clipboardWrites(): Promise<boolean> {
  if (!isRemote()) return true;
  return invoke<boolean>("host_clipboard").catch(() => false);
}

/** The link to the shown Host is up or down. While down, input is off. */
export function setHostConnected(up: boolean) {
  connected = up;
  renderHostLabel();
}

export const hostConnected = () => connected;

function renderHostLabel() {
  const name = hostName();
  document.title = `Skiff — ${name}`;
  let cell = document.getElementById("host-cell");
  const settings = document.getElementById("open-settings");
  if (!cell && settings) {
    cell = h("button", "tb-cell host-cell");
    cell.id = "host-cell";
    (cell as HTMLButtonElement).type = "button";
    cell.setAttribute("aria-haspopup", "dialog");
    cell.addEventListener("click", () => openHostsPanel(cell!, manage));
    settings.before(cell);
  }
  if (!cell) return;
  // The icon names the Host's kind. A dot shows only while the link is down.
  cell.replaceChildren(...(connected ? [] : [h("span", "dot error")]), hostIcon(isRemote()), h("span", "host-name", name));
  cell.classList.toggle("remote", isRemote());
  cell.title = t("settings.tab.hosts");
  cell.setAttribute("aria-label", cell.title + " · " + name);
}

/** True when the Host has a tree in the side panel: connected, connecting or needing attention. */
export const isConnected = (id: string | null) => orderedConnections().some((c) => c.endpoint === id);

/** Shows another Host in this window. Settings › Hosts picks with it too. */
export function pick(id: string | null) {
  void connectEndpoint(id).catch(showError);
}

export const manageHost = (page: HostsPage) => manage(page);
export async function toggleClipboard() {
  const route = pageRoute();
  if (!route?.endpoint) return;
  const allow = !clipboardAllowed();
  if (allow) {
    const ok = await confirmAction({
      title: t("hosts.clipboardTitle", { name: hostName() }),
      body: t("hosts.clipboardBody"),
      action: t("hosts.clipboardAction"),
    });
    if (!ok) return;
  }
  await invokeAt(route,"set_host_clipboard", { id: route.endpoint, allow }).catch(showError);
  await loadHosts().catch(showError);
}

let recovering = false;
let lostAgain = false;

/** Rust says the remote link ended. `reconnect` restores everything: it
 *  resolves only when every step worked, else it throws. A loss during a
 *  try is kept, and the whole recovery runs again. The Host shows connected,
 *  and `online` turns input back on, only after a full recovery with no loss
 *  in it. Tries wait 1 s, then twice as long each time, up to 30 s. */
export function watchHostLink(reconnect: () => Promise<void>, online: () => void) {
  void listen<HostRoute>("skiff:host-lost", ({ payload }) => {
    if (!accepts(payload)) return;
    if (recovering) {
      lostAgain = true;
      return;
    }
    if (!connected) return;
    void recover(reconnect, online);
  });
}

async function recover(reconnect: () => Promise<void>, online: () => void) {
  recovering = true;
  setHostConnected(false);
  let wait = 1000;
  for (;;) {
    lostAgain = false;
    const ok = await reconnect().then(
      () => true,
      (e) => (console.error(e), false),
    );
    if (ok && !lostAgain) break;
    await new Promise((r) => setTimeout(r, wait));
    wait = Math.min(wait * 2, 30_000);
  }
  recovering = false;
  setHostConnected(true);
  online();
}

/** The shown Host answered with another Skiff identity, and Skiff stopped.
 *  The person checks that Host, then accepts the new identity here. Its
 *  clipboard permission does not carry over. */
export async function acceptHostIdentity() {
  const route = pageRoute();
  if (!route?.endpoint) return;
  const name = hostName();
  const ok = await confirmAction({
    title: t("hosts.identityTitle", { name }),
    body: t("hosts.identityBody"),
    action: t("hosts.identityAction"),
  });
  if (!ok) return;
  await invokeAt(route,"accept_host_identity", { id: route.endpoint })
    .then(() => { const c = connection(route.entry); if (c) retry(c); })
    .catch(showError);
}

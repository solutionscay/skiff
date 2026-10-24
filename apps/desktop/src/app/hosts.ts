/** The Host this window shows, and the picker. "This Mac" is the local Host.
 *  A remote Host has a local id, a shown name, an SSH destination, and its
 *  skiffd socket. Settings › Hosts adds, edits and removes them. Picking
 *  another Host reloads the page: no view mixes two Hosts. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ctxMenu } from "../ui/contextMenu";
import type { MenuEntry } from "../ui/menu";
import { confirmAction } from "../ui/confirm";
import { h } from "../ui/dom";
import { showError } from "../ui/alerts";

export interface KnownHost {
  id: string;
  name: string;
  destination: string;
  user: string | null;
  port: number | null;
  /** Found by the last check, or set by the person. `null`: not found yet. */
  socket: string | null;
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
}

let list: HostList = { selected: null, localName: "", hosts: [], clipboard: false };
let connected = true;

/** Reads the known Hosts and the selected one. Call it before anything loads. */
export async function loadHosts(): Promise<void> {
  list = await invoke<HostList>("list_hosts");
  renderHostLabel();
}

/** The window shows a remote Host: this Mac's projects, files, ledger and
 *  Skipper are not its data, and are not shown. */
export const isRemote = () => list.selected !== null;

/** The local Host's label, on every OS: "This computer (thr0w-deb)". */
export const localHostName = () => (list.localName ? `This computer (${list.localName})` : "This computer");

export const hostName = () => list.hosts.find((x) => x.id === list.selected)?.name ?? list.selected ?? localHostName();

/** The known remote Hosts, as last loaded. */
export const knownHosts = (): readonly KnownHost[] => list.hosts;

/** The shown Host's id. `null`: this Mac. */
export const shownHost = () => list.selected;

/** What a Host menu link opens in Settings › Hosts: its Add form, a Host's
 *  check list, or the list. */
export type HostsPage = { add: true } | { check: string } | Record<string, never>;
let manage: (page: HostsPage) => void = () => {};

/** Opens the shown Host's check list in Settings › Hosts. */
export const checkShownHost = () => {
  if (list.selected) manage({ check: list.selected });
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
    cell.setAttribute("aria-haspopup", "menu");
    cell.addEventListener("click", () => openHostMenu(cell!));
    settings.before(cell);
  }
  if (!cell) return;
  const state = connected ? "connected" : "error";
  cell.replaceChildren(h("span", `dot ${state}`), h("span", "host-name", name));
  cell.classList.toggle("remote", isRemote());
  cell.title = connected
    ? `Host: ${name}. Pick another Host.`
    : `Host: ${name}, disconnected. Input is off. Skiff connects again on its own.`;
  cell.setAttribute("aria-label", cell.title);
}

/** Shows another Host in this window. Settings › Hosts picks with it too. */
export function pick(id: string | null) {
  if (id === list.selected) return;
  invoke("select_host", { id })
    .then(() => location.reload())
    .catch(showError);
}

/** The Host menu: pick a Host, add one, or manage them in Settings. */
function openHostMenu(anchor: HTMLElement) {
  const r = anchor.getBoundingClientRect();
  const mark = (on: boolean) => (on ? "indicators-check" : undefined);
  const entries: MenuEntry[] = [
    { icon: mark(!isRemote()), label: localHostName(), run: () => pick(null) },
    // A Host that needs attention opens its check list, not a page that cannot load.
    ...list.hosts.map((x) => ({
      icon: mark(x.id === list.selected),
      label: x.name,
      hint: x.state === "attention" ? "Needs attention" : undefined,
      run: () => (x.state === "attention" ? manage({ check: x.id }) : pick(x.id)),
    })),
    { icon: "indicators-plus", label: "Add SSH Host…", run: () => manage({ add: true }) },
  ];
  const shown = list.selected;
  if (shown && !connected) entries.push({ icon: "indicators-check", label: `Check ${hostName()}…`, run: () => manage({ check: shown }) });
  if (isRemote()) {
    entries.push({
      icon: "code-copy",
      label: clipboardAllowed() ? "Block clipboard writes from this Host" : "Allow clipboard writes from this Host…",
      run: () => void toggleClipboard(),
    });
  }
  entries.push({ icon: "tools-pencil", label: "Manage Hosts…", run: () => manage({}) });
  ctxMenu.open(r.left, r.bottom, "Host", entries);
}

async function toggleClipboard() {
  const allow = !clipboardAllowed();
  if (allow) {
    const ok = await confirmAction({
      title: `Allow clipboard writes from ${hostName()}?`,
      body: "A program in a terminal on this Host can then replace this computer's clipboard with OSC 52. It cannot read the clipboard.",
      action: "Allow writes",
    });
    if (!ok) return;
  }
  await invoke("set_host_clipboard", { id: list.selected, allow }).catch(showError);
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
  void listen("skiff:host-lost", () => {
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
  const name = hostName();
  const ok = await confirmAction({
    title: `Accept the new identity of ${name}?`,
    body: "Skiff then connects to the Host that answers there now. Accept only if you know why it changed, such as a new install of Skiff on that Host. Clipboard writes from it are blocked again.",
    action: "Accept identity",
  });
  if (!ok) return;
  await invoke("accept_host_identity", { id: list.selected })
    .then(() => location.reload())
    .catch(showError);
}

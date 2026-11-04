import { h } from "../ui/dom";
import type { MessageKey } from "../i18n";
import type { MenuEntry } from "../ui/menu";

/** Shared types, labels and state of the Hosts tab. The facade owns every
 *  Host and platform call and hands them to the controllers through `env`. */

/** The fields of a known Host that this tab uses. The facade assigns its own list to this shape. */
export interface HostRow {
  id: string;
  name: string;
  destination: string;
  user: string | null;
  port: number | null;
  auto_update: boolean;
  state: "ok" | "attention" | "unknown";
  skiff: string | null;
  update: boolean;
}

export interface Resolved {
  hostname: string;
  user: string;
  port: number;
  jump: string;
}

export interface Alias {
  name: string;
  goes_to: string;
}

export type Draft = { destination: string; name: string; user: string; port: string };

/** `typing`: Other server…: the fields show in place of the SSH config list. */
export type Form = { id: string | null; draft: Draft; typing: boolean; error: string | null; busy: boolean };

/** One row of the check list, as Rust's `check_host` answers it. */
export interface CheckLine {
  row: string;
  title: string;
  state: "passed" | "failed" | "skipped" | "warning";
  text: string;
  fix: Fix | null;
}
export type Fix = "trust_key" | "review_key" | "make_key" | "unlock_key" | "install_key" | "copy_public_key" | "fix_permissions" | "install_skiff" | "start_skiff" | "turn_on_linger" | "update_skiff";
export const FIX_LABEL: Record<Fix, MessageKey> = {
  trust_key: "hosts.fix.trustKey",
  review_key: "hosts.fix.reviewKey",
  make_key: "hosts.fix.makeKey",
  unlock_key: "hosts.fix.unlockKey",
  install_key: "hosts.fix.installKey",
  copy_public_key: "hosts.fix.showKey",
  fix_permissions: "hosts.fix.permissions",
  install_skiff: "hosts.fix.installSkiff",
  start_skiff: "hosts.fix.start",
  turn_on_linger: "hosts.fix.linger",
  update_skiff: "hosts.updateSkiff",
};
/** What `update_skiff` did. */
export type Updated = { version: string; source: string } & (
  | { result: "done"; from: string; restarted: boolean }
  | { result: "same" }
  | { result: "newer"; from: string }
  | { result: "no_handover"; from: string; sessions: string[] }
);
export interface Fingerprint {
  kind: string;
  sha256: string;
}
export interface HostKey {
  token: string;
  fingerprints: Fingerprint[];
  known: Fingerprint[];
  file: string;
}
export interface Report {
  rows: CheckLine[];
  ok: boolean;
  /** The socket in use. */
  socket: string | null;
  /** The server's OS and arch as bundle names write them. */
  platform: [string, string] | null;
  /** The Skiff version this app installs. */
  version: string;
  /** The server has this version in another build. */
  other_build: boolean;
}
/** The rows, in order, before an answer. These are Rust's titles, which the code matches on. */
export const CHECK_ROWS = [
  "Reach the server",
  "Host key",
  "Log in without a prompt",
  "Server supported",
  "Skiff on the server",
  "Sessions service (skiffd)",
  "Socket forwarding",
  "Skiff identity and version",
  "Projects service (skiff-host)",
  "Keeps running after logout",
];

/** A run's outcome on its check list row: the text, the time it took or
 *  the failure with its fix, and the output to copy. */
export type RowResult = { row: string; text: string; secs?: number; failed?: boolean; fix?: Fix; output?: string };
export type Check = { id: string; report: Report | null; error: string | null; note?: string; result?: RowResult };

/** The option shape of the facade's run dialog. */
export interface RunDialogOptions<T> {
  id: string;
  title: string;
  body: string;
  action: string;
  runningTitle: string;
  working: string;
  failTitle: string;
  steps: string[];
  tone: "go" | "danger";
  run: () => Promise<T>;
  done: (result: T, secs: number, output: string) => void;
  failed?: (cause: string, output: string) => void;
}

/** What the controllers may call. The facade wires each one: `ask` is dependency
 *  injection of the facade's Host command call, not a new Host route. */
export interface HostsEnv {
  ask<T>(cmd: string, args?: object): Promise<T>;
  redraw(): void;
  hosts: {
    knownHosts(): readonly HostRow[];
    loadHosts(): Promise<void>;
    localHostName(): string;
    shownHost(): string | null;
    pick(id: string): void;
  };
  copyText(text: string): Promise<void>;
  runDialog<T>(o: RunDialogOptions<T>): void;
  openMenu(left: number, top: number, title: string, entries: MenuEntry[], onClose: () => void): void;
}

/** The mutable state the controllers share. */
export interface HostsState {
  form: Form | null;
  check: Check | null;
  error: string | null;
  aliases: Alias[];
  /** This app's version, for this computer's Skiff cell. */
  version: string;
  /** The form's resolve line, and the input it answers. */
  line: { for: string; text: string; bad: boolean };
  timer: ReturnType<typeof setTimeout> | undefined;
  lastResolved: Resolved | null;
}

export const typed = (x: HostRow) => `${x.user ? `${x.user}@` : ""}${x.destination}${x.port ? `:${x.port}` : ""}`;

export const defaultName = (d: string) => d.split("@").pop() ?? d;

export function alert(text: string): HTMLElement {
  const err = h("div", "set-error set-line", text);
  err.setAttribute("role", "alert");
  return err;
}

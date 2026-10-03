/**
 * The peek: one full-size terminal over the terminal area. Settings › Open
 * with commands run in it. It is not a session to the user: no sidebar row,
 * no group, no split. skiffd owns its PTY, flagged `peek`, so it is not
 * saved or listed.
 *
 * A command starts hidden, and what it does decides what the user sees:
 * - It takes the terminal (raw input or the alternate screen), as `micro`,
 *   `less` or a pager does: a terminal tool. The peek shows and has the keys.
 *   When the tool exits, the peek closes.
 * - It prints and exits: the peek shows the output until a key.
 * - It prints nothing and exits, or keeps running without the terminal: an
 *   app with a window of its own. The peek never shows, and the app runs on.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { $, button, h } from "../ui/dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import { showError } from "../ui/alerts";
import { appKey, currentRow, markCurrent } from "../app/keyboard";
import type { Action } from "../app/keys";
import { S, sessions } from "../app/state";
import type { SessionInfo } from "../platform/types";
import { toBytes } from "../platform/ipcBytes";

import { panes } from "./terminalState";
import { fitShown } from "./terminal";
import { FONT, TERM_THEME } from "./terminalRuntime";

/** Where to find tools for a row that has none. */
export const TOOLS_URL = "https://github.com/solutionscay/skiff/wiki/Open-with";

/** A command that has not taken the terminal by now is an app with its own window. */
const APP_AFTER_MS = 3000;

type Opened =
  | { kind: "run"; script: string; cwd: string }
  | { kind: "default" }
  | { kind: "no_app"; message: string };

interface Peek {
  takeFocus: boolean;
  request: string;
  el: HTMLElement;
  term: Terminal;
  fit: FitAddon;
  resize: ResizeObserver;
  /** The command's session, once skiffd started it. */
  session: string | null;
  /** The program, for prompts and messages. */
  program: string;
  /** On screen. Until then the command runs hidden. */
  shown: boolean;
  exited: boolean;
  /** The tool took the terminal: it closes the peek when it exits. */
  interactive: boolean;
  timer: number;
}

let open: Peek | null = null;
/** Running file tools kept off screen while another file or session shows. */
const parked = new Map<string, Peek>();
let invitation: HTMLElement | null = null;
let previewPath: string | null = null;
let selection = 0;
/** The selection whose preview takes the keys when it is ready. */
let keysFor = -1;

function clearInvitation() {
  invitation?.remove();
  invitation = null;
}
/** Commands that ended before their start call returned. */
const early = new Map<string, number | null>();
/** Commands that took the terminal before their start call returned. Tools such as micro do it within milliseconds. */
const earlyInteractive = new Set<string>();
/** Apps let go from the peek. Their session goes when they exit. */
const released = new Set<string>();
/** The last request. A slow start that answers after a newer one is dropped. */
let seq = 0;
/** One number per subscription, as panes count theirs. */
let stream = 0;

/** The open peek's session, if a command runs in it. */
export function peekSession(): string | null {
  return open?.session ?? null;
}

/** A peek session that is not the open peek's: left behind, or an app let go. */
export function strayPeek(s: SessionInfo) {
  if (s.id === open?.session || [...parked.values()].some((p) => p.session === s.id)) return;
  // An app still running keeps its window. Its session goes when it exits.
  // A terminal tool out of the peek has no screen left: it ends.
  if (s.state === "done" || s.interactive) invoke("kill_session", { session: s.id }).catch(console.error);
  else released.add(s.id);
}

/**
 * Going to a session leaves the peek, once its command is done. A command
 * still starting stays: nothing shows yet, and an open request often comes
 * with a key or click that sends the keys back to a session. A tool that
 * runs stays alive when navigation hides the peek. Opening it again restores it.
 */
export function leavePeek() {
  selection++;
  previewPath = null;
  clearInvitation();
  if (open && !open.shown && !open.takeFocus) dismiss(false);
  if (!open?.shown) return;
  if (open.exited) dismiss(false);
  else open.el.hidden = true;
}

/** Navigation can leave a tool. Other app keys stay out of its terminal. */
export function peekBlocksAction(action: Action | number | null): boolean {
  if (!open?.shown || open.el.hidden || open.exited) return false;
  return typeof action !== "number" && action !== "session-next" && action !== "session-prev"
    && action !== "list-project" && action !== "list-back"
    && action !== "region-next" && action !== "region-prev";
}

/** The terminal region gives the keys to a visible peek before a session. */
export function focusPeek(): boolean {
  if (invitation) {
    (invitation.querySelector<HTMLButtonElement>("button") ?? invitation).focus();
    return true;
  }
  if (!open?.shown || open.el.hidden) return false;
  open.term.focus();
  return true;
}

/**
 * Ctrl+Shift+Up/Down reached a file or folder: its preview takes the keys, as a
 * session's pane does. A tool gets the terminal. An Open button gets focus.
 */
export function keysToPreview() {
  keysFor = selection;
  if (open && !open.shown) open.takeFocus = true;
  else focusPeek();
}

/** Takes the peek down. Its command ends, unless it is an app that runs on. */
function drop(p: Peek, end: boolean) {
  if (open === p) open = null;
  if (parked.get(p.request) === p) parked.delete(p.request);
  clearTimeout(p.timer);
  p.resize.disconnect();
  p.term.dispose();
  p.el.remove();
  if (!p.session) return;
  if (end || p.exited) invoke("kill_session", { session: p.session }).catch(console.error);
  else released.add(p.session);
}

/** Keep a live editor when selection moves to another file. */
function replacePeek() {
  if (open?.interactive && !open.exited) {
    open.el.hidden = true;
    parked.set(open.request, open);
    open = null;
  } else dismiss(false);
}

/** Closes the peek and ends its tool. Navigation hides a running tool instead. */
export function closePeek() {
  dismiss(true);
}

/** A replacement keeps the current row and does not give focus to a pane. */
function dismiss(restore: boolean) {
  if (!open) return;
  const p = open;
  const hadKeys = !p.el.hidden && (p.el.contains(document.activeElement) || document.activeElement === document.body);
  seq++;
  // A hidden command is an app being started: it keeps running.
  drop(p, p.shown);
  if (!p.shown) return;
  // Refit and repaint the live panes after the overlay leaves the screen.
  fitShown();
  for (const pane of panes.values()) {
    if (!pane.parked && pane.el.isConnected) pane.term.refresh(0, pane.term.rows - 1);
  }
  markCurrent();
  if (!restore || !hadKeys) return;
  // Keep the highlight where the user left it. A sidebar row may have been rebuilt.
  const row = S.atRail ? document.querySelector<HTMLElement>("#rail .rail-chip.active") : currentRow();
  if (row) row.focus();
  else if (S.focused) panes.get(S.focused)?.term.focus();
}

function frame(kind: string, title: string, where: string, program: string, request: string, takeFocus: boolean): Peek {
  replacePeek();
  const el = h("div", "peek pending");
  el.setAttribute("role", "dialog");
  el.setAttribute("aria-label", title);
  const head = h("div", "peek-head");
  head.append(h("span", "peek-kind", kind), h("span", "peek-title mono", title), h("span", "peek-where mono", where));
  const box = h("div", "peek-body");
  el.append(head, box);
  // Laid out but not visible, so the terminal has its real size from the start.
  $("main").appendChild(el);

  const term = new Terminal({
    theme: TERM_THEME,
    fontFamily: FONT,
    fontSize: S.fontSize,
    lineHeight: 1.2,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(box);
  const p: Peek = {
    takeFocus, request, el, term, fit, resize: new ResizeObserver(() => resized(p)),
    session: null, program, shown: false, exited: false, interactive: false, timer: 0,
  };
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown") return true;
    // After the command ends, any key closes the peek.
    if (p.exited) {
      if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return false;
      e.preventDefault();
      closePeek();
      return false;
    }
    // Navigation runs in the window handler. Copy and paste work in the tool.
    const key = appKey(e);
    if (key === "copy" || key === "paste") {
      e.preventDefault();
      if (key === "copy") void writeText(term.getSelection()).catch(showError);
      else void readText().then((t) => t && term.paste(t), showError);
      return false;
    }
    return key === null;
  });
  term.onData((data) => {
    if (p.session && !p.exited) invoke("pty_write", { session: p.session, data }).catch(console.error);
  });
  p.resize.observe(box);
  open = p;
  fit.fit();
  return p;
}

function show(p: Peek) {
  if (open !== p) return;
  clearInvitation();
  if (p.shown && !p.el.hidden) return;
  p.el.hidden = false;
  p.shown = true;
  clearTimeout(p.timer);
  p.el.classList.remove("pending");
  if (!p.exited) p.el.classList.add("busy");
  resized(p);
  if (p.takeFocus) p.term.focus();
}

function resized(p: Peek) {
  if (open !== p) return;
  p.fit.fit();
  if (p.session && !p.exited) invoke("pty_resize", { session: p.session, cols: p.term.cols, rows: p.term.rows }).catch(console.error);
}

/** Starts `script`, hidden. What it does decides whether the peek shows. */
async function run(kind: string, title: string, where: string, script: string, cwd: string, takeFocus = true) {
  const request = JSON.stringify([kind, title, where, script, cwd]);
  const saved = parked.get(request);
  if (saved) {
    replacePeek();
    parked.delete(request);
    open = saved;
  }
  if (open?.request === request && !open.exited) {
    open.takeFocus = takeFocus;
    if (open.shown) show(open);
    if (takeFocus && open.shown) open.term.focus();
    return;
  }
  const program = script.trim().split(/\s+/)[0] ?? kind;
  const p = frame(kind, title, where, program, request, takeFocus);
  // Replacing a pending peek cancels its request. Take this number afterward.
  const mine = ++seq;
  let info: SessionInfo;
  try {
    info = await invoke<SessionInfo>("create_session", {
      spec: { label: program, role: "task", cwd, command: "/bin/sh", args: ["-c", script], peek: true, cols: p.term.cols, rows: p.term.rows },
    });
  } catch (e) {
    if (mine === seq) closePeek();
    return showError(e);
  }
  // Closed meanwhile, or a newer request came: the command runs on its own.
  if (mine !== seq || open !== p) {
    earlyInteractive.delete(info.id);
    if (info.interactive) invoke("kill_session", { session: info.id }).catch(console.error);
    else released.add(info.id);
    return;
  }
  // A skiffd from before the peek ran it as a plain session, with a shell after it.
  if (!info.peek) {
    invoke("kill_session", { session: info.id }).catch(console.error);
    closePeek();
    return showError("This skiffd is older than the app. Restart skiffd to open files and diffs.");
  }
  p.session = info.id;
  // Still hidden and still running: an app with its own window. Let it go.
  p.timer = window.setTimeout(() => {
    if (open === p && !p.shown && !p.exited) closePeek();
  }, APP_AFTER_MS);
  const n = ++stream;
  const channel = new Channel<unknown>();
  channel.onmessage = (m) => {
    if (open !== p && parked.get(p.request) !== p) return;
    const bytes = toBytes(m);
    p.term.write(bytes, () => invoke("ack_output", { session: info.id, stream: n, bytes: bytes.length }).catch(console.error));
  };
  await invoke("subscribe_output", { session: info.id, stream: n, onOutput: channel }).catch(showError);
  if (info.interactive || earlyInteractive.delete(info.id)) peekUpdated({ ...info, interactive: true });
  if (early.has(info.id)) {
    const code = early.get(info.id) ?? null;
    early.delete(info.id);
    peekExited(info.id, code);
  }
}

/** skiffd changed a peek session: the command took the terminal. */
export function peekUpdated(info: SessionInfo) {
  if (!info.interactive) return;
  // A command let go as an app turned out to be a terminal tool: nothing shows it, so it ends.
  if (released.delete(info.id)) return void invoke("kill_session", { session: info.id }).catch(console.error);
  const p = open;
  if (!p) return;
  // A start is in flight: the update may be its command's, ahead of the reply.
  if (!p.session) return void earlyInteractive.add(info.id);
  if (p.session !== info.id || p.interactive) return;
  p.interactive = true;
  show(p);
}

/** The terminal holds text: the command printed something. */
function printed(term: Terminal): boolean {
  const b = term.buffer.active;
  for (let y = 0; y < b.length; y++) if (b.getLine(y)?.translateToString(true).trim()) return true;
  return false;
}

/** skiffd says a peek command ended. True when the session was a peek's. */
export function peekExited(id: string, code: number | null): boolean {
  if (released.delete(id)) {
    invoke("kill_session", { session: id }).catch(console.error);
    return true;
  }
  const saved = [...parked.values()].find((p) => p.session === id);
  if (saved) {
    saved.exited = true;
    drop(saved, true);
    return true;
  }
  const p = open;
  if (!p) return false;
  // A start is in flight: the exit may be its command's, ahead of the reply.
  if (!p.session && !sessions.has(id)) {
    early.set(id, code);
    return true;
  }
  if (p.session !== id) return false;
  p.exited = true;
  p.el.classList.remove("busy");
  // The last output can still be on its way: it comes on another channel.
  window.setTimeout(() => {
    if (open !== p) return;
    // A tool that took the terminal is done with it.
    if (p.interactive && code === 0) return closePeek();
    // Nothing printed, no error: an app took the file and runs on its own.
    if (!p.shown && !code && !printed(p.term)) return closePeek();
    show(p);
    p.term.write(`\r\n\x1b[2m${code ? `${p.program} exited with code ${code}. ` : ""}Press any key to close.\x1b[0m`);
  }, 150);
  return true;
}

/** Shows the diff of `file` in worktree `wt`, or of the whole worktree, with Settings › Open with. */
export function showDiff(wt: string, branch: string, file?: string) {
  const mine = ++selection;
  previewPath = null;
  showInvitation("diff", `${wt}/${file ?? "All changes"}`);
  void invoke<Opened>("open_diff", { path: wt, file: file ?? null })
    .then((o) => { if (mine === selection) handle(o, "diff", file ?? "All changes", branch); })
    .catch(showError);
}

/** Opens a file with Settings › Open with, else in the system's default app. */
export function openFile(path: string) {
  const mine = ++selection;
  previewPath = path;
  const name = path.split("/").pop() ?? path;
  const dir = path.slice(0, path.length - name.length - 1);
  void invoke<Opened>("open_file", { path })
    .then((o) => { if (mine === selection) handle(o, "file", name, dir); })
    .catch(showError);
}

type FilePreview = { script: string | null; cwd: string; app: string; peek: boolean };

function showInvitation(kind: string, path: string, label?: string, run?: () => void) {
  const name = path.split("/").pop() ?? path;
  const dir = path.slice(0, path.length - name.length - 1);
  const el = h("div", "peek file-preview");
  el.tabIndex = -1;
  el.setAttribute("aria-label", name);
  const head = h("div", "peek-head");
  head.append(h("span", "peek-kind", kind), h("span", "peek-title mono", name), h("span", "peek-where mono", dir));
  const body = h("div", "file-preview-body");
  if (label && run) body.append(h("div", "file-preview-name", name), button("file-preview-open", label, run));
  el.append(head, body);
  // Tab does nothing here, as it leaves no pane.
  el.addEventListener("keydown", (e) => {
    if (e.key === "Tab") e.preventDefault();
  });
  const hadKeys = !!invitation?.contains(document.activeElement);
  clearInvitation();
  invitation = el;
  $("main").appendChild(el);
  if (hadKeys || keysFor === selection) focusPeek();
}

/** Folder selection stays in the preview until the user goes to a session. */
export function previewFolder(path: string) {
  if (previewPath === path) return;
  leavePeek();
  previewPath = path;
  showInvitation("folder", path, "Show in file manager", () => {
    void invoke<Opened>("open_file", { path })
      .then((o) => handle(o, "folder", path.split("/").pop() ?? path, path))
      .catch(showError);
  });
}

/** Selection starts peek commands only. External apps wait for an explicit open. */
export function previewFile(path: string) {
  if (previewPath === path) return;
  leavePeek();
  previewPath = path;
  const mine = ++selection;
  const name = path.split("/").pop() ?? path;
  const dir = path.slice(0, path.length - name.length - 1);
  // Keep the sessions covered without offering an action before the setting is known.
  showInvitation("file", path);
  void invoke<FilePreview>("preview_file", { path }).then((p) => {
    if (mine !== selection) return;
    if (p.peek && p.script) {
      void run("file", name, dir, p.script, p.cwd, mine === keysFor);
      return;
    }
    showInvitation("file", path, `Open in ${p.app}`, () => openFile(path));
  }).catch(showError);
}

function handle(o: Opened, kind: string, title: string, where: string) {
  if (o.kind === "run") {
    void run(kind, title, where, o.script, o.cwd);
    return;
  }
  if (o.kind === "no_app") {
    showError(`No app opens ${title}. Set one in Settings › Open with.`, { label: "Find a tool", run: () => openUrl(TOOLS_URL) });
  }
}

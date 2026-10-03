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
import { icon } from "../ui/icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { showError } from "../ui/alerts";
import { confirmAction } from "../ui/confirm";
import { appKey, markCurrent } from "../app/keyboard";
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
/** Commands that ended before their start call returned. */
const early = new Map<string, number | null>();
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
  if (s.id === open?.session) return;
  // An app still running keeps its window. Its session goes when it exits.
  if (s.state === "done") invoke("kill_session", { session: s.id }).catch(console.error);
  else released.add(s.id);
}

/** A tool shows in the peek and has the keys. App keys rest until it ends. */
export function peekBusy(): boolean {
  return !!open?.shown && !!open.session && !open.exited;
}

/** Takes the peek down. Its command ends, unless it is an app that runs on. */
function drop(p: Peek, end: boolean) {
  if (open === p) open = null;
  clearTimeout(p.timer);
  p.resize.disconnect();
  p.term.dispose();
  p.el.remove();
  if (!p.session) return;
  if (end || p.exited) invoke("kill_session", { session: p.session }).catch(console.error);
  else released.add(p.session);
}

/** Closes the peek and ends its tool. Navigation calls it: leaving the view leaves the tool. */
export function closePeek() {
  if (!open) return;
  const p = open;
  seq++;
  // A hidden command is an app being started: it keeps running.
  drop(p, p.shown);
  if (!p.shown) return;
  // Refit and repaint the live panes after the overlay leaves the screen.
  fitShown();
  for (const pane of panes.values()) {
    if (!pane.parked && pane.el.isConnected) pane.term.refresh(0, pane.term.rows - 1);
  }
  const back = S.focused ? panes.get(S.focused) : undefined;
  // The keys go back to the pane, and the highlight to its row.
  if (S.focused) S.roveKey = S.focused;
  markCurrent();
  back?.term.focus();
}

/** True when no tool runs in the peek, or the user agreed to end it. */
async function mayEnd(): Promise<boolean> {
  const p = open;
  if (!p || !peekBusy()) return true;
  const ok = await confirmAction({
    title: `End ${p.program}?`,
    body: `${p.program} still runs in the peek. Closing ends it, and unsaved work in it is lost.`,
    action: "End",
  });
  if (!ok && open === p) p.term.focus();
  return ok && open === p;
}

/** The close button. A tool that still runs can hold unsaved work, so it asks. */
async function askClose() {
  if (await mayEnd()) closePeek();
}

function frame(kind: string, title: string, where: string, program: string): Peek {
  closePeek();
  const el = h("div", "peek pending");
  el.setAttribute("role", "dialog");
  el.setAttribute("aria-label", title);
  const head = h("div", "peek-head");
  // The same close button a pane has.
  const close = button("head-btn", "", () => void askClose());
  close.title = "Close";
  close.setAttribute("aria-label", "Close the peek");
  close.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
  head.append(h("span", "peek-kind", kind), h("span", "peek-title mono", title), h("span", "peek-where mono", where), close);
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
    el, term, fit, resize: new ResizeObserver(() => resized(p)),
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
    // App keys rest while the tool runs, but copy and paste work in it.
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
  if (p.shown || open !== p) return;
  p.shown = true;
  clearTimeout(p.timer);
  p.el.classList.remove("pending");
  if (!p.exited) p.el.classList.add("busy");
  p.term.focus();
}

function resized(p: Peek) {
  if (open !== p) return;
  p.fit.fit();
  if (p.session && !p.exited) invoke("pty_resize", { session: p.session, cols: p.term.cols, rows: p.term.rows }).catch(console.error);
}

/** Starts `script`, hidden. What it does decides whether the peek shows. */
async function run(kind: string, title: string, where: string, script: string, cwd: string) {
  const program = script.trim().split(/\s+/)[0] ?? kind;
  const mine = ++seq;
  const p = frame(kind, title, where, program);
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
    released.add(info.id);
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
    if (open !== p) return;
    const bytes = toBytes(m);
    p.term.write(bytes, () => invoke("ack_output", { session: info.id, stream: n, bytes: bytes.length }).catch(console.error));
  };
  await invoke("subscribe_output", { session: info.id, stream: n, onOutput: channel }).catch(showError);
  if (info.interactive) peekUpdated(info);
  if (early.has(info.id)) {
    const code = early.get(info.id) ?? null;
    early.delete(info.id);
    peekExited(info.id, code);
  }
}

/** skiffd changed a peek session: the command took the terminal. */
export function peekUpdated(info: SessionInfo) {
  const p = open;
  if (!p || p.session !== info.id || !info.interactive || p.interactive) return;
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
  void invoke<Opened>("open_diff", { path: wt, file: file ?? null })
    .then((o) => handle(o, "diff", file ?? "All changes", branch))
    .catch(showError);
}

/** Opens a file with Settings › Open with, else in the system's default app. */
export function openFile(path: string) {
  const name = path.split("/").pop() ?? path;
  const dir = path.slice(0, path.length - name.length - 1);
  void invoke<Opened>("open_file", { path })
    .then((o) => handle(o, "file", name, dir))
    .catch(showError);
}

async function handle(o: Opened, kind: string, title: string, where: string) {
  if (o.kind === "run") {
    if (await mayEnd()) void run(kind, title, where, o.script, o.cwd);
    return;
  }
  if (o.kind === "no_app") {
    showError(`No app opens ${title}. Set one in Settings › Open with.`, { label: "Find a tool", run: () => openUrl(TOOLS_URL) });
  }
}

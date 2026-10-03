/**
 * The peek: one full-size terminal over the terminal area. It runs one
 * terminal tool from Settings › Open with, such as `git diff`, `less` or
 * `micro`. It is not a session to the user: no sidebar row, no group, no
 * split. skiffd owns its PTY, flagged `peek`, so it is not saved or listed.
 *
 * While the tool runs, it has the keys and app keys rest. When it exits:
 * a full-screen tool (an editor, a pager) closes the peek at once; a tool
 * that only printed leaves its output up, and any key closes it.
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

type Opened =
  | { kind: "peek"; script: string; cwd: string }
  | { kind: "window" }
  | { kind: "no_app"; message: string };

interface Peek {
  el: HTMLElement;
  term: Terminal;
  fit: FitAddon;
  resize: ResizeObserver;
  /** The tool's session, once skiffd started it. */
  session: string | null;
  /** The program the peek runs, for the close prompt. */
  program: string;
  exited: boolean;
  /** The tool drew on the alternate screen: it is full-screen. */
  usedAlt: boolean;
}

let open: Peek | null = null;
/** Peek tools that ended before their start call returned. */
const early = new Map<string, number | null>();
/** The last request. A slow start that answers after a newer one is dropped. */
let seq = 0;
/** One number per subscription, as panes count theirs. */
let stream = 0;

/** The open peek's session, if a tool runs in it. */
export function peekSession(): string | null {
  return open?.session ?? null;
}

/** A tool runs in the peek and has the keys. App keys rest until it ends. */
export function peekBusy(): boolean {
  return !!open && !!open.session && !open.exited;
}

/** Closes the peek and ends its tool. Navigation calls it: leaving the view leaves the tool. */
export function closePeek() {
  if (!open) return;
  const p = open;
  open = null;
  seq++;
  p.resize.disconnect();
  p.term.dispose();
  p.el.remove();
  if (p.session) invoke("kill_session", { session: p.session }).catch(console.error);
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
  const el = h("div", "peek");
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
  const p: Peek = { el, term, fit, resize: new ResizeObserver(() => resized(p)), session: null, program, exited: false, usedAlt: false };
  term.buffer.onBufferChange((b) => {
    if (b.type === "alternate") p.usedAlt = true;
  });
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown") return true;
    // After the tool ends, any key closes the peek.
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
  term.focus();
  return p;
}

function resized(p: Peek) {
  if (open !== p) return;
  p.fit.fit();
  if (p.session && !p.exited) invoke("pty_resize", { session: p.session, cols: p.term.cols, rows: p.term.rows }).catch(console.error);
}

/** Starts `script` in the peek. */
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
  // Closed meanwhile, or a newer peek was asked for: this tool is not wanted.
  if (mine !== seq || open !== p) {
    invoke("kill_session", { session: info.id }).catch(console.error);
    return;
  }
  // A skiffd from before the peek ran it as a plain session, with a shell after it.
  if (!info.peek) {
    invoke("kill_session", { session: info.id }).catch(console.error);
    closePeek();
    return showError("This skiffd is older than the app. Restart skiffd to run tools in the peek.");
  }
  p.session = info.id;
  p.el.classList.add("busy");
  const n = ++stream;
  const channel = new Channel<unknown>();
  channel.onmessage = (m) => {
    if (open !== p) return;
    const bytes = toBytes(m);
    p.term.write(bytes, () => invoke("ack_output", { session: info.id, stream: n, bytes: bytes.length }).catch(console.error));
  };
  await invoke("subscribe_output", { session: info.id, stream: n, onOutput: channel }).catch(showError);
  if (early.has(info.id)) {
    const code = early.get(info.id) ?? null;
    early.delete(info.id);
    peekExited(info.id, code);
  }
}

const sessionsHas = (id: string) => sessions.has(id);

/** skiffd says a peek tool ended. True when the session was the peek's. */
export function peekExited(id: string, code: number | null): boolean {
  const p = open;
  if (!p) return false;
  // A start is in flight: the exit may be its tool's, ahead of the reply.
  if (!p.session && !sessionsHas(id)) {
    early.set(id, code);
    return true;
  }
  if (p.session !== id) return false;
  p.exited = true;
  p.el.classList.remove("busy");
  // The last output can still be on its way: it comes on another channel.
  window.setTimeout(() => {
    if (open !== p) return;
    if (p.usedAlt && code === 0) return closePeek();
    p.term.write(`\r\n\x1b[2m${code ? `${p.program} exited with code ${code}. ` : ""}Press any key to close.\x1b[0m`);
  }, 150);
  return true;
}

/** Shows the diff of `file` in worktree `wt`, or of the whole worktree, as Settings › Open with says. */
export function showDiff(wt: string, branch: string, file?: string) {
  void invoke<Opened>("open_diff", { path: wt, file: file ?? null })
    .then((o) => handle(o, "diff", file ?? "All changes", branch))
    .catch(showError);
}

/** Opens a file as Settings › Open with says: in the peek, in an app, or in the system's default app. */
export function openFile(path: string) {
  const name = path.split("/").pop() ?? path;
  const dir = path.slice(0, path.length - name.length - 1);
  void invoke<Opened>("open_file", { path })
    .then((o) => handle(o, "file", name, dir))
    .catch(showError);
}

async function handle(o: Opened, kind: string, title: string, where: string) {
  if (o.kind === "peek") {
    if (await mayEnd()) void run(kind, title, where, o.script, o.cwd);
    return;
  }
  if (o.kind === "no_app") {
    showError(`No app opens ${title}. Set one in Settings › Open with.`, { label: "Find a tool", run: () => openUrl(TOOLS_URL) });
  }
}

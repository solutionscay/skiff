import { Channel, invoke } from "@tauri-apps/api/core";

import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";

import { park } from "./terminalHost";

import { toBytes } from "../platform/ipcBytes";

import { appKey } from "../app/keyboard";
import { traceKey, traceOutput, traceRender, traceSend } from "../diagnostics/latency";

import { FONT_DEFAULT, S, sessions } from "../app/state";
import { shownIds } from "../app/stateQueries";
import { opening, type Pane, panes } from "./terminalState";
import * as waterline from "./waterline";

export const TERM_THEME = {
  background: "#0b0e12",
  foreground: "#e6e8eb",
  cursor: "#e6e8eb",
  selectionBackground: "#2f3742",
  black: "#0f1216",
  red: "#ff8a80",
  green: "#7ee0cb",
  yellow: "#ffb454",
  blue: "#6b9cff",
  magenta: "#b69cff",
  cyan: "#7ee0cb",
  white: "#c3c9d1",
  brightBlack: "#7d8794",
  brightRed: "#ffb3b3",
  brightGreen: "#a4f0e0",
  brightYellow: "#ffcf8a",
  brightBlue: "#9ec1ff",
  brightMagenta: "#d0c2ff",
  brightCyan: "#a4f0e0",
  brightWhite: "#ffffff",
};

export function openPane(id: string): Promise<Pane> {
  const existing = panes.get(id);
  if (existing) return Promise.resolve(existing);
  let pending = opening.get(id);
  if (!pending) {
    pending = createPane(id).finally(() => opening.delete(id));
    opening.set(id, pending);
  }
  return pending;
}

export const FONT = "JetBrains Mono, ui-monospace, monospace";

/**
 * xterm measures a cell once, when the terminal opens. A font that is not
 * loaded yet gives the fallback's width, and the fit leaves an empty margin
 * until a reload. So every terminal waits for the bundled font.
 */
const fontReady = Promise.all(
  ["400", "700"].map((w) => document.fonts.load(`${w} ${FONT_DEFAULT}px "JetBrains Mono"`)),
).catch(() => undefined);

async function createPane(id: string): Promise<Pane> {
  await fontReady;
  const el = document.createElement("div");
  el.className = "pane";
  park.appendChild(el);

  const info = sessions.get(id);
  const term = new Terminal({
    cols: info?.cols ?? 80,
    rows: info?.rows ?? 24,
    theme: TERM_THEME,
    fontFamily: FONT,
    fontSize: S.fontSize,
    lineHeight: 1.2,
    cursorBlink: true,
    // The daemon's snapshot holds 2000 lines. A pane shown again after it
    // was parked keeps that much, so every pane keeps that much.
    scrollback: 2000,
    allowProposedApi: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const search = new SearchAddon();
  term.loadAddon(search);
  const box = document.createElement("div");
  box.className = "term-box";
  el.appendChild(box);
  term.open(box);

  // Returning false keeps the key out of the PTY. The window listener acts on it.
  term.attachCustomKeyEventHandler((e) => {
    traceKey(id, e);
    // Ctrl+Backspace deletes a word, as in VS Code: ^W works in bash, zsh,
    // fish, vim and the agents. xterm.js would send ^H, one character.
    if (e.key === "Backspace" && e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      if (e.type === "keydown") term.input("\x17");
      e.preventDefault();
      return false;
    }
    return appKey(e) === null;
  });

  term.onData((data) => {
    const sent = traceSend(id);
    invoke("pty_write", { session: id, data }).then(sent, console.error);
  });
  term.onRender(() => {
    traceRender(id);
    waterline.update(id);
  });
  term.onScroll(() => waterline.update(id));
  // A key the terminal takes: the user reads from here now. App keys never get here.
  term.onKey(() => waterline.clear(id));

  const pane: Pane = { el, term, fit, search, parked: false, stream: 0, wrote: 0, unacked: 0, acking: false, sub: Promise.resolve() };
  const drop = () => {
    term.dispose();
    el.remove();
  };
  try {
    await streamOutput(id, pane);
  } catch (e) {
    drop();
    throw e;
  }
  // session_removed may have arrived during the await.
  if (!sessions.has(id)) {
    drop();
    throw new Error(`session ${id} is gone`);
  }

  panes.set(id, pane);
  return pane;
}

/** Subscribes the pane to its session: a snapshot, then live output. */
function streamOutput(id: string, pane: Pane): Promise<void> {
  const n = ++pane.stream;
  const channel = new Channel<unknown>();
  channel.onmessage = (m) => {
    // A chunk can still arrive after a park, or after session_removed
    // disposed the terminal.
    const s = sessions.get(id);
    if (!s || pane.stream !== n) return;
    const bytes = toBytes(m);
    const traced = traceOutput(id, bytes.length);
    pane.term.write(bytes, () => {
      traced?.();
      if (pane.stream !== n) return;
      ackOutput(id, pane, n, bytes.length);
      pane.wrote = n;
      waterline.ready(id);
    });
    s.last_output_at = Date.now();
  };
  pane.unacked = 0;
  return invoke("subscribe_output", { session: id, stream: n, onOutput: channel });
}

/**
 * Tells Rust that xterm parsed `bytes`, so it keeps sending. One call is in
 * flight at a time; acks that arrive meanwhile add up for the next one.
 */
function ackOutput(id: string, pane: Pane, stream: number, bytes: number) {
  pane.unacked += bytes;
  if (pane.acking) return;
  pane.acking = true;
  const sent = pane.unacked;
  pane.unacked = 0;
  invoke("ack_output", { session: id, stream, bytes: sent })
    .catch(console.error)
    .finally(() => {
      pane.acking = false;
      // What is left belongs to the current stream: a new one resets it.
      if (pane.unacked) ackOutput(id, pane, pane.stream, 0);
    });
}

/** A pane out of the layout stops its stream; showing it again redraws it from a snapshot. */
export function parkPane(id: string, pane: Pane) {
  park.appendChild(pane.el);
  if (pane.parked) return;
  pane.parked = true;
  pane.stream++;
  pane.sub = pane.sub.then(() => invoke<void>("unsubscribe_output", { session: id })).catch(console.error);
}

export function unparkPane(id: string, pane: Pane) {
  if (!pane.parked) return;
  pane.parked = false;
  pane.sub = pane.sub.then(() => streamOutput(id, pane)).catch(console.error);
}

/**
 * WebGL contexts are few (WebKit drops the oldest past 16) and slow to make.
 * The panes shown most recently keep theirs, so switching back to a group
 * is cheap; older parked panes fall back to the DOM renderer, which draws
 * nothing while hidden.
 */
const GL_KEEP = 8;
/** Pane ids with a WebGL renderer, least recently shown first. */
const glOrder: string[] = [];

export function useWebgl(id: string, pane: Pane) {
  const i = glOrder.indexOf(id);
  if (i >= 0) glOrder.splice(i, 1);
  glOrder.push(id);
  if (!pane.webgl) {
    try {
      // WebKitGTK without DMA-BUF shows the WebGL canvas one frame late unless the
      // drawing buffer is kept. Echo drops from ~600 ms to ~30 ms.
      const gl = new WebglAddon(true);
      gl.onContextLoss(() => {
        gl.dispose();
        if (pane.webgl !== gl) return;
        pane.webgl = undefined;
        // A shown pane gets a new context; a parked one when it shows again.
        if (panes.get(id) === pane && shownIds().includes(id)) setTimeout(() => useWebgl(id, pane), 100);
      });
      pane.term.loadAddon(gl);
      pane.webgl = gl;
    } catch (e) {
      console.warn("WebGL renderer unavailable, using DOM renderer", e);
    }
  }
  const shown = shownIds();
  for (let j = 0; glOrder.length > GL_KEEP && j < glOrder.length; ) {
    const old = glOrder[j];
    const p = panes.get(old);
    if (p && shown.includes(old)) {
      j++;
      continue;
    }
    glOrder.splice(j, 1);
    p?.webgl?.dispose();
    if (p) p.webgl = undefined;
  }
}

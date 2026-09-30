/** xterm panes: open, fit, font size, clipboard, find, pane headers. */
import { agentIcon, agentKind } from "./agentIcon";
import { isSlot, slotBody, slotHead } from "./canvas";
import { createLayoutView, sessionsOf } from "./layout";
import { taskTitle } from "./model";
import { stateIcon } from "./stateIcon";
import { Channel, invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { button, h, host, icon, park, showError, toBytes } from "./dom";
import { saveGroup } from "./groups";
import { appKey } from "./keyboard";
import { traceKey, traceOutput, traceRender, traceSend } from "./latency";
import { ctxMenu, paneMenu } from "./menus";
import { accent, activeGroupObj, FONT_DEFAULT, opening, type Pane, panes, place, S, sessions, shownIds, viewLayout } from "./state";
import { closePane, dragSessions, focusPane } from "./view";

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

function openPane(id: string): Promise<Pane> {
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
  term.onRender(() => traceRender(id));

  const pane: Pane = { el, term, fit, search, parked: false, stream: 0, unacked: 0, acking: false, sub: Promise.resolve() };
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
      if (pane.stream === n) ackOutput(id, pane, n, bytes.length);
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
function parkPane(id: string, pane: Pane) {
  park.appendChild(pane.el);
  if (pane.parked) return;
  pane.parked = true;
  pane.stream++;
  pane.sub = pane.sub.then(() => invoke<void>("unsubscribe_output", { session: id })).catch(console.error);
}

function unparkPane(id: string, pane: Pane) {
  if (!pane.parked) return;
  pane.parked = false;
  pane.sub = pane.sub.then(() => streamOutput(id, pane)).catch(console.error);
}

export const view = createLayoutView(host, {
  attach: (id, body) => {
    if (isSlot(id)) return slotBody(id, body);
    const pane = panes.get(id);
    if (pane) {
      unparkPane(id, pane);
      body.appendChild(pane.el);
      return;
    }
    openPane(id)
      .then((p) => {
        if (!body.isConnected) return;
        body.appendChild(p.el);
        useWebgl(id, p);
        fitShown();
        const grab = S.grab === id;
        if (grab) S.grab = null;
        if (id === S.focused && (canTakeFocus() || (grab && !ctxMenu.isOpen))) p.term.focus();
      })
      .catch(console.error);
  },
  head: paneHead,
  focus: (id) => {
    if (!isSlot(id) && id !== S.focused) focusPane(id);
  },
  menu: (id, e) => paneMenu(id, e.clientX, e.clientY),
  resized: () => fitSoon(),
  committed: () => {
    const g = activeGroupObj();
    if (g) saveGroup(g);
  },
});

/**
 * WebGL contexts are few (WebKit drops the oldest past 16) and slow to make.
 * The panes shown most recently keep theirs, so switching back to a group
 * is cheap; older parked panes fall back to the DOM renderer, which draws
 * nothing while hidden.
 */
const GL_KEEP = 8;
/** Pane ids with a WebGL renderer, least recently shown first. */
const glOrder: string[] = [];

function useWebgl(id: string, pane: Pane) {
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

/** Draws the view and parks every terminal it does not show. */
export function renderLayout() {
  const layout = viewLayout();
  const changed = view.sync(layout);
  const shown = new Set(sessionsOf(layout));
  for (const [id, p] of panes) if (!shown.has(id) && (!p.parked || p.el.parentElement !== park)) parkPane(id, p);
  if (changed === "resized") fitShown();
  if (changed === "rebuilt") {
    for (const id of shown) {
      const p = panes.get(id);
      if (p) useWebgl(id, p);
    }
    requestAnimationFrame(() => {
      fitShown();
      for (const id of shown) {
        const p = panes.get(id);
        if (p) p.term.refresh(0, p.term.rows - 1);
      }
      if (S.focused && canTakeFocus()) panes.get(S.focused)?.term.focus();
    });
  }
}

/** True when no menu, dialog or input holds the keys, so a terminal may take them. */
function canTakeFocus(): boolean {
  const a = document.activeElement;
  // An empty pane of a split canvas keeps the keys while it is being filled.
  return !ctxMenu.isOpen && (!a || a === document.body || (host.contains(a) && !a.closest(".slot")));
}

let resizeTimer: number | undefined;
/** Panes fitted since the last pty_resize. A later fit adds to them: it must not drop an earlier pane's new size. */
const resizePending = new Set<string>();

/** Fit every visible terminal now; tell the daemon the new sizes once the drag settles. */
function fitShown() {
  const shown = [...panes].filter(([, p]) => p.el.parentElement !== park && p.el.isConnected);
  for (const [id, p] of shown) {
    p.fit.fit();
    resizePending.add(id);
  }
  clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(() => {
    for (const id of resizePending) {
      const p = panes.get(id);
      if (p) invoke("pty_resize", { session: id, cols: p.term.cols, rows: p.term.rows }).catch(console.error);
    }
    resizePending.clear();
  }, 50);
}

let fitFrame = 0;

/** fitShown once per frame: a window or divider drag fires many resizes. */
function fitSoon() {
  if (fitFrame) return;
  fitFrame = requestAnimationFrame(() => {
    fitFrame = 0;
    fitShown();
  });
}

new ResizeObserver(() => fitSoon()).observe(host);

/** One pane header: the agent icon, the title, then the close button. */
function paneHead(id: string, head: HTMLElement, cell: HTMLElement) {
  if (isSlot(id)) return slotHead(id, head);
  if (!head.dataset.drag) {
    // Drag a pane by its header to move it within the layout.
    head.dataset.drag = "1";
    head.addEventListener("mousedown", (e) => {
      if (!(e.target as Element).closest("button")) dragSessions(e, [id]);
    });
  }
  const s = sessions.get(id);
  const at = s ? place(s) : null;
  cell.style.setProperty("--pc", accent(at?.project));
  cell.classList.toggle("focused", id === S.focused);
  const multi = shownIds().length > 1;
  // Rebuild only on change, so a click that spans a daemon event still lands.
  const sig = s ? JSON.stringify([s.state, s.exit_code, s.unread, at?.project.name, taskTitle(s), agentKind(s), multi]) : "";
  if (head.dataset.sig === sig) return;
  head.dataset.sig = sig;
  head.replaceChildren();
  if (!s) return;
  head.classList.toggle("st-waiting", s.state === "waiting");
  head.classList.toggle("st-done", s.state === "done");
  head.append(agentIcon(s), h("span", "title", taskTitle(s)), stateIcon(s));
  const x = button("head-btn", "", () => closePane(id));
  const label = multi ? "Remove from group. The session keeps running." : "Close. The session keeps running.";
  x.title = label;
  x.setAttribute("aria-label", label);
  x.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
  head.appendChild(x);
}

// The right button belongs to Skiff inside the terminal area. xterm.js and
// the program behind it (which may paste on right-click) never see it.
for (const type of ["mousedown", "mouseup", "auxclick"] as const) {
  host.addEventListener(type, (e) => {
    if (e.button === 2) e.stopPropagation();
  }, true);
}

host.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  e.stopPropagation();
  const id = (e.target as HTMLElement).closest<HTMLElement>(".cell")?.dataset.session;
  if (id) paneMenu(id, e.clientX, e.clientY);
}, true);

let saveFontSize: ReturnType<typeof setTimeout> | undefined;

/**
 * The daemon's `[appearance]` keeps the size across machines and cleared
 * caches. localStorage is the fallback for a daemon older than the setting.
 * `save: false` applies a size the daemon already has.
 */
export function setFontSize(n: number, save = true) {
  S.fontSize = Math.min(28, Math.max(8, n));
  try {
    localStorage.setItem("skiff.fontSize", String(S.fontSize));
  } catch {
    /* a private window keeps the size for this run only */
  }
  for (const p of panes.values()) p.term.options.fontSize = S.fontSize;
  applyZoom();
  fitShown();
  if (!save) return;
  showZoom();
  // A held key steps the size many times; the config file gets the last one.
  clearTimeout(saveFontSize);
  saveFontSize = setTimeout(() => invoke("set_font_size", { size: S.fontSize }).catch(console.error), 300);
}

let zoomTimer: ReturnType<typeof setTimeout> | undefined;

/** Ctrl+Plus/Minus: the size as a percent of the default, briefly, over the terminals. */
function showZoom() {
  const badge = host.querySelector<HTMLElement>(".zoom-badge") ?? host.appendChild(h("div", "zoom-badge"));
  badge.setAttribute("role", "status");
  badge.textContent = `${Math.round((S.fontSize / FONT_DEFAULT) * 100)}%`;
  clearTimeout(zoomTimer);
  zoomTimer = setTimeout(() => badge.remove(), 1200);
}

/** The app text follows the terminal size: 13 is 1x. */
function applyZoom() {
  document.documentElement.style.setProperty("--zoom", String(S.fontSize / FONT_DEFAULT));
}
applyZoom();

/** The middle of the focused pane, where key-opened menus appear. */
export function paneCenter(): [number, number] {
  const r = S.focused ? view.cellRects().get(S.focused) : undefined;
  return r ? [r.left + r.width / 2 - 140, r.top + 60] : [window.innerWidth / 2 - 140, 120];
}

let copyNoticeTimer: number | undefined;

function showCopyConfirmation(id: string) {
  document.querySelectorAll(".copy-notice").forEach((el) => el.remove());
  const head = [...document.querySelectorAll<HTMLElement>(".cell-head")]
    .find((el) => el.parentElement?.dataset.session === id);
  if (!head) return;
  const notice = h("span", "copy-notice", "Copied");
  notice.setAttribute("role", "status");
  notice.setAttribute("aria-live", "polite");
  head.querySelector(".head-btn")?.before(notice);
  if (copyNoticeTimer) clearTimeout(copyNoticeTimer);
  copyNoticeTimer = window.setTimeout(() => notice.remove(), 1600);
}

export async function copySelection() {
  const id = S.focused;
  const text = id ? panes.get(id)?.term.getSelection() : "";
  if (!text) return;
  try {
    await writeText(text);
    showCopyConfirmation(id!);
  } catch (e) {
    showError(e);
  }
}

export async function pasteClipboard() {
  const pane = S.focused ? panes.get(S.focused) : undefined;
  if (!pane) return;
  const text = await readText().catch(() => "");
  if (text) pane.term.paste(text);
}

let findBar: HTMLElement | null = null;

export function openFind() {
  const pane = S.focused ? panes.get(S.focused) : undefined;
  if (!pane) return;
  findBar?.remove();
  const bar = h("div", "find-bar");
  const input = h("input", "find-input");
  input.placeholder = "Find";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Find in terminal");
  const opts = { decorations: { matchOverviewRuler: "#ffb454", activeMatchColorOverviewRuler: "#ffb454", activeMatchBackground: "#ffb45466", matchBackground: "#ffb45433" } };
  const next = () => pane.search.findNext(input.value, opts);
  const prev = () => pane.search.findPrevious(input.value, opts);
  const close = () => {
    pane.search.clearDecorations();
    bar.remove();
    findBar = null;
    pane.term.focus();
  };
  input.addEventListener("input", next);
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") (e.shiftKey ? prev : next)();
    else if (e.key === "Escape") close();
    else return;
    e.preventDefault();
  });
  bar.append(input, button("find-btn", "↑", prev), button("find-btn", "↓", next), button("find-btn", "×", close));
  host.appendChild(bar);
  findBar = bar;
  input.focus();
}

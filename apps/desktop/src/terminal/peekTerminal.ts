/**
 * The peek's command terminal: the record of one command, and pure checks on
 * what its terminal holds. It owns no controller state. peek.ts decides what
 * to do with the answers.
 */
import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITheme } from "@xterm/xterm";
import { keyLabel } from "../app/keys";
import { $, h } from "../ui/dom";
import { t } from "../i18n";
import { peekHead } from "./peekCard";

export interface Peek {
  /** The preview this command belongs to. */
  gen: number;
  kind: string;
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
  /** Output came. The terminal may not have parsed it yet. */
  got: boolean;
  timer: number;
}

/** What `newPeek` needs. The facade passes the theme, font and size, so this file reads no app state. */
export interface NewPeek {
  gen: number;
  kind: string;
  request: string;
  title: string;
  where: string;
  program: string;
  close: () => void;
  theme: ITheme;
  font: string;
  fontSize: number;
  /** Runs on the terminal before it opens. */
  prepare: (term: Terminal) => void;
  onResize: (p: Peek) => void;
}

/**
 * Builds the command's element and terminal, laid out but not visible, so the
 * terminal has its real size from the start. The caller adds the key handler
 * and the input handler.
 */
export function newPeek(o: NewPeek): Peek {
  const el = h("div", "peek pending");
  el.setAttribute("role", "dialog");
  el.setAttribute("aria-label", o.title);
  const head = peekHead(o.kind, o.title, o.where, o.close);
  const box = h("div", "peek-body");
  el.append(head, box);
  $("main").appendChild(el);

  const term = new Terminal({
    theme: o.theme,
    fontFamily: o.font,
    fontSize: o.fontSize,
    lineHeight: 1.2,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: true,
  });
  o.prepare(term);
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(box);
  const p: Peek = {
    gen: o.gen, kind: o.kind, request: o.request, el, term, fit, resize: new ResizeObserver(() => o.onResize(p)),
    session: null, program: o.program, shown: false, exited: false, interactive: false, got: false, timer: 0,
  };
  p.resize.observe(box);
  return p;
}

/**
 * The command took the terminal: Esc is the tool's now. The keycap names the
 * close key, which Skiff takes before the tool does. A click still closes.
 */
export function tookTerminal(p: Peek) {
  p.interactive = true;
  const b = p.el.querySelector<HTMLButtonElement>(".peek-esc");
  if (!b) return;
  const key = keyLabel("close-pane");
  b.textContent = key;
  b.title = t("peek.closeKey", { key });
}

/** The program `sh` could not find, from its message: dash, bash and busybox word it apart. */
export function notFound(term: Terminal): string | null {
  const b = term.buffer.active;
  for (let y = 0; y < b.length; y++) {
    const m = b.getLine(y)?.translateToString(true).match(/([^\s:]+): (?:command )?not found/);
    if (m) return m[1];
  }
  return null;
}

/** The terminal holds text: the command printed something. */
export function printed(term: Terminal): boolean {
  const b = term.buffer.active;
  for (let y = 0; y < b.length; y++) if (b.getLine(y)?.translateToString(true).trim()) return true;
  return false;
}

/**
 * What a command's end means for its peek: close it, show it, or name the
 * program `sh` could not find.
 */
export function exitAction(p: Peek, code: number | null): "close" | "show" | { missing: string } {
  // A tool that took the terminal is done with it, and it cleared up after itself.
  // A pager that quit at once because the text fit (less -F under delta) took the
  // terminal for a moment but left the text on screen: that text is the preview.
  if (p.interactive && code === 0 && !printed(p.term)) return "close";
  // Nothing printed, no error: an app took the file and runs on its own.
  if (!p.shown && !code && !p.got && !printed(p.term)) return "close";
  // `sh` could not find the program: name it, and where to find tools.
  if (code === 127) return { missing: notFound(p.term) ?? p.program };
  return "show";
}

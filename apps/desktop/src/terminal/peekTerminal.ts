/**
 * The peek's command terminal: the record of one command, and pure checks on
 * what its terminal holds. It owns no controller state. peek.ts decides what
 * to do with the answers.
 */
import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import { keyLabel } from "../app/keys";
import { t } from "../i18n";

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

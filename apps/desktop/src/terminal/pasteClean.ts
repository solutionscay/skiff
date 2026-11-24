/**
 * Every paste into a terminal goes through here. xterm wraps a paste in the
 * bracketed paste marks ESC[200~ … ESC[201~ but leaves a mark that is already
 * in the text. An end mark in copied text would end the paste early and type
 * the rest as keys, Enter included (#289). So the marks are taken out until
 * none is left, then every other control byte (ESC, C0 and C1, 0x9b CSI
 * included) goes. Tab, LF and CR stay.
 */
import type { Terminal } from "@xterm/xterm";

const MARK = /\x1b\[20[01]~/g;
// C0 but tab, LF and CR; DEL; C1. ESC (0x1b) and CSI (0x9b) are in these ranges.
const CONTROL = /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/g;

/** `text` with no paste mark and no control byte but tab, LF and CR. */
export function cleanPaste(text: string): string {
  let t = text;
  for (let prev = ""; prev !== t; ) {
    prev = t;
    t = t.replace(MARK, "");
  }
  return t.replace(CONTROL, "");
}

/** Pastes `text` into `term`, cleaned. */
export function pasteInto(term: Terminal, text: string) {
  term.paste(cleanPaste(text));
}

/**
 * xterm pastes a browser paste event itself, past `term.paste`. This takes
 * the event first, on `el` around the terminal, and pastes it cleaned.
 */
export function guardPasteEvents(term: Terminal, el: HTMLElement) {
  el.addEventListener("paste", (e) => {
    e.preventDefault();
    e.stopPropagation();
    const text = e.clipboardData?.getData("text/plain");
    if (text) pasteInto(term, text);
  }, true);
}

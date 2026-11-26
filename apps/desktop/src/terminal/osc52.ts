/**
 * OSC 52: a program in the terminal puts text on the clipboard. Claude Code
 * copies its own mouse selection this way, and so do Neovim, tmux and others
 * over SSH. The sequence is `ESC ] 52 ; <targets> ; <base64> BEL`. Every
 * target goes to the clipboard.
 *
 * Writes only. A query (`?`) would hand the clipboard to whatever runs in
 * the pane, so it gets no answer.
 */
import { pageRoute } from "../platform/invoke";
import type { Terminal } from "@xterm/xterm";
import { copyText } from "../platform/clipboard";
import { showError } from "../ui/alerts";
import { clipboardWrites } from "../app/hosts";
import { traceCopy, traceOsc52Refused } from "../diagnostics/clipTrace";

/** The most base64 a program may send: about 6 MB of text. */
const MAX = 8 << 20;

/** `sid`: the session in the terminal, for the clipboard trace. */
export function handleOsc52(term: Terminal, sid: string | null = null) {
  const owner = pageRoute()?.entry;
  term.parser.registerOscHandler(52, (data) => {
    if (pageRoute()?.entry !== owner) return true;
    const payload = data.slice(data.indexOf(";") + 1);
    if (!data.includes(";") || !payload || payload === "?" || payload.length > MAX) return true;
    let text: string;
    try {
      const bin = atob(payload);
      text = new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
    } catch {
      return true;
    }
    // A remote Host's programs write this Mac's clipboard only when allowed.
    void clipboardWrites()
      .then((ok) => {
        if (!ok) return traceOsc52Refused(sid, text);
        const traced = traceCopy(sid, term, text, "osc52");
        return copyText(text, traced?.tr).then(() => traced?.end(), (e) => {
          traced?.end(e);
          throw e;
        });
      })
      .catch(showError);
    return true;
  });
}

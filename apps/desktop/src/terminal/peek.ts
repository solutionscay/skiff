/**
 * The peek: one full-size, read-only view over the terminal area. It is not a
 * session: no sidebar row, no group, no split. Esc or q closes it and the
 * layout under it is as it was. It shows what the `[open] diff` command
 * prints: `git diff` unless the settings name another command.
 */
import { invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { $, button, h } from "../ui/dom";
import { icon } from "../ui/icons";
import { showError } from "../ui/alerts";
import { markCurrent } from "../app/keyboard";
import { S } from "../app/state";

import { panes } from "./terminalState";
import { fitShown } from "./terminal";
import { FONT, TERM_THEME } from "./terminalRuntime";

let open: { el: HTMLElement; term: Terminal; fit: FitAddon; resize: ResizeObserver } | null = null;
/** The last request. A slow diff that answers after a newer one is dropped. */
let seq = 0;

export function closePeek() {
  if (!open) return;
  open.resize.disconnect();
  open.term.dispose();
  open.el.remove();
  open = null;
  // Refit and repaint the live panes after the overlay leaves the screen.
  fitShown();
  for (const p of panes.values()) {
    if (!p.parked && p.el.isConnected) p.term.refresh(0, p.term.rows - 1);
  }
  const back = S.focused ? panes.get(S.focused) : undefined;
  // The keys go back to the pane, and the highlight to its row.
  if (S.focused) S.roveKey = S.focused;
  markCurrent();
  back?.term.focus();
}

function frame(): NonNullable<typeof open> {
  if (open) return open;
  const el = h("div", "peek");
  el.setAttribute("role", "dialog");
  el.setAttribute("aria-label", "Diff");
  const head = h("div", "peek-head");
  // The same close button a pane has.
  const close = button("head-btn", "", closePeek);
  close.title = "Close (Esc)";
  close.setAttribute("aria-label", "Close the diff");
  close.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
  head.append(h("span", "peek-kind", "diff"), h("span", "peek-title mono"), h("span", "peek-where mono"), close);
  const box = h("div", "peek-body");
  el.append(head, box);
  $("main").appendChild(el);

  const term = new Terminal({
    theme: TERM_THEME,
    fontFamily: FONT,
    fontSize: S.fontSize,
    lineHeight: 1.2,
    disableStdin: true,
    cursorBlink: false,
    cursorStyle: "bar",
    cursorInactiveStyle: "none",
    convertEol: true,
    scrollback: 100000,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(box);
  term.attachCustomKeyEventHandler((e) => {
    if (e.type === "keydown" && (e.key === "Escape" || e.key === "q")) {
      e.preventDefault();
      closePeek();
      return false;
    }
    return true;
  });
  const resize = new ResizeObserver(() => fit.fit());
  resize.observe(box);
  open = { el, term, fit, resize };
  return open;
}

/** Shows the diff of `file` in worktree `wt`, or of the whole worktree. A second call swaps the content. */
export function showDiff(wt: string, branch: string, file?: string) {
  void peekDiff(wt, branch, file);
}

/** Shows the diff in the peek. A second call swaps the content. */
async function peekDiff(wt: string, branch: string, file?: string) {
  const mine = ++seq;
  const p = frame();
  p.el.querySelector(".peek-title")!.textContent = file ?? "All changes";
  p.el.querySelector(".peek-where")!.textContent = branch;
  p.fit.fit();
  p.term.reset();
  p.term.write("\x1b[2mReading the diff…\x1b[0m");
  p.term.focus();
  let text: string;
  try {
    text = await invoke<string>("git_diff", { path: wt, file: file ?? null, cols: p.term.cols });
  } catch (e) {
    if (mine === seq) closePeek();
    return showError(e);
  }
  // Closed meanwhile, or a newer diff was asked for.
  if (mine !== seq || open !== p) return;
  p.term.reset();
  p.term.write(text || "\x1b[2mNo differences against HEAD.\x1b[0m", () => p.term.scrollToTop());
}

import { invoke } from "@tauri-apps/api/core";

import { copyText, pasteText } from "../platform/clipboard";

import { h } from "../ui/dom";

import { showError } from "../ui/alerts";

import { S } from "../app/state";
import { isRemote, hostName } from "../app/hosts";

import { panes } from "./terminalState";

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
    await copyText(text);
    showCopyConfirmation(id!);
  } catch (e) {
    showError(e);
  }
}

/** What the backend typed for a drop or a paste of files or an image. */
export interface Typed {
  text: string;
  warning: string | null;
}

/** Types the words as a paste, so an agent attaches a pasted image path. Never presses Enter. */
export function typeFiles(id: string, t: Typed) {
  panes.get(id)?.term.paste(t.text);
  if (t.warning) showError(t.warning);
}

/**
 * Skiff's Paste: text as it is; else a file list as a drop does; else an
 * image, saved to the cache with its path typed. Bare Ctrl+V stays with the
 * terminal, where Claude Code and Codex read the clipboard themselves.
 */
export async function pasteClipboard() {
  const id = S.focused;
  const pane = id ? panes.get(id) : undefined;
  if (!id || !pane) return;
  const text = await pasteText().catch(() => "");
  if (text) return pane.term.paste(text);
  try {
    const t = await invoke<Typed | null>("clipboard_image", { session: id, host: isRemote() ? hostName() : null });
    if (t) typeFiles(id, t);
  } catch (e) {
    showError(e);
  }
}

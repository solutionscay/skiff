import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

import { h } from "./dom";

import { showError } from "./alerts";

import { S } from "./state";

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

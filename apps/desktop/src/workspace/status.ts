import { isUnread } from "./model";

import { $ } from "../ui/dom";

import { sessions } from "../app/state";

import { renderWelcome } from "./welcome";
export function renderCounts() {
  const all = [...sessions.values()];
  const waiting = all.filter((s) => s.state === "waiting").length;
  const working = all.filter((s) => s.state === "working").length;
  const unread = all.filter(isUnread).length;
  $("counts").textContent = `${all.length} sessions · ${waiting} waiting · ${unread} unread · ${working} working`;
  $("next-waiting").hidden = waiting === 0;
  $("waiting-count").textContent = String(waiting);
  $("next-unread").hidden = unread === 0;
  $("unread-count").textContent = String(unread);

  renderWelcome();
}

/**
 * A narrow window drops status items instead of wrapping them: first the
 * socket and pid, then the counts, then the memory. The skiffd version stays.
 */
const DROP = ["socket", "counts", "memory"];

function fitStatusbar() {
  const bar = $("statusbar");
  const items = DROP.map((id) => $(id));
  for (const el of items) el.hidden = false;
  for (const el of items) {
    if (bar.scrollWidth <= bar.clientWidth) break;
    el.hidden = true;
  }
}

export function watchStatusbar() {
  const bar = $("statusbar");
  let frame = 0;
  const soon = () => {
    if (!frame) frame = requestAnimationFrame(() => {
      frame = 0;
      fitStatusbar();
    });
  };
  new ResizeObserver(soon).observe(bar);
  // Text changes (counts, memory, a new pid) can make the items wider.
  new MutationObserver(soon).observe(bar, { childList: true, subtree: true, characterData: true });
}

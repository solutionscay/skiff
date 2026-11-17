import { isUnread } from "./model";

import { $, h, setHidden, setText } from "../ui/dom";
import { locale } from "../i18n";
import { ring } from "../ui/ring";
import { type AgentUsage, cellOf, level, productName, resetClock } from "../app/usageModel";

import { sessions } from "../app/state";

import { renderWelcome } from "./welcome";
export function renderCounts() {
  const all = [...sessions.values()];
  const waiting = all.filter((s) => s.state === "waiting").length;
  const working = all.filter((s) => s.state === "working").length;
  const unread = all.filter(isUnread).length;
  setText($("counts"), `${all.length} sessions · ${waiting} waiting · ${unread} unread · ${working} working`);
  setHidden($("next-waiting"), waiting === 0);
  setText($("waiting-count"), String(waiting));
  setHidden($("next-unread"), unread === 0);
  setText($("unread-count"), String(unread));

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

/**
 * The top bar's usage cell (#276), after Waiting and Unread. It exists only
 * while a window of an agent on the shown Host is at or above its threshold:
 * at rest it is not in the DOM. It is not a wait: it sets no state and rings
 * nothing. A click opens Settings › Usage, as the Usage key does.
 */
export function renderUsage(list: AgentUsage[], open: () => void) {
  const cell = cellOf(list, Date.now(), locale());
  let el = document.getElementById("usage-cell");
  if (!cell) return el?.remove();
  if (!el) {
    el = h("button", "tb-cell nx nx-usage");
    el.id = "usage-cell";
    (el as HTMLButtonElement).type = "button";
    el.addEventListener("click", open);
    $("next-unread").after(el);
  }
  const lvl = level(cell.w);
  const num = h("span", `num ${lvl}`, `${cell.w.used}%`);
  if (lvl === "limit" && cell.w.resets_at !== undefined) num.textContent += ` · ${resetClock(cell.w.resets_at, locale())}`;
  const ringEl = h("span", "ring");
  ringEl.append(ring(cell.w.used, lvl));
  el.replaceChildren(ringEl, h("span", "label", productName(cell.agent)), num, h("span", "more", cell.more ? `+${cell.more}` : ""));
  el.title = cell.tip;
  el.setAttribute("aria-label", cell.tip);
}

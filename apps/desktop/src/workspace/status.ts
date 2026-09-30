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
  const nw = $<HTMLButtonElement>("next-waiting");
  nw.hidden = waiting + unread === 0;
  $("next-waiting-label").textContent = waiting
    ? `Next waiting (${waiting})`
    : `Next unread (${unread})`;

  renderWelcome();
}

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

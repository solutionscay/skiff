/** The app icon's number (#275): sessions that wait, the same count as the
 *  top bar's Waiting cell. The rule is `BadgeCount` in `waits.ts`. macOS
 *  shows it on the Dock tile (`setBadgeCount`). On Linux, Rust sends the
 *  Unity launcher signal that docks read (`set_waiting_badge`). */

import { invoke } from "../platform/invoke";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isMac } from "./keys";
import { BadgeCount } from "./waits";

const badge = new BadgeCount();
const isLinux = !isMac && /Linux/.test(navigator.userAgent);
let timer: ReturnType<typeof setTimeout> | undefined;

function apply() {
  clearTimeout(timer);
  timer = undefined;
  const { send, wait } = badge.step(Date.now());
  if (wait !== null) timer = setTimeout(apply, wait);
  if (send === null) return;
  if (isMac) getCurrentWindow().setBadgeCount(send || undefined).catch(console.error);
  else if (isLinux) invoke("set_waiting_badge", { count: send }).catch(console.error);
}

/** The top bar's Waiting count, on each render. */
export function waitingCount(n: number) {
  badge.count(n, Date.now());
  apply();
}

/** The session list was read from a live Host: the icon follows it. */
export function listRead(n: number) {
  badge.link(true, n, Date.now());
  apply();
}

/** The window lost its Host: no number. */
export function linkLost() {
  badge.link(false);
  apply();
}

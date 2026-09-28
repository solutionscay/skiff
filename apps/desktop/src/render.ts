/** render() and the throttled render for daemon events. */
import { syncMenu } from "./commands";
import { host } from "./dom";
import { applyTabOrder } from "./keyboard";
import { renderSelectBar } from "./selection";
import { renderCounts, renderHeader, renderRail, renderSidebar } from "./sidebar";
import { S } from "./state";
import { renderLayout } from "./terminal";
import { applyApp, applyThemes } from "./themes";

/**
 * Daemon events render at most every RENDER_MS, and never while a mouse
 * button is down: a row rebuilt between press and release loses the click.
 */
const RENDER_MS = 80;

let renderTimer: number | undefined;
let renderQueued = false;
let pointerDown = false;

export function scheduleRender() {
  renderQueued = true;
  if (pointerDown || renderTimer !== undefined) return;
  renderTimer = window.setTimeout(() => {
    renderTimer = undefined;
    if (renderQueued && !pointerDown) render();
  }, RENDER_MS);
}

function releasePointer() {
  if (!pointerDown) return;
  pointerDown = false;
  // After the click this release produces has run on the old rows.
  if (renderQueued) window.setTimeout(scheduleRender, 0);
}

window.addEventListener("pointerdown", () => (pointerDown = true), true);

window.addEventListener("pointerup", releasePointer, true);

window.addEventListener("pointercancel", releasePointer, true);

window.addEventListener("blur", releasePointer);

// A release outside the window sends no pointerup.
window.addEventListener("pointermove", (e) => {
  if (pointerDown && e.buttons === 0) releasePointer();
}, true);

export function render() {
  renderQueued = false;
  host.classList.toggle("group-picked", !!S.groupPicked && S.groupPicked === S.activeGroup);
  renderRail();
  renderSidebar();
  renderSelectBar();
  renderLayout();
  applyThemes();
  applyApp();
  renderHeader();
  renderCounts();
  applyTabOrder();
  syncMenu();
}

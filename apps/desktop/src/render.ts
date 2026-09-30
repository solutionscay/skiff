/** render() and the throttled render for daemon events. */
import { syncMenu } from "./commands";
import { host } from "./terminalHost";
import { applyTabOrder } from "./keyboard";
import { renderModes, settleModes } from "./modes";
import { renderSelectBar } from "./selection";
import { renderSidebar } from "./sidebar";
import { renderRail } from "./rail";
import { renderHeader } from "./header";
import { renderCounts } from "./status";
import { S } from "./state";
import { savePlace } from "./stored";
import { renderLayout } from "./terminal";
import { applyBackdrop } from "./backdrop";
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
  settleModes();
  // App colors first: the sidebar's marks are inked against them.
  applyApp();
  host.classList.toggle("group-picked", !!S.groupPicked && S.groupPicked === S.activeGroup);
  renderRail();
  renderSidebar();
  renderSelectBar();
  renderLayout();
  renderModes();
  applyThemes();
  applyBackdrop();
  renderHeader();
  renderCounts();
  applyTabOrder();
  syncMenu();
  savePlace({
    project: S.selectedProject,
    session: S.focused,
    group: S.activeGroup,
    picked: !!S.groupPicked && S.groupPicked === S.activeGroup,
  });
}

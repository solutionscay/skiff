import { openPane, parkPane, unparkPane, useWebgl } from "./terminalRuntime";
import { paneHead } from "./paneHeader";
import { applyZoom } from "./terminalFont";
/** Pane layout, terminal fitting, and terminal-area listeners. */

import { slotBody } from "../workspace/canvas";
import { isSlot } from "../workspace/layoutSlots";
import { sessionsOf } from "../workspace/layout";
import { createLayoutView } from "../workspace/layoutView";

import { invoke } from "@tauri-apps/api/core";

import { modalOpen } from "../ui/dom";
import { host, park } from "./terminalHost";

import { saveGroup } from "../app/groups";

import { paneMenu } from "../workspace/menus";

import { S } from "../app/state";
import { keysOffPanes } from "../app/seen";
import { activeGroupObj, viewLayout } from "../app/stateQueries";
import { panes } from "./terminalState";
import { arrive } from "./waterline";
import { focusPane } from "./paneActions";
import { previewOnScreen } from "./peek";

export const view = createLayoutView(host, {
  attach: (id, body) => {
    if (isSlot(id)) return slotBody(id, body);
    const pane = panes.get(id);
    if (pane) {
      unparkPane(id, pane);
      body.appendChild(pane.el);
      return;
    }
    openPane(id)
      .then((p) => {
        if (!body.isConnected) return;
        body.appendChild(p.el);
        useWebgl(id, p);
        fitShown();
        const grab = S.grab === id;
        if (grab) S.grab = null;
        if (id === S.focused && (canTakeFocus() || (grab && !modalOpen() && !previewOnScreen()))) p.term.focus();
        // A pane on screen shows where the user stopped reading, keys or not.
        arrive(id);
      })
      .catch(console.error);
  },
  head: paneHead,
  focus: (id) => {
    // The focused pane of a picked group is not selected yet: a click selects it.
    if (!isSlot(id) && (id !== S.focused || keysOffPanes())) focusPane(id);
  },
  menu: (id, e) => paneMenu(id, e.clientX, e.clientY),
  resized: () => fitSoon(),
  committed: () => {
    const g = activeGroupObj();
    if (g) saveGroup(g);
  },
});

/** Draws the view and parks every terminal it does not show. */
export function renderLayout() {
  const layout = viewLayout();
  const hidden = new Set([...panes].filter(([, p]) => p.parked).map(([id]) => id));
  const changed = view.sync(layout);
  const shown = new Set(sessionsOf(layout));
  for (const [id, p] of panes) if (!shown.has(id) && (!p.parked || p.el.parentElement !== park)) parkPane(id, p);
  // The waterline is for what the user can see, not only the pane with the keys.
  // Only a pane that comes back on screen: one that stayed in view was not left.
  for (const id of shown) if (hidden.has(id)) arrive(id);
  if (changed === "resized") fitShown();
  if (changed === "rebuilt") {
    for (const id of shown) {
      const p = panes.get(id);
      if (p) useWebgl(id, p);
    }
    requestAnimationFrame(() => {
      fitShown();
      for (const id of shown) {
        const p = panes.get(id);
        if (p) p.term.refresh(0, p.term.rows - 1);
      }
      if (S.focused && canTakeFocus()) panes.get(S.focused)?.term.focus();
    });
  }
}

/** True when no menu, dialog or input holds the keys, so a terminal may take them. */
function canTakeFocus(): boolean {
  const a = document.activeElement;
  // An empty pane of a split canvas keeps the keys while it is being filled.
  // A preview over the panes keeps the keys until the user leaves it.
  return !modalOpen() && !previewOnScreen() && (!a || a === document.body || (host.contains(a) && !a.closest(".slot")));
}

let resizeTimer: number | undefined;
/** Panes fitted since the last pty_resize. A later fit adds to them: it must not drop an earlier pane's new size. */
const resizePending = new Set<string>();

/**
 * Frames left to wait for a terminal that cannot measure yet. A terminal
 * opens in the hidden park, where xterm measures no cell, and it measures
 * only after a frame on screen. A fit before that does nothing, and the
 * pane keeps the size skiffd saved.
 */
let fitTries = 0;
const FIT_TRIES = 10;

/** Fit every visible terminal now; tell the daemon the new sizes once the drag settles. */
export function fitShown() {
  cancelAnimationFrame(fitFrame);
  fitFrame = 0;
  const shown = [...panes].filter(([, p]) => p.el.parentElement !== park && p.el.isConnected);
  let unmeasured = false;
  for (const [id, p] of shown) {
    if (!p.fit.proposeDimensions()) {
      unmeasured = true;
      continue;
    }
    p.fit.fit();
    resizePending.add(id);
  }
  if (unmeasured && fitTries < FIT_TRIES) {
    fitTries++;
    fitSoon();
  } else {
    fitTries = 0;
  }
  clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(() => {
    for (const id of resizePending) {
      const p = panes.get(id);
      if (p) invoke("pty_resize", { session: id, cols: p.term.cols, rows: p.term.rows }).catch(console.error);
    }
    resizePending.clear();
  }, 50);
}

let fitFrame = 0;
/** fitShown once per frame: a window or divider drag fires many resizes. */
function fitSoon() {
  if (fitFrame) return;
  fitFrame = requestAnimationFrame(() => {
    fitFrame = 0;
    fitShown();
  });
}

new ResizeObserver(() => fitSoon()).observe(host);

// The right button belongs to Skiff inside the terminal area. xterm.js and
// the program behind it (which may paste on right-click) never see it.
for (const type of ["mousedown", "mouseup", "auxclick"] as const) {
  host.addEventListener(type, (e) => {
    if (e.button === 2) e.stopPropagation();
  }, true);
}

host.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  e.stopPropagation();
  const id = (e.target as HTMLElement).closest<HTMLElement>(".cell")?.dataset.session;
  if (id) paneMenu(id, e.clientX, e.clientY);
}, true);

/** The middle of the focused pane, where key-opened menus appear. */
export function paneCenter(): [number, number] {
  const r = S.focused ? view.cellRects().get(S.focused) : undefined;
  return r ? [r.left + r.width / 2 - 140, r.top + 60] : [window.innerWidth / 2 - 140, 120];
}

applyZoom();

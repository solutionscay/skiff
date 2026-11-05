/**
 * How mission control (#88) comes and goes: open, close, go to a session,
 * maximize one, and come back from that maximize. missionControl.ts is the board.
 *
 * The board root is opaque and takes the window from the panes in one frame.
 * Only its content fades in, once, on open. Two dense screens never blend, and
 * the panes under it do not paint or fit. A close lets them work first, under
 * the board, and then the board goes at once: what shows is already drawn.
 */

import { S } from "../app/state";
import { maxReturn, toggleMaximize } from "../app/modes";
import { board, missionOpen } from "./missionControl";
import { refocusTerminal, revealSession } from "./view";
import { holdPanes, releasePanes } from "../terminal/terminal";
import { panesDrawn } from "../terminal/terminalRuntime";
import { fade, still } from "../ui/motion";

/** The most frames a close waits under the board for the panes to write their screens. */
const DRAW_FRAMES = 12;

let returnTo: HTMLElement | null = null;
/** Each transition has a number. An async step that finds another number stops. */
let gen = 0;
/** An open waits for the board to read its screens. A second open waits with it. */
let preparing = false;
/** The end of the running transition, and its animations. */
let done: (() => void) | null = null;
let running: Animation[] = [];

/** Ends the running transition at once, in its end state. */
function settle() {
  gen++;
  const end = done;
  done = null;
  for (const a of running.splice(0)) a.cancel();
  board.root.style.removeProperty("pointer-events");
  end?.();
}

/** Starts a transition: the running one ends first. Returns its number. */
function begin(end: () => void): number {
  settle();
  done = end;
  return gen;
}

/** `after`: runs when the board covers the window, at the end of the open. */
async function openMission(after?: () => void) {
  if (preparing) return;
  settle();
  if (missionOpen()) return after?.();
  const my = gen;
  preparing = true;
  returnTo = document.activeElement as HTMLElement | null;
  await board.prepare(S.focused);
  preparing = false;
  if (my !== gen) {
    // A close came first: nothing showed. A restore that waited on this open stays to be pressed again.
    board.clear();
    if (after) maxReturn.to = (restore) => void openMission(restore);
    return;
  }
  const now = begin(() => after?.());
  const root = board.root;
  root.hidden = false;
  root.focus();
  board.start();
  holdPanes();
  if (still()) return settle();
  running = [...root.children].map((el) => el.animate([{ opacity: 0, offset: 0 }], fade()));
  Promise.all(running.map((a) => a.finished)).then(
    () => now === gen && settle(),
    () => {},
  );
}

/** Takes the board away once the panes under it have drawn, or after DRAW_FRAMES. */
function whenDrawn(my: number, left = DRAW_FRAMES) {
  requestAnimationFrame(() => {
    if (my !== gen) return;
    if (left > 0 && !panesDrawn()) return whenDrawn(my, left - 1);
    // One more frame: what the panes wrote paints in this one.
    requestAnimationFrame(() => my === gen && settle());
  });
}

/** `go`: the session to show. `max`: maximize its pane. Without `go`, back to the panes as they were. */
export function closeMission(go?: string, max = false) {
  if (preparing) {
    gen++;
    return;
  }
  settle();
  if (!missionOpen()) return;
  board.stop();
  const root = board.root;
  const my = begin(() => {
    board.clear();
    if (go && (document.activeElement === root || document.activeElement === document.body)) refocusTerminal();
  });
  root.style.pointerEvents = "none";
  // Under the opaque board: the pane shows, it maximizes, and the layout settles. The panes paint here.
  if (go) {
    revealSession(go);
    if (max && !S.maximized) {
      toggleMaximize();
      maxReturn.to = (restore) => void openMission(restore);
    }
    refocusTerminal();
  } else if (returnTo?.isConnected) returnTo.focus();
  else refocusTerminal();
  releasePanes();
  whenDrawn(my);
}

/** Ctrl+Shift+Enter here: the lit session, maximized. */
export function maximizeLit() {
  const lit = board.lit();
  if (lit) closeMission(lit, true);
}

/** To a session: from the board, it closes on that session; else the session shows. */
export function goToSession(id: string | undefined) {
  if (!id) return;
  if (missionOpen()) closeMission(id);
  else revealSession(id);
}

export function toggleMission() {
  if (missionOpen() || preparing) closeMission();
  else void openMission();
}

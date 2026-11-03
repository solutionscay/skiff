/**
 * Mission control's windows: Admiral and each Skipper show as a small
 * terminal, in the pane's look. The pane header (the agent icon, the name,
 * the state mark and what the program reports, as paneHeader.ts builds it),
 * then the body: the face's column, a 1px rule, and the output's column with
 * the last screen lines (output.ts reads them).
 *
 * The faces live in a layer of their own over the strings, so a string ends
 * under a face and the face draws over it. They take the window's place
 * from the layout (missionLayout.ts `faceBox`). A face shows its state with
 * its own motion only: it blinks and turns as it does everywhere; waiting, it
 * holds a turn toward the task it waits on; done, its eyes shut and it stays
 * still. Reduced motion stops the loops.
 */
import type { SessionInfo } from "../../platform/types";
import { agentIcon } from "../../appearance/agentIcon";
import { stateIcon } from "../../appearance/stateIcon";
import { h } from "../../ui/dom";
import { faceSig, skipperFace } from "../skipperFace";
import { statusNote } from "../programStatus";
import * as G from "../missionLayout";

const scopeOf = (s: SessionInfo) => (s.coordinator ? null : (s.skipper ?? ""));

/** A window's two parts: the header to fill, and the output's column. Built once per card. */
export function windowOf(card: HTMLElement, rung: G.Rung): { head: HTMLElement; out: HTMLElement } {
  let head = card.querySelector<HTMLElement>(":scope > .win-head");
  let out = card.querySelector<HTMLElement>(":scope > .win-body > .win-out");
  if (!head || !out || card.dataset.rung !== rung) {
    card.dataset.rung = rung;
    // The card's session markup is gone: a later session card fills it again.
    delete card.dataset.html;
    head = h("span", "win-head");
    out = h("pre", "win-out");
    const col = h("span", "win-face");
    col.style.width = `${G.WINDOW[rung].face + 15}px`;
    const body = h("span", "win-body");
    body.append(col, out);
    card.replaceChildren(head, body);
  }
  return { head, out };
}

/** The header's inside: the number key, the agent icon, the name, the state mark and the program's note. */
export function windowHead(s: SessionInfo, name: string, num: number): HTMLElement[] {
  const parts: HTMLElement[] = [];
  if (num) parts.push(h("span", "num", String(num)));
  parts.push(agentIcon(s), h("span", "title", name), stateIcon(s));
  const note = statusNote(s);
  if (note) parts.push(h("span", "status-note", note));
  return parts;
}

// ---- The faces ----

export const faceLayer = h("div", "mission-faces");
const faces = new Map<string, { el: HTMLElement; sig: string }>();

/**
 * Puts each window's face at its place, in its state. `look`: the side a
 * waiting face turns to, toward the task it waits on. `dim` and `lit` follow
 * the window's own.
 */
export function placeFaces(placed: G.Placed, sess: Map<string, SessionInfo>, look: (id: string) => 1 | -1, cls: (id: string) => string) {
  for (const [id, rung] of placed.rungs) {
    const s = sess.get(id);
    const r = placed.cards.get(id);
    if (!s || !r) continue;
    const px = G.WINDOW[rung].face;
    const sig = JSON.stringify([faceSig(scopeOf(s)), px]);
    let f = faces.get(id);
    if (!f || f.sig !== sig) {
      f?.el.remove();
      const el = h("div", "mission-face");
      el.dataset.id = id;
      el.appendChild(skipperFace(scopeOf(s), px));
      f = { el, sig };
      faces.set(id, f);
      faceLayer.appendChild(el);
    }
    const at = G.faceBox(r, rung);
    f.el.className = `mission-face st-${s.state} ${cls(id)}`;
    f.el.style.left = `${at.x}px`;
    f.el.style.top = `${at.y}px`;
    f.el.style.setProperty("--look", String(look(id)));
  }
  for (const [id, f] of faces) {
    if (placed.rungs.has(id) && sess.has(id)) continue;
    f.el.remove();
    faces.delete(id);
  }
}

/** Only the places, for a drag frame. */
export function moveFaces(placed: G.Placed) {
  for (const [id, f] of faces) {
    const r = placed.cards.get(id);
    const rung = placed.rungs.get(id);
    if (!r || !rung) continue;
    const at = G.faceBox(r, rung);
    f.el.style.left = `${at.x}px`;
    f.el.style.top = `${at.y}px`;
  }
}

export function clearFaces() {
  faceLayer.replaceChildren();
  faces.clear();
}

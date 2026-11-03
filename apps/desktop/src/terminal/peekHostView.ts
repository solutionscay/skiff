/**
 * A Host's read in the peek card: it shows at once, says it loads, then shows
 * the value or why there is none. The controller in peek.ts passes the read,
 * the data connection's epoch and the card hooks; this file holds no state.
 */
import { button, h } from "../ui/dom";
import { t } from "../i18n";
import type { HostFile } from "../platform/types";
import { cardFrame, slot } from "./peekCard";
import { diffState } from "./diffView";
import { fileBody } from "./fileView";

/** What the controller answers for one preview. */
export interface Gate {
  /** The preview is still the one on screen. */
  live(): boolean;
  /** The preview takes the keys when it is ready. */
  keys(): boolean;
  /** The data connection's number now. */
  epoch(): number;
  /** Puts the card up. */
  mount(el: HTMLElement): void;
}

/**
 * A Host's read in the card `el`: it shows at once, says it loads, then
 * shows `render` of what `read` gave, or why there is none. A reply for a
 * row the user left shows nothing; one from an older data connection is
 * asked again.
 */
export function showHostView<T>(gate: Gate, el: HTMLElement, loading: string, read: () => Promise<T>, render: (value: T) => HTMLElement, none: string) {
  if (!gate.live()) return;
  el.append(diffState(loading));
  gate.mount(el);
  const at = gate.epoch();
  const shows = () => gate.live() && slot.el === el;
  const fill = (body: HTMLElement) => {
    const hadKeys = el.contains(document.activeElement);
    el.querySelector(".diff-view-body")?.replaceWith(body);
    if (hadKeys || gate.keys()) body.focus();
  };
  const again = () => showHostView(gate, el, loading, read, render, none);
  read()
    .then((value) => {
      if (!shows()) return;
      if (at !== gate.epoch()) return again();
      fill(render(value));
    })
    .catch((e) => {
      if (!shows()) return console.error(e);
      if (at !== gate.epoch()) return again();
      fill(diffState(t("peek.noneReason", { none, error: String(e) }), true));
    });
}

/** The card frame of a Host file. `copy`: the Open a copy button. */
export function hostFileFrame(title: string, close: () => void, copy: (() => void) | null): HTMLElement {
  const el = cardFrame("file", title, close, "peek diff-view file-view");
  if (copy) {
    const open = button("peek-open", "", copy);
    open.title = t("peek.openCopyTitle");
    open.setAttribute("aria-label", t("menu.openCopy"));
    open.append(h("span", "peek-open-key", "Enter"), " " + t("menu.openCopy"));
    el.querySelector(".peek-esc")?.before(open);
  }
  return el;
}

/** A Host file's body; an image's blob URL goes to the card slot, revoked when the card goes. */
export const renderHostFile = (f: HostFile) => fileBody(f, (b) => (slot.url = URL.createObjectURL(b)));

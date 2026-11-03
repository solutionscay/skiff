/** Focus, caret, hover and scroll anchor of the session tree across renders. */
import { $ } from "../ui/dom";
import { renamingKey } from "./rename";
import { findRow, itemKey, listItems } from "../app/keyboard";
import { keepRow } from "./selection";
import { startPreview } from "../terminal/peek";

/** The first row in view, its key, how far it sits from the list's top, and where the list was scrolled. */
export type Anchor = { el: HTMLElement; key: string; dy: number; scroll: number };

/**
 * The first row in view. WebKit has no scroll anchoring: a row that comes or
 * grows above the view would push the rows the person reads down. At the top
 * there is nothing to keep.
 */
export function anchorOf(side: HTMLElement, items: HTMLElement[]): Anchor | null {
  settleAnchor();
  const scroll = side.scrollTop;
  if (!scroll) return null;
  const top = side.getBoundingClientRect().top;
  const el = items.find((x) => x.getBoundingClientRect().bottom > top);
  return el ? { el, key: itemKey(el), dy: el.getBoundingClientRect().top - top, scroll } : null;
}

/** The anchor to put back at the next frame. */
let due: { side: HTMLElement; anchor: Anchor } | null = null;
let hoverDue = false;
let frame = 0;

/**
 * Puts the anchor row back where it was, at the next frame, before it paints.
 * Read here, just after the render's writes, its position would force a
 * layout of its own; the frame lays the page out once anyway.
 */
export function keepAnchor(side: HTMLElement, anchor: Anchor | null) {
  if (!anchor) return;
  // No read here, not even scrollTop: any read lays the page out now.
  due = { side, anchor };
  settleSoon();
}

function settleSoon() {
  if (!frame) frame = requestAnimationFrame(() => {
    frame = 0;
    settleAnchor();
    if (hoverDue) restoreHover();
  });
}

/** Puts back the anchor still due now: a render measures the list as the person sees it. */
function settleAnchor() {
  const d = due;
  if (!d) return;
  due = null;
  // Something scrolled the list since it was measured, such as a row brought into view: that wins.
  if (d.side.scrollTop !== d.anchor.scroll) return;
  // A kept row is the same node; a rebuilt one has the same key.
  const el = d.anchor.el.isConnected ? d.anchor.el : listItems().find((x) => itemKey(x) === d.anchor.key);
  if (!el) return;
  const moved = el.getBoundingClientRect().top - d.side.getBoundingClientRect().top - d.anchor.dy;
  if (Math.abs(moved) >= 1) d.side.scrollTop += moved;
}

export type Caret = [start: number, end: number, dir: "forward" | "backward" | "none"];

/** Where the pointer last was over the list, or null when it left. */
let pointer: [number, number] | null = null;
const HOVERABLE = ".session-row, .wt-pick, .wt-plus, .wt-count, .group-pick, .file-row";
const HOVERED = HOVERABLE.split(", ").map((x) => `${x}:hover`).join(", ");
{
  const side = $<HTMLElement>("sidebar-scroll");
  side.addEventListener("mousemove", (e) => (pointer = [e.clientX, e.clientY]));
  side.addEventListener("mouseleave", () => (pointer = null));
  side.addEventListener("mouseout", (e) => (e.target as HTMLElement).closest?.(".hover")?.classList.remove("hover"));
}

/**
 * A rebuilt row has no :hover until the pointer moves, so the row under a
 * still pointer flashes on every render. The class stands in until it moves.
 * A kept row keeps its :hover: nothing to find. Runs at the next frame.
 */
function restoreHover() {
  hoverDue = false;
  if (!pointer) return;
  const side = $<HTMLElement>("sidebar-scroll");
  if (side.querySelector(HOVERED)) return;
  document.elementFromPoint(...pointer)?.closest(HOVERABLE)?.classList.add("hover");
}

export function restoreFocus(side: HTMLElement, activeKey: string | undefined, caret: Caret | null, near: string[]) {
  if (pointer) {
    hoverDue = true;
    settleSoon();
  }
  // The row whose rename begins is now its name field, which takes the keys. It
  // has not gone: the rows near it must not become the current one.
  if (activeKey && activeKey === renamingKey()) return;
  // The focused row was kept: it still has the focus and the caret.
  const a = document.activeElement as HTMLElement | null;
  if (activeKey && a && side.contains(a) && (a.dataset.key ?? a.dataset.session ?? a.dataset.wt ?? a.dataset.group) === activeKey) return;
  if (activeKey) {
    const first = [...side.querySelectorAll<HTMLElement>("[data-key], [data-session], [data-wt], [data-group]")]
      .find((el) => (el.dataset.key ?? el.dataset.session ?? el.dataset.wt ?? el.dataset.group) === activeKey);
    // A Skipper in a group has two rows: the keys go back to the one they were on.
    const again = first?.dataset.session ? findRow(listItems(), activeKey) ?? first : first;
    const disabled = again instanceof HTMLButtonElement || again instanceof HTMLInputElement || again instanceof HTMLSelectElement
      ? again.disabled
      : false;
    if (again && !disabled) {
      // The list stays where the person scrolled it, away from the row or not.
      again.focus({ preventScroll: true });
      if (caret !== null && again instanceof HTMLInputElement) again.setSelectionRange(...caret);
    } else if (!again) {
      const keys = new Set(listItems().map(itemKey));
      const next = near.find((k) => keys.has(k));
      if (next) {
        keepRow(next);
        // The row that takes the place of the current one shows its preview, or the panes.
        startPreview(next, false);
      }
    }
  }
}

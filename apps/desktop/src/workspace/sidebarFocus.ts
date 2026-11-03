/** Focus, caret, hover and scroll anchor of the session tree across renders. */
import { $ } from "../ui/dom";
import { renamingKey } from "./rename";
import { findRow, itemKey, listItems } from "../app/keyboard";
import { keepRow } from "./selection";
import { startPreview } from "../terminal/peek";

/**
 * The first row in view and how far it sits from the list's top. WebKit has
 * no scroll anchoring: a row that comes or grows above the view would push
 * the rows the person reads down. At the top there is nothing to keep.
 */
export function anchorOf(side: HTMLElement, items: HTMLElement[]): [key: string, dy: number] | null {
  if (!side.scrollTop) return null;
  const top = side.getBoundingClientRect().top;
  const el = items.find((x) => x.getBoundingClientRect().bottom > top);
  return el ? [itemKey(el), el.getBoundingClientRect().top - top] : null;
}

export function keepAnchor(side: HTMLElement, anchor: [key: string, dy: number] | null) {
  if (!anchor) return;
  const el = listItems().find((x) => itemKey(x) === anchor[0]);
  if (!el) return;
  const moved = el.getBoundingClientRect().top - side.getBoundingClientRect().top - anchor[1];
  if (Math.abs(moved) >= 1) side.scrollTop += moved;
}

export type Caret = [start: number, end: number, dir: "forward" | "backward" | "none"];

/** Where the pointer last was over the list, or null when it left. */
let pointer: [number, number] | null = null;
const HOVERABLE = ".session-row, .wt-pick, .wt-plus, .wt-count, .group-pick, .file-row";
{
  const side = $<HTMLElement>("sidebar-scroll");
  side.addEventListener("mousemove", (e) => (pointer = [e.clientX, e.clientY]));
  side.addEventListener("mouseleave", () => (pointer = null));
  side.addEventListener("mouseout", (e) => (e.target as HTMLElement).closest?.(".hover")?.classList.remove("hover"));
}

/**
 * A rebuilt row has no :hover until the pointer moves, so the row under a
 * still pointer flashes on every render. The class stands in until it moves.
 */
function restoreHover() {
  if (!pointer) return;
  document.elementFromPoint(...pointer)?.closest(HOVERABLE)?.classList.add("hover");
}

export function restoreFocus(side: HTMLElement, activeKey: string | undefined, caret: Caret | null, near: string[]) {
  restoreHover();
  // The row whose rename begins is now its name field, which takes the keys. It
  // has not gone: the rows near it must not become the current one.
  if (activeKey && activeKey === renamingKey()) return;
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

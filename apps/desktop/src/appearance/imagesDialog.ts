/**
 * The Images dialog: the library as a gallery in a centered modal. It shares
 * Settings › Images' grid (`imagesSection` with `pick`). Enter or a click
 * inserts the current image into the pane that had focus when the dialog
 * opened, then closes it. Esc or a click outside closes it.
 */
import { S } from "../app/state";
import { dialogFrame, escapeButton } from "../ui/dialogParts";
import { onImagesChanged } from "../terminal/images";
import { panes } from "../terminal/terminalState";
import { refocusTerminal } from "../workspace/view";
import { imagesSection, imagesState } from "./imagesPage";
import { t } from "../i18n";

let shut: (() => void) | null = null;
/** Draws the open dialog again. */
let redraw: (() => void) | null = null;
onImagesChanged(() => redraw?.());

export function openImages() {
  if (shut) return shut();
  // The pane that had focus at open. It loses Insert when it closes.
  const id = S.focused && panes.has(S.focused) ? S.focused : null;
  const target = () => (id && panes.has(id) ? id : null);
  const st = imagesState();
  st.keys = "field";
  const { overlay, panel } = dialogFrame("images-panel", "dialog", t("images.title"));
  overlay.id = "images-dialog";
  panel.tabIndex = -1;
  overlay.appendChild(panel);

  const close = () => {
    if (!shut) return;
    shut = redraw = null;
    overlay.remove();
    refocusTerminal();
  };
  const draw = () => {
    const main = imagesSection(st, close, { target });
    main.querySelector(":scope > .set-head")?.append(escapeButton("set-x", t("dialog.close"), close));
    panel.replaceChildren(main);
  };
  shut = close;
  redraw = draw;

  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  });
  document.body.appendChild(overlay);
  draw();
  // The empty library has no field to take the keys: the panel holds them, so Esc works.
  queueMicrotask(() => {
    if (!panel.contains(document.activeElement)) panel.focus();
  });
}

/**
 * The peek's card: the DOM of the frame, header and body over the panes.
 * It holds the one card slot and owns no preview state. The controller in
 * peek.ts decides when a card shows, and passes what the card calls back.
 */
import { $, button, h } from "../ui/dom";
import { icon } from "../ui/icons";
import { t, type MessageKey } from "../i18n";
import { splitPath } from "../workspace/paths";

/** The card over the panes, and the blob URL of a Host image on it, revoked when the card goes. */
export const slot: { el: HTMLElement | null; url: string | null } = { el: null, url: null };

/** The card's own action: the Enter keycap. Its Esc keycap is not one. */
export const cardAction = () => slot.el?.querySelector<HTMLButtonElement>("button.file-preview-key, button.peek-open") ?? null;

export function clearCard() {
  slot.el?.remove();
  slot.el = null;
  if (slot.url) URL.revokeObjectURL(slot.url);
  slot.url = null;
}

/** The Esc keycap at the right of a preview's header. */
function escButton(close: () => void): HTMLButtonElement {
  const b = button("peek-esc", "Esc", close);
  b.title = t("peek.closeKey", { key: "Esc" });
  b.setAttribute("aria-label", t("peek.close"));
  return b;
}

/** The word in a preview's header for its kind: file, folder or diff. */
const KIND_NAMES: Record<string, MessageKey> = { file: "peek.kindFile", folder: "peek.kindFolder", diff: "peek.kindDiff" };
const kindName = (kind: string) => (KIND_NAMES[kind] ? t(KIND_NAMES[kind]) : kind);

export function peekHead(kind: string, title: string, where: string, close: () => void): HTMLElement {
  const head = h("div", "peek-head");
  head.append(h("span", "peek-kind", kindName(kind)), h("span", "peek-title mono", title), h("span", "peek-where mono", where), escButton(close));
  return head;
}

/** A card's frame: its head, with the kind, the name, its folder and Esc. */
export function cardFrame(kind: string, path: string, close: () => void, cls = "peek file-preview"): HTMLElement {
  const { name, dir } = splitPath(path);
  const el = h("div", cls);
  el.tabIndex = -1;
  el.setAttribute("aria-label", name);
  const head = peekHead(kind, name, dir, close);
  el.append(head);
  return el;
}

/** A card with its body: `label` and `act` add the clickable Enter keycap. */
export function buildCard(kind: string, path: string, close: () => void, label?: string, act?: () => void): HTMLElement {
  const el = cardFrame(kind, path, close);
  const body = h("div", "file-preview-body");
  if (label && act) {
    // A keycap you can click, with the action under it: the hint and the button in one.
    const cap = button("file-preview-key", "", act);
    cap.title = t("peek.actKey", { label });
    cap.setAttribute("aria-label", label);
    const arrow = icon(`<path d="M19 5v8H6m0 0 4-4m-4 4 4 4"></path>`);
    arrow.classList.add("file-preview-key-arrow");
    cap.append(h("span", "", "Enter"), arrow);
    const space = label.indexOf(" ");
    const text = h("div", "file-preview-label");
    text.append(h("b", "", space < 0 ? label : label.slice(0, space)), space < 0 ? "" : label.slice(space));
    body.append(cap, text);
  }
  el.append(body);
  return el;
}

/** Puts `el` up as the card, over the panes. True when the keys were on the card it replaces. */
export function mountCard(el: HTMLElement, hooks: { close(): void; fold(): void }): boolean {
  // Tab does nothing here. Space opens or closes the row, as on the row. Enter runs the button.
  el.addEventListener("keydown", (e) => {
    if (e.key === "Tab") e.preventDefault();
    else if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      e.preventDefault();
      hooks.close();
    } else if (e.key === " " && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      e.preventDefault();
      hooks.fold();
    }
  });
  const hadKeys = !!slot.el && slot.el.contains(document.activeElement);
  clearCard();
  slot.el = el;
  $("main").appendChild(el);
  return hadKeys;
}

/** The whole-worktree diff's title: what it counts from, when not HEAD. */
export const allChanges = (base?: string) => (base ? t("peek.allChangesSince", { base }) : t("peek.allChanges"));

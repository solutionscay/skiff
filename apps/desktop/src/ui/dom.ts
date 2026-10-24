/** DOM queries and element builders. */
export const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/**
 * True while a menu, the palette, Settings, a dialog or a visible preview is up.
 * A session terminal must not take its focus. App navigation checks dialogs separately,
 * so it can leave the peek without sending its key to the tool.
 */
export function modalOpen(includePeek = true): boolean {
  const dialogs = "#ctx-menu:not([hidden]), #launch-menu:not([hidden]), #switcher:not([hidden]), #settings:not([hidden]), #mission-control:not([hidden]), .confirm-overlay, .fleet-name";
  return !!document.querySelector(includePeek ? `${dialogs}, .peek:not([hidden])` : dialogs);
}

/**
 * Sets `hidden` and text only when they change. Renders run many times a
 * second, and the engine restyles on every write, even of the same value:
 * a `hidden` write also wakes the `body:has(... :not([hidden]))` rules.
 */
export function setHidden(el: HTMLElement, hidden: boolean) {
  if (el.hidden !== hidden) el.hidden = hidden;
}

export function setText(el: HTMLElement, text: string) {
  if (el.textContent !== text) el.textContent = text;
}

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  if (text !== undefined) el.textContent = text;
  return el;
}

export function button(cls: string, text: string, onClick: () => void): HTMLButtonElement {
  const b = h("button", cls, text);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

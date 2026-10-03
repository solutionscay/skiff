/** DOM queries and element builders. */
export const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/**
 * True while a menu, the palette, Settings, a dialog or a tool in the peek is up. It owns the keys
 * until it closes: no terminal takes focus and no app shortcut fires.
 */
export function modalOpen(): boolean {
  return !!document.querySelector("#ctx-menu:not([hidden]), #launch-menu:not([hidden]), #switcher:not([hidden]), #settings:not([hidden]), .confirm-overlay, .peek.busy");
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

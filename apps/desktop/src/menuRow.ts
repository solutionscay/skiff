import { h } from "./dom";

/** Shared menu-row markup. The menu controller adds events and state. */
export function menuRow(label: string, hint: string, cls: string) {
  const row = h("button", cls);
  row.type = "button";
  row.setAttribute("role", "menuitem");
  const key = h("span", "lm-key mono");
  const name = h("span", "lm-name", label);
  const trailing = h("span", "lm-hint mono", hint);
  return { row, key, name, trailing };
}

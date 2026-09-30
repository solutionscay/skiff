import { button, h } from "./dom";
import { host } from "./terminalHost";

import { S } from "./state";

import { panes } from "./terminalState";

let findBar: HTMLElement | null = null;

export function openFind() {
  const pane = S.focused ? panes.get(S.focused) : undefined;
  if (!pane) return;
  findBar?.remove();
  const bar = h("div", "find-bar");
  const input = h("input", "find-input");
  input.placeholder = "Find";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Find in terminal");
  const opts = { decorations: { matchOverviewRuler: "#ffb454", activeMatchColorOverviewRuler: "#ffb454", activeMatchBackground: "#ffb45466", matchBackground: "#ffb45433" } };
  const next = () => pane.search.findNext(input.value, opts);
  const prev = () => pane.search.findPrevious(input.value, opts);
  const close = () => {
    pane.search.clearDecorations();
    bar.remove();
    findBar = null;
    pane.term.focus();
  };
  input.addEventListener("input", next);
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") (e.shiftKey ? prev : next)();
    else if (e.key === "Escape") close();
    else return;
    e.preventDefault();
  });
  bar.append(input, button("find-btn", "↑", prev), button("find-btn", "↓", next), button("find-btn", "×", close));
  host.appendChild(bar);
  findBar = bar;
  input.focus();
}

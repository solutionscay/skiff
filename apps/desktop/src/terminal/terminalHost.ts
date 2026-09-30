/** The terminal host and the hidden park for panes outside the layout. */
import { $ } from "../ui/dom";

export const host = $<HTMLDivElement>("terminal-host");

/** Terminals not in the layout wait here, hidden, with their scrollback. */
export const park = document.createElement("div");

park.className = "park";

host.appendChild(park);

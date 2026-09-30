import { h } from "./dom";

/** Shared dialog markup. Each caller owns its events and focus rules. */
export function dialogFrame(panelClass: string, role: "dialog" | "alertdialog" = "dialog", label?: string) {
  const overlay = h("div", "confirm-overlay");
  const panel = h("div", panelClass);
  panel.setAttribute("role", role);
  panel.setAttribute("aria-modal", "true");
  if (label !== undefined) panel.setAttribute("aria-label", label);
  return { overlay, panel };
}

export function dialogActions(action: string, actionClass = "confirm-act") {
  const foot = h("div", "confirm-foot");
  const cancel = h("button", "confirm-cancel", "Cancel");
  const act = h("button", actionClass, action);
  cancel.type = act.type = "button";
  foot.append(cancel, act);
  return { foot, cancel, act };
}

export function escapeButton(cls: string, label: string, close: () => void) {
  const x = h("button", cls);
  x.type = "button";
  x.title = "Close (Esc)";
  x.setAttribute("aria-label", label);
  x.appendChild(h("kbd", "", "Esc"));
  x.addEventListener("click", close);
  return x;
}

export function dialogHeader(title: string, close: () => void) {
  const head = h("div", "tc-head");
  head.append(h("div", "tc-title", title), escapeButton("tc-x", "Close", close));
  return head;
}

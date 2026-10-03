import { $, button, h } from "./dom";

/**
 * A bar above the workspace that stays until dismissed. The status bar was
 * not enough: the next daemon status update wrote over the message.
 */
export function showError(e: unknown) {
  console.error(e);
  showBar(e instanceof Error ? e.message : String(e), "alert");
}

/** The same bar for a result the user asked for. */
export function showNotice(text: string) {
  showBar(text, "status");
}

function showBar(text: string, role: string) {
  document.getElementById("error-alert")?.remove();
  const alert = h("div", "");
  alert.id = "error-alert";
  alert.setAttribute("role", role);
  if (role === "status") alert.className = "notice";
  alert.append(h("span", "alert-text", text), button("alert-btn", "Dismiss", () => alert.remove()));
  $("body").before(alert);
}

import { h } from "./dom";
import { t } from "../i18n";

/** Shared dialog markup. Each caller owns its events and focus rules. */
export function dialogFrame(panelClass: string, role: "dialog" | "alertdialog" = "dialog", label?: string) {
  const overlay = h("div", "confirm-overlay");
  const panel = h("div", panelClass);
  panel.setAttribute("role", role);
  panel.setAttribute("aria-modal", "true");
  if (label !== undefined) panel.setAttribute("aria-label", label);
  return { overlay, panel };
}

/** A click on the backdrop or Esc closes. No key reaches the app behind the overlay. */
export function closeOnBackdropOrEscape(overlay: HTMLElement, close: () => void) {
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
}

export function dialogActions(action: string, actionClass = "confirm-act") {
  const foot = h("div", "confirm-foot");
  const cancel = h("button", "confirm-cancel", t("dialog.cancel"));
  const act = h("button", actionClass, action);
  cancel.type = act.type = "button";
  foot.append(cancel, act);
  return { foot, cancel, act };
}

export function escapeButton(cls: string, label: string, close: () => void) {
  const x = h("button", cls);
  x.type = "button";
  x.title = t("dialog.closeEsc");
  x.setAttribute("aria-label", label);
  x.appendChild(h("kbd", "", "Esc"));
  x.addEventListener("click", close);
  return x;
}

export function dialogHeader(title: string, close: () => void) {
  const head = h("div", "tc-head");
  head.append(h("div", "tc-title", title), escapeButton("tc-x", t("dialog.close"), close));
  return head;
}

/**
 * The one action dialog: a title, an optional line, the caller's content, an
 * error line, and Cancel beside the action. Enter runs the action unless Cancel
 * has the keys. Esc, Cancel or a click outside closes. Tab stays in the dialog.
 * A rejected `submit` shows its error and keeps the dialog open. The keys go
 * back to what held them, unless `onClose` places them.
 */
export function actionDialog(o: {
  title: string;
  body?: string;
  content?: HTMLElement[];
  action: string;
  /** "go" for an ordinary action, "danger" for one that cannot be undone. */
  tone: "go" | "danger";
  submit: () => void | Promise<void>;
  onClose?: () => void;
  /** Takes the keys when the dialog opens. Default: the action. */
  focus?: HTMLElement;
  /** A face before the title: a Skipper's avatar. */
  face?: HTMLElement;
  /** A second action before Cancel, such as Reset to default. It closes as the action does. */
  also?: { label: string; run: () => void | Promise<void> };
}) {
  const back = document.activeElement as HTMLElement | null;
  const { overlay, panel } = dialogFrame("confirm-panel", o.tone === "danger" ? "alertdialog" : "dialog", o.title);
  const title = h("div", "confirm-title", o.title);
  if (o.face) title.prepend(o.face);
  const error = h("div", "prompt-error");
  error.setAttribute("role", "alert");
  const { foot, cancel, act } = dialogActions(o.action, o.tone === "go" ? "confirm-act go" : "confirm-act");
  const also = o.also ? h("button", "confirm-cancel", o.also.label) : null;
  if (also) {
    also.type = "button";
    foot.prepend(also);
  }
  panel.append(title, ...(o.body ? [h("div", "confirm-body", o.body)] : []), ...(o.content ?? []), error, foot);
  overlay.appendChild(panel);
  document.body.appendChild(overlay);

  let busy = false;
  let open = true;
  const close = () => {
    if (!open) return;
    open = false;
    overlay.remove();
    if (o.onClose) o.onClose();
    else back?.focus?.();
  };
  const run = async (job: () => void | Promise<void> = o.submit, button: HTMLButtonElement = act) => {
    if (busy) return;
    const text = button.textContent;
    busy = act.disabled = true;
    if (also) also.disabled = true;
    button.textContent = "…";
    error.textContent = "";
    try {
      await job();
      close();
    } catch (e) {
      busy = act.disabled = false;
      if (also) also.disabled = false;
      button.textContent = text;
      error.textContent = String(e);
    }
  };
  cancel.addEventListener("click", () => !busy && close());
  if (also && o.also) {
    const job = o.also.run;
    also.addEventListener("click", () => void run(job, also));
  }
  act.addEventListener("click", () => void run());
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay && !busy) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      if (!busy) close();
    } else if (e.key === "Enter") {
      if (document.activeElement === cancel || (also && document.activeElement === also)) return;
      void run();
    } else if (e.key === "Tab") {
      const stops = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled)")];
      const i = stops.indexOf(document.activeElement as HTMLElement);
      stops[(i + (e.shiftKey ? -1 : 1) + stops.length) % stops.length]?.focus();
    } else return;
    e.preventDefault();
  });
  (o.focus ?? act).focus();
  return { close, error, run: () => run() };
}

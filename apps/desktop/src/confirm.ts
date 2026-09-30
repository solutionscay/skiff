import { h } from "./dom";
import { dialogActions, dialogFrame } from "./dialogParts";

/**
 * One modal for actions that cannot be undone. Resolves true on the action,
 * false on Cancel, Esc, or a click outside. Enter runs the action.
 */
export function confirmAction(o: { title: string; body: string; action: string }): Promise<boolean> {
  return new Promise((resolve) => {
    const back = document.activeElement as HTMLElement | null;
    const { overlay, panel } = dialogFrame("confirm-panel", "alertdialog");
    const title = h("div", "confirm-title");
    title.id = "confirm-title";
    title.textContent = o.title;
    const body = h("div", "confirm-body");
    body.id = "confirm-body";
    body.textContent = o.body;
    panel.setAttribute("aria-labelledby", title.id);
    panel.setAttribute("aria-describedby", body.id);
    const { foot, cancel, act } = dialogActions(o.action);
    panel.append(title, body, foot);
    overlay.appendChild(panel);
    document.body.appendChild(overlay);

    const done = (ok: boolean) => {
      overlay.remove();
      back?.focus?.();
      resolve(ok);
    };
    cancel.addEventListener("click", () => done(false));
    act.addEventListener("click", () => done(true));
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) done(false);
    });
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        done(false);
      } else if (e.key === "Enter") {
        e.preventDefault();
        done(document.activeElement !== cancel);
      } else if (e.key === "Tab") {
        e.preventDefault();
        (document.activeElement === act ? cancel : act).focus();
      }
    });
    act.focus();
  });
}

/**
 * A modal with one text field. `submit` runs on Enter or the action button;
 * a thrown error shows in the dialog and keeps it open. Esc or Cancel closes.
 */
export function promptAction(o: {
  title: string;
  body: string;
  placeholder: string;
  action: string;
  submit: (value: string) => Promise<void>;
  onClose?: () => void;
}): void {
  const { overlay, panel } = dialogFrame("confirm-panel", "dialog", o.title);
  const title = h("div", "confirm-title");
  title.textContent = o.title;
  const body = h("div", "confirm-body");
  body.textContent = o.body;
  const input = h("input", "prompt-input");
  input.type = "text";
  input.spellcheck = false;
  input.placeholder = o.placeholder;
  input.setAttribute("aria-label", o.placeholder);
  const error = h("div", "prompt-error");
  error.setAttribute("role", "alert");
  const { foot, cancel, act } = dialogActions(o.action, "confirm-act go");
  panel.append(title, body, input, error, foot);
  overlay.appendChild(panel);
  document.body.appendChild(overlay);

  let busy = false;
  const close = () => {
    overlay.remove();
    o.onClose?.();
  };
  const run = async () => {
    if (busy) return;
    const v = input.value.trim();
    if (!v) {
      error.textContent = `Type ${o.placeholder}.`;
      input.focus();
      return;
    }
    busy = true;
    act.disabled = input.disabled = true;
    act.textContent = "…";
    error.textContent = "";
    try {
      await o.submit(v);
      close();
    } catch (e) {
      busy = false;
      act.disabled = input.disabled = false;
      act.textContent = o.action;
      error.textContent = String(e);
      input.focus();
    }
  };
  cancel.addEventListener("click", close);
  act.addEventListener("click", () => void run());
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay && !busy) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape" && !busy) close();
    else if (e.key === "Enter" && document.activeElement !== cancel) void run();
    else return;
    e.preventDefault();
  });
  input.focus();
}

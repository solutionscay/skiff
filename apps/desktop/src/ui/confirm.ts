import { h } from "./dom";
import { actionDialog } from "./dialogParts";

/**
 * One modal for actions that cannot be undone. Resolves true on the action,
 * false on Cancel, Esc, or a click outside. Enter runs the action.
 */
export function confirmAction(o: { title: string; body: string; action: string }): Promise<boolean> {
  return new Promise((resolve) => {
    let ok = false;
    const back = document.activeElement as HTMLElement | null;
    actionDialog({
      ...o,
      tone: "danger",
      submit: () => void (ok = true),
      onClose: () => {
        back?.focus?.();
        resolve(ok);
      },
    });
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
  const input = h("input", "prompt-input");
  input.type = "text";
  input.spellcheck = false;
  input.placeholder = o.placeholder;
  input.setAttribute("aria-label", o.placeholder);
  actionDialog({
    title: o.title,
    body: o.body,
    content: [input],
    action: o.action,
    tone: "go",
    focus: input,
    onClose: o.onClose,
    submit: async () => {
      const v = input.value.trim();
      if (!v) {
        input.focus();
        throw `Type ${o.placeholder}.`;
      }
      await o.submit(v);
    },
  });
}

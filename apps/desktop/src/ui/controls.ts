/**
 * The app's form controls: a picker, a switch, a text field, a text field
 * with a list and a small button. Every screen uses these, so they look and
 * act one way. No native <select> or <datalist>: a picker opens the app's own
 * menu.
 */
import { h } from "./dom";
import { ctxMenu } from "./contextMenu";
import type { MenuEntry } from "./menu";

/** One line of a picker's menu. `value` picks it. `run` does something else, such as a file picker. */
export type PickEntry =
  | { value: string; label: string; hint?: string }
  | { label: string; run: () => void }
  | { head: string }
  | { sep: true };

interface PickOptions {
  /** The control's name, and the title of its menu. */
  label: string;
  /** The value in use. The menu marks it. */
  value: string;
  /** Read when the menu opens. */
  entries: () => PickEntry[];
  /** What the control shows. Absent: the label of the value's entry. */
  shows?: string;
  disabled?: boolean;
  /** Shows dim: a default, not a choice the person made. */
  quiet?: boolean;
  /** Typed text filters a long menu. */
  filter?: boolean;
  pick: (value: string) => void;
}

/** A control that shows a value and opens a menu of values. */
export function picker(o: PickOptions): HTMLButtonElement {
  const b = h("button", "ctl-pick");
  b.type = "button";
  b.disabled = !!o.disabled;
  b.dataset.key = o.label;
  b.setAttribute("aria-label", o.label);
  b.setAttribute("aria-haspopup", "menu");
  const shows = o.shows ?? o.entries().flatMap((e) => ("value" in e && e.value === o.value ? [e.label] : []))[0] ?? o.value;
  b.append(h("span", "ctl-pick-value" + (o.quiet ? " quiet" : ""), shows));
  b.addEventListener("click", () => {
    const r = b.getBoundingClientRect();
    const items = o.entries().map((e): MenuEntry => {
      if ("value" in e) {
        const value = e.value;
        return {
          label: e.label,
          hint: e.hint,
          ...(value === o.value ? { icon: "indicators-check" } : {}),
          run: () => {
            b.focus();
            if (value !== o.value) o.pick(value);
          },
        };
      }
      if ("run" in e) {
        const run = e.run;
        return { label: e.label, run: () => { b.focus(); run(); } };
      }
      return e;
    });
    ctxMenu.open(r.left, r.bottom, o.label, items, () => b.focus(), { filter: o.filter });
  });
  return b;
}

/** A switch. `text` adds a label after it. `danger`: red when on, for a choice that destroys. */
export function switchButton(o: { label: string; on: boolean; disabled?: boolean; text?: string; danger?: boolean; toggle: () => void }): HTMLButtonElement {
  const b = h("button", "ctl-switch" + (o.danger ? " danger" : ""));
  b.type = "button";
  b.disabled = !!o.disabled;
  b.dataset.key = o.label;
  b.setAttribute("role", "switch");
  b.setAttribute("aria-checked", String(o.on));
  b.setAttribute("aria-label", o.label);
  b.append(h("span", "switch"));
  if (o.text) b.append(h("span", "ctl-switch-text", o.text));
  b.addEventListener("click", () => o.toggle());
  return b;
}

/**
 * A text field. `save` runs on Enter or blur when the text changed, and Esc
 * puts the old text back. Without `save`, `input` follows each key and `enter`
 * runs on Enter.
 */
export function textBox(o: {
  label: string;
  value: string;
  placeholder?: string;
  mono?: boolean;
  /** The name of a row: the row's weight. */
  name?: boolean;
  save?: (value: string) => void;
  input?: (value: string) => void;
  enter?: () => void;
}): HTMLInputElement {
  const el = h("input", "ctl-field" + (o.mono ? " mono" : "") + (o.name ? " name" : ""));
  el.type = "text";
  el.value = o.value;
  el.placeholder = o.placeholder ?? "";
  el.spellcheck = false;
  el.autocomplete = "off";
  el.setAttribute("aria-label", o.label);
  if (o.input) el.addEventListener("input", () => o.input!(el.value));
  el.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (o.enter) o.enter();
      else el.blur();
    } else if (e.key === "Escape" && o.save) {
      // The old text comes back, and Esc goes on to close what holds the field.
      el.value = o.value;
      el.blur();
    }
  });
  if (o.save) el.addEventListener("blur", () => {
    const v = el.value.trim();
    if (v !== o.value.trim()) o.save!(v);
  });
  return el;
}

/**
 * A text field with a list: the person types a value, or opens the list with
 * the ▾, Alt+Down or F4 and picks one. Enter saves, and so does blur when the
 * text changed. Esc puts the saved text back. `save` rejects with the error
 * to show: the field takes a red rule and the error shows in its cell. An
 * empty field shows `placeholder`: the default.
 */
export function comboBox(o: {
  label: string;
  value: string;
  placeholder?: string;
  mono?: boolean;
  /** The list's value in use. The list marks it. */
  picked: string;
  entries: () => PickEntry[];
  pick: (value: string) => void;
  save: (text: string) => Promise<void>;
}): HTMLElement {
  const box = h("span", "ctl-combo");
  box.dataset.key = o.label;
  const el = h("input", "ctl-field" + (o.mono ? " mono" : ""));
  el.type = "text";
  el.value = o.value;
  el.placeholder = o.placeholder ?? "";
  el.spellcheck = false;
  el.autocomplete = "off";
  el.setAttribute("aria-label", o.label);
  el.setAttribute("aria-haspopup", "menu");
  const error = h("span", "ctl-combo-error");
  error.hidden = true;
  const open = h("span", "ctl-combo-open");
  open.setAttribute("aria-hidden", "true");
  box.append(el, error, open);

  /** The text that failed last: blur after Enter does not try it again. */
  let failed: string | null = null;
  let listOpen = false;
  // Settings lets Esc through to the field while there is something to put back.
  const mark = () => box.classList.toggle("edited", el.value !== o.value || !error.hidden);
  const showError = (text: string | null) => {
    error.textContent = text ?? "";
    error.hidden = text === null;
    if (text === null) {
      el.removeAttribute("aria-invalid");
      el.removeAttribute("aria-description");
    } else {
      el.setAttribute("aria-invalid", "true");
      el.setAttribute("aria-description", text);
    }
    mark();
  };
  const commit = async () => {
    const v = el.value.trim();
    if (v === failed) return;
    if (v === o.value.trim()) return showError(null);
    try {
      await o.save(v);
      failed = null;
      showError(null);
    } catch (e) {
      failed = v;
      showError(String(e));
    }
  };
  const showList = () => {
    const r = box.getBoundingClientRect();
    const items = o.entries().map((e): MenuEntry => {
      if ("value" in e) {
        const value = e.value;
        return { label: e.label, hint: e.hint, ...(value === o.picked ? { icon: "indicators-check" } : {}), run: () => {
          listOpen = false;
          el.value = o.value;
          showError(null);
          el.focus();
          if (value !== o.picked) o.pick(value);
        } };
      }
      if ("run" in e) {
        const run = e.run;
        return { label: e.label, run: () => { listOpen = false; el.focus(); run(); } };
      }
      return e;
    });
    listOpen = true;
    ctxMenu.open(r.left, r.bottom, o.label, items, () => { listOpen = false; el.focus(); });
  };
  el.addEventListener("input", () => {
    failed = null;
    mark();
  });
  el.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void commit();
    } else if (e.key === "Escape" && box.classList.contains("edited")) {
      e.preventDefault();
      e.stopPropagation();
      el.value = o.value;
      failed = null;
      showError(null);
    } else if ((e.key === "ArrowDown" && e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) || e.key === "F4") {
      e.preventDefault();
      showList();
    }
  });
  el.addEventListener("blur", () => {
    if (!listOpen) void commit();
  });
  // The ▾ keeps the keys in the field.
  open.addEventListener("mousedown", (e) => e.preventDefault());
  open.addEventListener("click", () => {
    el.focus();
    showList();
  });
  return box;
}

/** A small button. `go`: the one action a screen asks for. `danger`: red when lit. */
export function smallButton(text: string, run: () => void, o: { go?: boolean; danger?: boolean; label?: string; icon?: Node } = {}): HTMLButtonElement {
  const b = h("button", "ctl-btn" + (o.go ? " go" : "") + (o.danger ? " danger" : "") + (o.icon ? " icon" : ""), o.icon ? undefined : text);
  b.type = "button";
  if (o.icon) b.append(o.icon);
  if (o.label) b.setAttribute("aria-label", o.label);
  if (o.icon) b.title = text;
  b.addEventListener("click", run);
  return b;
}

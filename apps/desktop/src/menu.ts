export type MenuEntry =
  | { head: string }
  | { icon?: string; glyph?: HTMLElement; label: string; hint?: string; run: () => void; danger?: boolean; disabled?: boolean; hover?: () => void };

/** One right-click menu for session rows, panes, and groups. Esc or a click outside closes it. */
export function createMenu(onClose: () => void) {
  const menu = document.createElement("div");
  menu.id = "ctx-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  document.body.appendChild(menu);

  /** Called once when the menu closes without a pick (Esc, click outside, another menu). */
  let dismiss: (() => void) | null = null;

  function close(refocus = true) {
    if (menu.hidden) return;
    menu.hidden = true;
    const d = dismiss;
    dismiss = null;
    d?.();
    if (refocus) onClose();
  }

  function open(x: number, y: number, title: string, entries: MenuEntry[], onDismiss?: () => void) {
    if (!menu.hidden) close(false);
    dismiss = onDismiss ?? null;
    const t = document.createElement("div");
    t.className = "lm-title";
    t.textContent = title;
    const rows = entries.map((e) => {
      if ("head" in e) {
        const hd = document.createElement("div");
        hd.className = "lm-head";
        hd.textContent = e.head;
        return hd;
      }
      const b = document.createElement("button");
      b.type = "button";
      b.className = "lm-item" + (e.danger ? " danger" : "");
      b.disabled = !!e.disabled;
      b.setAttribute("role", "menuitem");
      const k = document.createElement("span");
      k.className = "lm-key mono";
      k.textContent = e.icon ?? "";
      const n = document.createElement("span");
      n.className = "lm-name";
      n.textContent = e.label;
      const hint = document.createElement("span");
      hint.className = "lm-hint mono";
      hint.textContent = e.hint ?? "";
      b.append(e.glyph ?? k, n, hint);
      if (e.hover) {
        b.addEventListener("mouseenter", e.hover);
        b.addEventListener("focus", e.hover);
      }
      b.addEventListener("click", () => {
        dismiss = null;
        close(false);
        e.run();
      });
      return b;
    });
    menu.replaceChildren(t, ...rows);
    menu.setAttribute("aria-label", title);
    menu.hidden = false;
    const w = menu.offsetWidth;
    const h = menu.offsetHeight;
    menu.style.left = `${Math.max(0, Math.min(x, window.innerWidth - w - 8))}px`;
    menu.style.top = `${Math.max(0, Math.min(y, window.innerHeight - h - 8))}px`;
    menu.querySelector<HTMLButtonElement>(".lm-item:not(:disabled)")?.focus();
  }

  menu.addEventListener("keydown", (e) => {
    const items = [...menu.querySelectorAll<HTMLButtonElement>(".lm-item:not(:disabled)")];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") close();
    else if (e.key === "ArrowDown") items[(i + 1) % items.length]?.focus();
    else if (e.key === "ArrowUp") items[(i - 1 + items.length) % items.length]?.focus();
    else return;
    e.preventDefault();
    e.stopPropagation();
  });
  document.addEventListener("mousedown", (e) => {
    if (!menu.hidden && !menu.contains(e.target as Node)) close(false);
  }, true);

  return { open, close, get isOpen() { return !menu.hidden; } };
}

import { menuRow } from "./menuRow";
import { isRune, rune } from "./runes";
import { eventLabel } from "../app/keys";

export type MenuEntry =
  | { head: string }
  | {
      icon?: string;
      glyph?: HTMLElement;
      label: string;
      hint?: string;
      /** A leaf: runs on pick. */
      run?: () => void;
      /** A submenu: opens beside the item on hover, click, Enter or Right. */
      sub?: MenuEntry[];
      danger?: boolean;
      disabled?: boolean;
      hover?: () => void;
    };

/** One right-click menu for session rows, panes, and groups. Esc or a click outside closes it. */
/** Where the pointer last was, and where it rested when a menu opened. */
let pointerAt = [-1, -1];
let restAt: number[] | null = null;
window.addEventListener("mousemove", (e) => (pointerAt = [e.clientX, e.clientY]), true);

/** Call when a menu opens. A menu drawn under a still pointer gets a move event
 *  there; that one must not take the keys from the first item. */
export function menuOpened() {
  restAt = pointerAt;
}

/** The pointer takes the keys once it really moves, so one item is lit whether
 *  pointer or arrows moved last. */
export function followPointer(b: HTMLButtonElement) {
  b.addEventListener("mousemove", (e) => {
    if (restAt && e.clientX === restAt[0] && e.clientY === restAt[1]) return;
    restAt = null;
    if (!b.disabled && document.activeElement !== b) b.focus();
  });
}

export function createMenu(onClose: () => void) {
  const menu = document.createElement("div");
  menu.id = "ctx-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  document.body.appendChild(menu);

  /** The flyout for the item under the pointer. One level deep. */
  const fly = document.createElement("div");
  fly.id = "ctx-sub";
  fly.className = "ctx-fly";
  fly.setAttribute("role", "menu");
  fly.hidden = true;
  document.body.appendChild(fly);
  let flyOwner: HTMLButtonElement | null = null;
  let hoverTimer = 0;

  /** Called once when the menu closes without a pick (Esc, click outside, another menu). */
  let dismiss: (() => void) | null = null;

  function closeFly() {
    clearTimeout(hoverTimer);
    fly.hidden = true;
    flyOwner?.setAttribute("aria-expanded", "false");
    flyOwner = null;
  }

  function close(refocus = true) {
    if (menu.hidden) return;
    closeFly();
    menu.hidden = true;
    const d = dismiss;
    dismiss = null;
    d?.();
    if (refocus) onClose();
  }

  function pick(run: () => void) {
    dismiss = null;
    close(false);
    run();
  }

  function rowsFor(entries: MenuEntry[], inFly: boolean): HTMLElement[] {
    return entries.map((e) => {
      if ("head" in e) {
        const hd = document.createElement("div");
        hd.className = "lm-head";
        hd.textContent = e.head;
        return hd;
      }
      const { row: b, key: k, name: n, trailing: hint } = menuRow(e.label, e.sub ? "›" : e.hint ?? "", "lm-item" + (e.danger ? " danger" : ""));
      b.disabled = !!e.disabled;
      // A rune name draws that icon; other text shows as it is.
      const flip = e.icon?.endsWith(":flip");
      const name = flip ? e.icon!.slice(0, -5) : e.icon ?? "";
      if (isRune(name)) k.append(rune(name, 14, flip));
      else k.textContent = e.icon ?? "";
      b.append(e.glyph ?? k, n, hint);
      if (e.sub) b.setAttribute("aria-haspopup", "menu");
      if (e.hover) {
        b.addEventListener("mouseenter", e.hover);
        b.addEventListener("focus", e.hover);
      }
      if (!inFly) {
        // Moving onto another item closes a flyout; resting on a submenu item opens one.
        b.addEventListener("mouseenter", () => {
          clearTimeout(hoverTimer);
          if (e.sub && !e.disabled) hoverTimer = window.setTimeout(() => openFly(b, e.sub!, false), 120);
          else if (flyOwner) closeFly();
        });
      }
      followPointer(b);
      if (e.hint && !e.sub) b.dataset.hint = e.hint;
      b.addEventListener("click", () => {
        if (e.sub) return openFly(b, e.sub, false);
        pick(() => e.run?.());
      });
      return b;
    });
  }

  function openFly(owner: HTMLButtonElement, entries: MenuEntry[], focus: boolean) {
    clearTimeout(hoverTimer);
    flyOwner?.setAttribute("aria-expanded", "false");
    flyOwner = owner;
    owner.setAttribute("aria-expanded", "true");
    fly.replaceChildren(...rowsFor(entries, true));
    fly.setAttribute("aria-label", owner.textContent ?? "");
    fly.hidden = false;
    const r = owner.getBoundingClientRect();
    const w = fly.offsetWidth;
    const h = fly.offsetHeight;
    // Right of the item; left of the menu when there is no room. Aligned with the item's top edge.
    const m = menu.getBoundingClientRect();
    const left = r.right + w + 8 <= window.innerWidth ? r.right - 1 : Math.max(0, m.left - w + 1);
    fly.style.left = `${left}px`;
    fly.style.top = `${Math.max(0, Math.min(r.top, window.innerHeight - h - 8))}px`;
    if (focus) fly.querySelector<HTMLButtonElement>(".lm-item:not(:disabled)")?.focus();
  }

  function open(x: number, y: number, title: string, entries: MenuEntry[], onDismiss?: () => void) {
    if (!menu.hidden) close(false);
    menuOpened();
    dismiss = onDismiss ?? null;
    const t = document.createElement("div");
    t.className = "lm-title";
    t.textContent = title;
    menu.replaceChildren(t, ...rowsFor(entries, false));
    menu.setAttribute("aria-label", title);
    menu.hidden = false;
    const w = menu.offsetWidth;
    const h = menu.offsetHeight;
    menu.style.left = `${Math.max(0, Math.min(x, window.innerWidth - w - 8))}px`;
    menu.style.top = `${Math.max(0, Math.min(y, window.innerHeight - h - 8))}px`;
    menu.querySelector<HTMLButtonElement>(".lm-item:not(:disabled)")?.focus();
  }

  const nav = (root: HTMLElement, e: KeyboardEvent) => {
    const items = [...root.querySelectorAll<HTMLButtonElement>(".lm-item:not(:disabled)")];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") items[(i + 1) % items.length]?.focus();
    else if (e.key === "ArrowUp") items[(i - 1 + items.length) % items.length]?.focus();
    else return false;
    return true;
  };

  /** A key that an item shows as its hint picks that item, as a click does.
   *  The item runs on the menu's own target, so the key acts on that project,
   *  group or session. Enter still picks the lit item: Ctrl+Shift held from the
   *  key that opened the menu must not run Maximize. */
  const hintKey = (root: HTMLElement, e: KeyboardEvent) => {
    // A bare digit picks a start-list item, which shows its digit as the hint.
    const digit = !e.ctrlKey && !e.metaKey && !e.altKey && /^[1-9]$/.test(e.key);
    if ((!digit && !e.ctrlKey && !e.metaKey && !e.altKey) || e.key === "Enter" || e.key === " ") return false;
    const want = digit ? e.key : eventLabel(e);
    const b = [...root.querySelectorAll<HTMLButtonElement>(".lm-item[data-hint]:not(:disabled)")].find((x) => x.dataset.hint === want);
    b?.click();
    return !!b;
  };

  menu.addEventListener("keydown", (e) => {
    const cur = document.activeElement as HTMLButtonElement | null;
    if (e.key === "Escape") close();
    else if ((e.key === "ArrowRight" || e.key === "Enter" || e.key === " ") && cur?.getAttribute("aria-haspopup")) {
      // Enter and Space also click the button; the click opens the flyout without focus, so open it here with focus.
      const sub = flyOwner === cur && !fly.hidden ? null : cur;
      if (sub) sub.click();
      fly.querySelector<HTMLButtonElement>(".lm-item:not(:disabled)")?.focus();
    } else if (!nav(menu, e) && !hintKey(menu, e)) return;
    e.preventDefault();
    e.stopPropagation();
  });

  fly.addEventListener("keydown", (e) => {
    if (e.key === "ArrowLeft" || e.key === "Escape") {
      const owner = flyOwner;
      closeFly();
      owner?.focus();
    } else if (!nav(fly, e) && !hintKey(fly, e)) return;
    e.preventDefault();
    e.stopPropagation();
  });

  document.addEventListener("mousedown", (e) => {
    const t = e.target as Node;
    if (!menu.hidden && !menu.contains(t) && !fly.contains(t)) close(false);
  }, true);

  return { open, close, get isOpen() { return !menu.hidden; } };
}

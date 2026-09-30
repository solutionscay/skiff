import { rgb } from "./colors";
import { dialogFrame, dialogHeader } from "../ui/dialogParts";
import { h } from "../ui/dom";
import type { TerminalTheme } from "../platform/types";

const mix = (a: string, b: string, t: number) => {
  const x = rgb(a);
  const y = rgb(b);
  return "#" + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("");
};
const isLight = (hex: string) => {
  const [r, g, b] = rgb(hex);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 128;
};

/** A small Skiff window in the theme's colors: a sidebar and a few terminal lines. */
function preview(t: TerminalTheme): HTMLElement {
  const p = t.palette;
  const box = h("span", "tc-preview");
  box.style.background = t.background;
  const side = h("span", "tc-side");
  side.style.background = mix(t.background, t.foreground, 0.06);
  side.style.borderRight = `1px solid ${mix(t.background, t.foreground, 0.16)}`;
  for (const w of [70, 50, 60]) {
    const bar = h("span", "tc-bar");
    bar.style.width = `${w}%`;
    bar.style.background = mix(t.background, t.foreground, 0.35);
    side.appendChild(bar);
  }
  const term = h("span", "tc-term");
  const line = (parts: [string, string][]) => {
    const l = h("span", "tc-line");
    for (const [text, color] of parts) {
      const s = h("span", "", text);
      s.style.color = color;
      l.appendChild(s);
    }
    term.appendChild(l);
  };
  line([["~/skiff", p[2]], [" main", p[4]], [" $ cargo test", t.foreground]]);
  line([["   Compiling", p[2]], [" skiffd", t.foreground]]);
  line([["warning", p[3]], [": unused var", mix(t.foreground, t.background, 0.3)]]);
  line([["error", p[1]], ["[E0308]", p[5]], [" types", p[6]]]);
  const cursor = h("span", "tc-cursor");
  cursor.style.background = t.cursor ?? t.foreground;
  const last = h("span", "tc-line");
  const prompt = h("span", "", "$ ");
  prompt.style.color = p[2];
  last.append(prompt, cursor);
  term.appendChild(last);
  box.append(side, term);
  return box;
}

export interface CardOpts {
  /** The id shown as active. */
  active: string | null;
  pick: (id: string | null) => void;
  /** A first card that clears the choice, e.g. "Same as app". */
  none?: { label: string; theme: TerminalTheme | undefined };
}

/** A grid of theme cards. A click picks; nothing previews on hover. */
export function themeGrid(themes: TerminalTheme[], o: CardOpts): HTMLElement {
  const grid = h("div", "tc-grid");
  const card = (t: TerminalTheme, id: string | null, label: string, badge: string) => {
    const on = o.active === id;
    const b = h("button", "tc-card" + (on ? " active" : ""));
    b.type = "button";
    b.setAttribute("aria-pressed", String(on));
    b.appendChild(preview(t));
    const foot = h("span", "tc-foot");
    foot.append(h("span", "tc-name", label), h("span", "tc-badge", badge));
    if (on) foot.appendChild(h("span", "tc-state", "✓ Active"));
    b.appendChild(foot);
    b.addEventListener("click", () => o.pick(id));
    return b;
  };
  if (o.none?.theme) grid.appendChild(card(o.none.theme, null, o.none.label, "default"));
  for (const t of themes) grid.appendChild(card(t, t.id, t.name, isLight(t.background) ? "Light" : "Dark"));
  return grid;
}

const KEYS: Record<string, "left" | "right" | "up" | "down" | "first" | "last"> = {
  ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", Home: "first", End: "last",
};

/** The card an arrow key lands on. Up and down keep to the nearest column; the ends stop. */
function nextCard(cards: HTMLElement[], at: number, key: "left" | "right" | "up" | "down" | "first" | "last"): number {
  if (key === "first") return 0;
  if (key === "last") return cards.length - 1;
  if (key === "left") return Math.max(0, at - 1);
  if (key === "right") return Math.min(cards.length - 1, at + 1);
  const rect = cards[at].getBoundingClientRect();
  const dir = key === "down" ? 1 : -1;
  let best = at;
  let bestRow = Infinity;
  let bestDx = Infinity;
  for (const [i, c] of cards.entries()) {
    const r = c.getBoundingClientRect();
    const dy = (r.top - rect.top) * dir;
    if (dy <= 1) continue;
    const dx = Math.abs(r.left - rect.left);
    if (dy < bestRow - 1 || (Math.abs(dy - bestRow) <= 1 && dx < bestDx)) {
      best = i;
      bestRow = dy;
      bestDx = dx;
    }
  }
  return best;
}

/** Terminal theme picker: a modal of cards. A click applies and closes. */
export function pickTheme(title: string, themes: TerminalTheme[], o: CardOpts): void {
  const back = document.activeElement as HTMLElement | null;
  const { overlay, panel } = dialogFrame("tc-panel", "dialog", title);
  const head = dialogHeader(title, () => close());
  const body = h("div", "tc-body");
  const close = () => {
    overlay.remove();
    back?.focus?.();
  };
  body.appendChild(
    themeGrid(themes, {
      ...o,
      pick: (id) => {
        close();
        o.pick(id);
      },
    }),
  );
  panel.append(head, body);
  overlay.appendChild(panel);
  document.body.appendChild(overlay);
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      return;
    }
    const step = KEYS[e.key];
    if (!step || e.ctrlKey || e.altKey || e.metaKey) return;
    // Arrows move between cards, not the panel's scroll.
    e.preventDefault();
    document.body.classList.add("kbd");
    const cards = [...body.querySelectorAll<HTMLButtonElement>(".tc-card")];
    const at = cards.indexOf(document.activeElement as HTMLButtonElement);
    cards[nextCard(cards, at < 0 ? 0 : at, step)]?.focus();
  });
  (body.querySelector<HTMLButtonElement>(".tc-card.active") ?? body.querySelector<HTMLButtonElement>(".tc-card"))?.focus();
}

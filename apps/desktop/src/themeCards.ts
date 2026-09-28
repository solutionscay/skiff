import type { TerminalTheme } from "./types";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

const rgb = (hex: string) => {
  const n = parseInt(hex.replace("#", "").slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
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
  const box = el("span", "tc-preview");
  box.style.background = t.background;
  const side = el("span", "tc-side");
  side.style.background = mix(t.background, t.foreground, 0.06);
  side.style.borderRight = `1px solid ${mix(t.background, t.foreground, 0.16)}`;
  for (const w of [70, 50, 60]) {
    const bar = el("span", "tc-bar");
    bar.style.width = `${w}%`;
    bar.style.background = mix(t.background, t.foreground, 0.35);
    side.appendChild(bar);
  }
  const term = el("span", "tc-term");
  const line = (parts: [string, string][]) => {
    const l = el("span", "tc-line");
    for (const [text, color] of parts) {
      const s = el("span", "", text);
      s.style.color = color;
      l.appendChild(s);
    }
    term.appendChild(l);
  };
  line([["~/skiff", p[2]], [" main", p[4]], [" $ cargo test", t.foreground]]);
  line([["   Compiling", p[2]], [" skiffd", t.foreground]]);
  line([["warning", p[3]], [": unused var", mix(t.foreground, t.background, 0.3)]]);
  line([["error", p[1]], ["[E0308]", p[5]], [" types", p[6]]]);
  const cursor = el("span", "tc-cursor");
  cursor.style.background = t.cursor ?? t.foreground;
  const last = el("span", "tc-line");
  const prompt = el("span", "", "$ ");
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
  const grid = el("div", "tc-grid");
  const card = (t: TerminalTheme, id: string | null, label: string, badge: string) => {
    const on = o.active === id;
    const b = el("button", "tc-card" + (on ? " active" : ""));
    b.type = "button";
    b.setAttribute("aria-pressed", String(on));
    b.appendChild(preview(t));
    const foot = el("span", "tc-foot");
    foot.append(el("span", "tc-name", label), el("span", "tc-badge", badge));
    if (on) foot.appendChild(el("span", "tc-state", "✓ Active"));
    b.appendChild(foot);
    b.addEventListener("click", () => o.pick(id));
    return b;
  };
  if (o.none?.theme) grid.appendChild(card(o.none.theme, null, o.none.label, "default"));
  for (const t of themes) grid.appendChild(card(t, t.id, t.name, isLight(t.background) ? "Light" : "Dark"));
  return grid;
}

/** Terminal theme picker: a modal of cards. A click applies and closes. */
export function pickTheme(title: string, themes: TerminalTheme[], o: CardOpts): void {
  const back = document.activeElement as HTMLElement | null;
  const overlay = el("div", "confirm-overlay");
  const panel = el("div", "tc-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-modal", "true");
  panel.setAttribute("aria-label", title);
  const head = el("div", "tc-head");
  head.append(el("div", "tc-title", title), el("kbd", "", "Esc"));
  const body = el("div", "tc-body");
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
    }
  });
  (body.querySelector<HTMLButtonElement>(".tc-card.active") ?? body.querySelector<HTMLButtonElement>(".tc-card"))?.focus();
}

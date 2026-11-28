import { isLight, mix, readable } from "./colors";
import { COLOR_FAMILIES, type ColorFamily, themeProfile } from "./themeProfile";
import { invoke } from "../platform/invoke";
import { open } from "@tauri-apps/plugin-dialog";
import { dialogFrame, dialogHeader } from "../ui/dialogParts";
import { h } from "../ui/dom";
import { icon } from "../ui/icons";
import type { TerminalTheme } from "../platform/types";
import { t, tn } from "../i18n";

/** A color family in the current language. */
const familyName = (f: ColorFamily) => t(`family.${f}`);

/** The colors of a card preview. */
interface PreviewColors {
  bg: string; side: string; sideRule: string; bar: string; fg: string;
  warn: string; dim: string; err: string; cursor: string;
}

/** A theme's id, name and colors: the key of its cached preview colors and card. */
const themeKeys = new WeakMap<TerminalTheme, string>();
function themeKey(t: TerminalTheme): string {
  let k = themeKeys.get(t);
  if (k === undefined) {
    k = JSON.stringify([t.id, t.name, t.background, t.foreground, t.cursor ?? null, t.palette]);
    themeKeys.set(t, k);
  }
  return k;
}

/** Preview colors, computed once for each theme id and its colors. */
const previewCache = new Map<string, PreviewColors>();
function previewColors(t: TerminalTheme): PreviewColors {
  const key = themeKey(t);
  const hit = previewCache.get(key);
  if (hit) return hit;
  const light = isLight(t.background);
  const ink = (c: string) => (light ? readable(c, [t.background], 7) : c);
  const fg = ink(t.foreground);
  const c: PreviewColors = {
    bg: t.background,
    side: mix(t.background, t.foreground, 0.06),
    sideRule: `1px solid ${mix(t.background, t.foreground, 0.16)}`,
    bar: mix(t.background, t.foreground, 0.35),
    fg,
    warn: ink(t.palette[3]),
    dim: readable(mix(fg, t.background, 0.3), [t.background], 4.5),
    err: ink(t.palette[1]),
    cursor: t.cursor ?? t.foreground,
  };
  previewCache.set(key, c);
  return c;
}

/** A small Skiff window in the theme's colors: a sidebar and a few terminal lines. */
function preview(t: TerminalTheme): HTMLElement {
  const c = previewColors(t);
  const box = h("span", "tc-preview");
  box.style.background = c.bg;
  const side = h("span", "tc-side");
  side.style.background = c.side;
  side.style.borderRight = c.sideRule;
  for (const w of [70, 50, 60]) {
    const bar = h("span", "tc-bar");
    bar.style.width = `${w}%`;
    bar.style.background = c.bar;
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
  line([["~/skiff main $ cargo test", c.fg]]);
  line([["   Compiling skiffd", c.fg]]);
  line([["warning", c.warn], [": unused var", c.dim]]);
  line([["error", c.err], ["[E0308] types", c.fg]]);
  const cursor = h("span", "tc-cursor");
  cursor.style.background = c.cursor;
  const last = h("span", "tc-line");
  const prompt = h("span", "", "$ ");
  prompt.style.color = c.fg;
  last.append(prompt, cursor);
  term.appendChild(last);
  box.append(side, term);
  return box;
}

/** A representative hue for each color family's swatch. */
const SWATCH: Record<string, string> = {
  Neutral: "#8a8f98", Beige: "#e4d2ac", Red: "#e5484d", Orange: "#f07630", Yellow: "#e8c547", Green: "#46a758",
  Cyan: "#2fb7c4", Blue: "#3e7bfa", Purple: "#8e5cf6", Pink: "#e45fa8",
  Unclassified: "#8a8f98",
};

export type ThemeMode = "Light" | "Dark";

export interface ThemeFilters {
  query: string;
  family: string | null;
  mode?: ThemeMode | null;
}

const pickerFilters: ThemeFilters = { query: "", family: null, mode: null };

interface CardOpts {
  /** The id shown as active. */
  active: string | null;
  pick: (id: string | null) => void;
  /** A first card that clears the choice, e.g. "Same as app". */
  none?: { label: string; theme: TerminalTheme | undefined };
  reload?: () => Promise<TerminalTheme[]>;
  filters?: ThemeFilters;
  /** Cards a lower scope uses on screen, with a label that names it: "Project: skiff". */
  scopes?: { id: string; label: string }[];
}

/** A grid of theme cards and the call that brings it up to date in place. */
export interface ThemeBrowser {
  el: HTMLElement;
  /** New themes, active id or scope labels. Cards whose theme did not change stay the same elements. */
  update(themes: TerminalTheme[], active: string | null, scopes?: { id: string; label: string }[]): void;
}

/** A grid of theme cards. A click picks; nothing previews on hover. */
export function themeGrid(themes: TerminalTheme[], o: CardOpts): HTMLElement {
  return themeBrowser(themes, o).el;
}

/**
 * A grid of theme cards, built once. A filter shows and hides cards, a pick moves
 * the marks, and an update builds only the cards of new or changed themes.
 */
export function themeBrowser(themes: TerminalTheme[], o: CardOpts): ThemeBrowser {
  const filters = o.filters ?? { query: "", family: null };
  const browser = h("div", "tc-browser");
  const tools = h("div", "tc-tools");
  const search = h("input", "tc-search");
  search.type = "search";
  search.placeholder = t("theme.search");
  search.setAttribute("aria-label", t("theme.searchAria"));
  search.value = filters.query;
  // One swatch per color family in the list. "All" clears the filter.
  const swatches = h("div", "tc-swatches");
  swatches.setAttribute("role", "group");
  swatches.setAttribute("aria-label", t("theme.color"));
  const swatch = (value: ColorFamily | null) => {
    const b = h("button", "tc-swatch" + (value ? "" : " all"), value ? "" : t("theme.all"));
    b.type = "button";
    b.title = value ? familyName(value) : t("theme.allColors");
    b.setAttribute("aria-label", b.title);
    b.setAttribute("aria-pressed", String(value === filters.family));
    if (value) b.style.setProperty("--sw", SWATCH[value]);
    b.addEventListener("click", () => {
      filters.family = value;
      for (const s of swatches.children) s.setAttribute("aria-pressed", String(s === b));
      draw();
    });
    return b;
  };
  let families = "";
  const drawSwatches = () => {
    const present = new Set(themes.map((t) => themeProfile(t).family));
    if (filters.family && !present.has(filters.family as ColorFamily)) filters.family = null;
    const now = COLOR_FAMILIES.filter((f) => present.has(f)).join() + "|" + filters.family;
    if (now === families) return;
    families = now;
    swatches.replaceChildren(swatch(null), ...COLOR_FAMILIES.filter((f) => present.has(f)).map(swatch));
  };
  drawSwatches();
  // Light and Dark. A second click on the pressed one clears the filter.
  const modes = h("div", "tc-modes");
  modes.setAttribute("role", "group");
  modes.setAttribute("aria-label", t("theme.mode"));
  for (const value of ["Light", "Dark"] as const) {
    const b = h("button", "tc-mode", t(value === "Light" ? "theme.light" : "theme.dark"));
    b.type = "button";
    b.setAttribute("aria-pressed", String(filters.mode === value));
    b.addEventListener("click", () => {
      filters.mode = filters.mode === value ? null : value;
      for (const m of modes.children) m.setAttribute("aria-pressed", String(m === b && filters.mode === value));
      draw();
    });
    modes.appendChild(b);
  }
  tools.append(search, modes, swatches);
  const status = h("div", "tc-status");
  status.setAttribute("role", "status");
  const grid = h("div", "tc-grid");
  /** Built cards by theme key, label and badge. Each one keeps its theme id. */
  const built = new Map<string, HTMLButtonElement>();
  /** Each card's foot and the spans that always lead it: the signature, the name and the badge. */
  const feet = new WeakMap<HTMLButtonElement, { foot: HTMLElement; lead: HTMLElement[] }>();
  /** The active mark and the scope labels of one card, set in place. */
  const mark = (b: HTMLButtonElement) => {
    const id = b.dataset.theme || null;
    const on = o.active === id;
    b.classList.toggle("active", on);
    b.setAttribute("aria-pressed", String(on));
    const { foot, lead } = feet.get(b)!;
    const tail: HTMLElement[] = [];
    if (on) tail.push(h("span", "tc-state", t("theme.active")));
    for (const s of o.scopes ?? []) if (s.id === id) tail.push(h("span", "tc-in-scope", s.label));
    foot.replaceChildren(...lead, ...tail);
  };
  const card = (theme: TerminalTheme, id: string | null, label: string, badge: string) => {
    const key = `${id === null ? "none" : "theme"}\n${themeKey(theme)}\n${label}\n${badge}`;
    const had = built.get(key);
    if (had) return had;
    const on = o.active === id;
    const b = h("button", "tc-card" + (on ? " active" : ""));
    b.type = "button";
    b.dataset.theme = id ?? "";
    b.setAttribute("aria-pressed", String(on));
    b.appendChild(preview(theme));
    const foot = h("span", "tc-foot");
    const signature = h("span", "tc-signature");
    signature.style.background = themeProfile(theme).color;
    signature.setAttribute("aria-hidden", "true");
    const lead = [signature, h("span", "tc-name", label), h("span", "tc-badge", badge)];
    foot.append(...lead);
    feet.set(b, { foot, lead });
    if (on) foot.appendChild(h("span", "tc-state", t("theme.active")));
    for (const s of o.scopes ?? []) if (s.id === id) foot.appendChild(h("span", "tc-in-scope", s.label));
    b.appendChild(foot);
    b.addEventListener("click", () => {
      o.active = id;
      // Update the marks in place. Keep the filter, scroll and focused card.
      for (const c of built.values()) mark(c);
      o.pick(id);
    });
    built.set(key, b);
    return b;
  };
  /** The cards in list order: the same elements on every call while their themes stay the same. */
  let order: HTMLButtonElement[] = [];
  const draw = () => {
    const words = filters.query.toLowerCase().trim().split(/\s+/);
    const next: HTMLButtonElement[] = [];
    if (o.none?.theme) next.push(card(o.none.theme, null, o.none.label, t("theme.default")));
    let shown = 0;
    for (const theme of themes) {
      const p = themeProfile(theme);
      const c = card(theme, theme.id, theme.name, familyName(p.family));
      // A color matches by its English id and by its name in the current language.
      const text = `${theme.name} ${p.family} ${familyName(p.family)}`.toLowerCase();
      const match = words.every((word) => text.includes(word))
        && (!filters.family || filters.family === p.family)
        && (!filters.mode || filters.mode === p.mode);
      c.hidden = !match;
      if (match) shown++;
      next.push(c);
    }
    // Only a new, changed or removed theme changes the grid's children.
    if (next.length !== order.length || next.some((c, i) => c !== order[i])) {
      grid.replaceChildren(...next);
      const keep = new Set(next);
      for (const [k, c] of built) if (!keep.has(c)) built.delete(k);
      order = next;
    }
    status.textContent = tn("theme.count", themes.length, { n: shown });
  };
  search.addEventListener("input", () => {
    filters.query = search.value;
    draw();
  });
  if (o.reload) {
    const tool = (label: string, paths: string) => {
      const b = h("button", "tc-tool");
      b.type = "button";
      b.title = label;
      b.setAttribute("aria-label", label);
      const svg = icon(paths);
      svg.setAttribute("width", "16");
      svg.setAttribute("height", "16");
      b.appendChild(svg);
      return b;
    };
    const add = tool(t("theme.import"), '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"></path>');
    const folder = tool(t("theme.openFolder"), '<path d="M3 6h6l2 2h10v11H3z"></path>');
    const reload = async () => { themes = await o.reload!(); drawSwatches(); draw(); };
    add.addEventListener("click", async () => {
      try {
        const paths = await open({ multiple: true, title: t("theme.importTitle") });
        if (!paths) return;
        add.disabled = true;
        const errors = await invoke<string[]>("import_themes", { paths: Array.isArray(paths) ? paths : [paths] });
        await reload();
        if (errors.length) status.textContent = errors.join("; ");
      } catch (e) { status.textContent = String(e); }
      finally { add.disabled = false; }
    });
    folder.addEventListener("click", () => {
      invoke("open_theme_folder").catch((e) => { status.textContent = String(e); });
      // Pick up files the user drops in the folder when they come back.
      window.addEventListener("focus", () => { if (browser.isConnected) reload().catch(() => {}); }, { once: true });
    });
    tools.append(add, folder);
  }
  tools.append(status);
  browser.append(tools, grid);
  draw();
  return {
    el: browser,
    update(list, active, scopes) {
      const marks = active !== o.active || JSON.stringify(scopes ?? []) !== JSON.stringify(o.scopes ?? []);
      o.active = active;
      o.scopes = scopes;
      if (list !== themes) {
        themes = list;
        drawSwatches();
      }
      draw();
      if (marks) for (const c of built.values()) mark(c);
    },
  };
}

const KEYS: Record<string, "left" | "right" | "up" | "down" | "first" | "last"> = {
  ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", Home: "first", End: "last",
};

/** The card an arrow key lands on. Up and down keep to the nearest column; the ends stop. */
export function nextCard(
  cards: HTMLElement[],
  at: number,
  key: "left" | "right" | "up" | "down" | "first" | "last",
  /** Where a card's line starts. Default: its own top. */
  top: (c: HTMLElement) => number = (c) => c.getBoundingClientRect().top,
): number {
  if (key === "first") return 0;
  if (key === "last") return cards.length - 1;
  if (key === "left") return Math.max(0, at - 1);
  if (key === "right") return Math.min(cards.length - 1, at + 1);
  const rect = cards[at].getBoundingClientRect();
  const y = top(cards[at]);
  const dir = key === "down" ? 1 : -1;
  let best = at;
  let bestRow = Infinity;
  let bestDx = Infinity;
  for (const [i, c] of cards.entries()) {
    const r = c.getBoundingClientRect();
    const dy = (top(c) - y) * dir;
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

/** Theme picker: a modal of cards. `scope` says what the theme applies to. A click applies and closes. */
export function pickTheme(title: string, scope: string, themes: TerminalTheme[], o: CardOpts): void {
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
      filters: o.filters ?? pickerFilters,
      pick: (id) => {
        close();
        o.pick(id);
      },
    }),
  );
  panel.append(head, h("div", "tc-scope", scope), body);
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
    if (!step || e.ctrlKey || e.altKey || e.metaKey || !(e.target instanceof HTMLElement)) return;
    const cards = [...body.querySelectorAll<HTMLButtonElement>(".tc-card:not([hidden])")];
    const search = body.querySelector<HTMLInputElement>(".tc-search");
    // Down from the search goes into the cards, at the active theme. Left and right stay in the text.
    if (e.target === search) {
      if (step !== "down" || !cards.length) return;
      e.preventDefault();
      document.body.classList.add("kbd");
      (cards.find((c) => c.classList.contains("active")) ?? cards[0]).focus();
      return;
    }
    if (!e.target.closest(".tc-card")) return;
    // Arrows move between cards, not the panel's scroll. Up from the top row goes back to the search.
    e.preventDefault();
    document.body.classList.add("kbd");
    const at = Math.max(0, cards.indexOf(document.activeElement as HTMLButtonElement));
    const next = nextCard(cards, at, step);
    if (step === "up" && next === at && search) search.focus();
    else cards[next]?.focus();
  });
  body.querySelector<HTMLInputElement>(".tc-search")?.focus();
}

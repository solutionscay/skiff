import type { AgentInfo, Project, Worktree } from "./types";
import { launchIcon } from "./agentIcon";

interface Opts {
  /** Enabled agents, in order. */
  agents: () => AgentInfo[];
  /** `agent` null starts a plain shell. */
  start: (p: Project, w: Worktree, agent: AgentInfo | null) => void;
  openSettings: () => void;
  /** Layouts for a blank split, with a small picture of each. */
  splits: () => { name: string; glyph: SVGElement }[];
  split: (p: Project, w: Worktree, i: number) => void;
  onClose: () => void;
}

/** The + menu on a worktree: enabled agents, then Shell. Digits pick, Esc closes. */
export function createLaunchMenu(o: Opts) {
  const menu = document.createElement("div");
  menu.id = "launch-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  document.body.appendChild(menu);

  let anchor: HTMLElement | null = null;

  function close(refocus = true) {
    if (menu.hidden) return;
    menu.hidden = true;
    anchor?.setAttribute("aria-expanded", "false");
    anchor = null;
    if (refocus) o.onClose();
  }

  function item(key: string, name: string, hint: string, run: () => void, cls = "", icon?: HTMLElement) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "lm-item " + cls;
    b.setAttribute("role", "menuitem");
    const k = document.createElement("span");
    k.className = "lm-key mono";
    k.textContent = key;
    const n = document.createElement("span");
    n.className = "lm-name";
    n.textContent = name;
    const h = document.createElement("span");
    h.className = "lm-hint mono";
    h.textContent = hint;
    b.append(k);
    if (icon) b.append(icon);
    b.append(n, h);
    b.addEventListener("click", () => {
      close(false);
      run();
    });
    return b;
  }

  /** `at` is the + button it hangs off, or the point of a right-click. */
  function open(at: HTMLElement | { x: number; y: number }, p: Project, w: Worktree) {
    const el = at instanceof HTMLElement ? at : null;
    if (el && !menu.hidden && anchor === el) return close();
    close(false);
    anchor = el;
    el?.setAttribute("aria-expanded", "true");
    const title = document.createElement("div");
    title.className = "lm-title";
    title.textContent = "START IN " + (w.branch ?? "detached").toUpperCase();
    const items: (AgentInfo | null)[] = [...o.agents(), null];
    const rows = items.map((a, i) =>
      item(String(i + 1), a?.id ?? "Shell", "", () => o.start(p, w, a), "", launchIcon(a)),
    );
    const head = document.createElement("div");
    head.className = "lm-head";
    head.textContent = "NEW GROUP, THEN A SESSION PER PANE";
    const splits = o.splits().map((s, i) => {
      const g = document.createElement("span");
      g.className = "lm-glyph";
      g.append(s.glyph);
      return item("", s.name, "", () => o.split(p, w, i), "", g);
    });
    const settings = item("", "Agent settings…", "", () => o.openSettings(), "lm-settings");
    menu.replaceChildren(title, ...rows, head, ...splits, settings);

    menu.hidden = false;
    const h = menu.offsetHeight;
    const r = el ? el.getBoundingClientRect() : null;
    const left = r ? r.right : at instanceof HTMLElement ? 0 : at.x;
    const top = r ? r.top : at instanceof HTMLElement ? 0 : at.y;
    menu.style.left = `${Math.max(0, Math.min(left, window.innerWidth - menu.offsetWidth))}px`;
    menu.style.top = `${Math.max(0, Math.min(top, window.innerHeight - h))}px`;
    rows[0]?.focus();
  }

  menu.addEventListener("keydown", (e) => {
    const items = [...menu.querySelectorAll<HTMLButtonElement>(".lm-item")];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") close();
    else if (e.key === "ArrowDown") items[(i + 1) % items.length]?.focus();
    else if (e.key === "ArrowUp") items[(i - 1 + items.length) % items.length]?.focus();
    else if (/^[1-9]$/.test(e.key)) items.find((b) => b.querySelector(".lm-key")?.textContent === e.key)?.click();
    else return;
    e.preventDefault();
    e.stopPropagation();
  });
  document.addEventListener("mousedown", (e) => {
    if (!menu.hidden && !menu.contains(e.target as Node) && e.target !== anchor && !anchor?.contains(e.target as Node)) close(false);
  });

  return { open, close };
}

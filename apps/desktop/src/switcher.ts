import { fuzzy } from "./model";
import type { SessionState } from "./types";

export interface SwitchItem {
  kind: "command" | "session" | "worktree";
  /** Project accent. */
  color: string;
  primary: string;
  secondary: string;
  state?: SessionState;
  meta: string;
  /** What the query matches against. */
  text: string;
  run: () => void;
}

const MAX_ROWS = 50;

/** The command palette: commands, sessions and worktrees in one list. Arrows move, Enter runs, Esc closes. */
export function createSwitcher(items: () => SwitchItem[], onClose: () => void) {
  const overlay = document.createElement("div");
  overlay.id = "switcher";
  overlay.hidden = true;
  overlay.innerHTML = `
    <div class="sw-panel" role="dialog" aria-label="Switch to session or worktree">
      <div class="sw-input">
        <label for="sw-q" class="visually-hidden">Jump to project, worktree, or session</label>
        <input id="sw-q" type="text" autocomplete="off" spellcheck="false"
          role="combobox" aria-expanded="true" aria-controls="sw-list" aria-autocomplete="list"
          placeholder="Jump to project, worktree, or session" />
        <kbd>Esc</kbd>
      </div>
      <div id="sw-list" role="listbox" aria-label="Matches"></div>
      <div class="sw-foot"><span><kbd>↑</kbd> <kbd>↓</kbd> move</span><span><kbd>Enter</kbd> focus</span><span><kbd>Esc</kbd> close</span></div>
    </div>`;
  document.body.appendChild(overlay);

  const input = overlay.querySelector("input") as HTMLInputElement;
  const list = overlay.querySelector("#sw-list") as HTMLDivElement;
  let shown: SwitchItem[] = [];
  let active = 0;

  function filter() {
    const q = input.value.trim();
    const all = items();
    if (!q) {
      shown = all.slice(0, MAX_ROWS);
    } else {
      shown = all
        .map((item, i) => ({ item, i, score: fuzzy(q, item.text) }))
        .filter((x) => x.score >= 0)
        .sort((a, b) => b.score - a.score || a.i - b.i)
        .slice(0, MAX_ROWS)
        .map((x) => x.item);
    }
    active = 0;
    render();
  }

  function render() {
    list.replaceChildren();
    if (shown.length === 0) {
      const empty = document.createElement("div");
      empty.className = "sw-empty";
      empty.textContent = "No match";
      list.appendChild(empty);
      input.removeAttribute("aria-activedescendant");
      return;
    }
    shown.forEach((item, i) => {
      const row = document.createElement("div");
      row.id = `sw-opt-${i}`;
      row.className = "sw-row" + (i === active ? " active" : "");
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(i === active));
      row.style.setProperty("--pc", item.color);
      row.innerHTML = `<span class="sq"></span><span class="kind"></span><span class="primary"></span><span class="secondary"></span><span class="meta"></span>`;
      const kind = row.querySelector(".kind") as HTMLElement;
      if (item.state) {
        kind.innerHTML = `<span class="dot ${item.state}"></span>`;
        kind.append(item.state);
        kind.classList.add("state", item.state);
      } else {
        kind.textContent = item.kind;
      }
      (row.querySelector(".primary") as HTMLElement).textContent = item.primary;
      (row.querySelector(".secondary") as HTMLElement).textContent = item.secondary;
      (row.querySelector(".meta") as HTMLElement).textContent = item.meta;
      row.addEventListener("mousedown", (e) => e.preventDefault());
      row.addEventListener("click", () => choose(i));
      list.appendChild(row);
    });
    input.setAttribute("aria-activedescendant", `sw-opt-${active}`);
    list.children[active]?.scrollIntoView({ block: "nearest" });
  }

  function move(delta: number) {
    if (shown.length === 0) return;
    active = (active + delta + shown.length) % shown.length;
    render();
  }

  function choose(i: number) {
    const item = shown[i];
    close();
    item?.run();
  }

  function open() {
    overlay.hidden = false;
    input.value = "";
    filter();
    input.focus();
  }

  function close() {
    if (overlay.hidden) return;
    overlay.hidden = true;
    onClose();
  }

  input.addEventListener("input", filter);
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter") choose(active);
    else if (e.key === "Escape") close();
    else return;
    e.preventDefault();
    e.stopPropagation();
  });
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });

  return {
    open,
    close,
    toggle: () => (overlay.hidden ? open() : close()),
    isOpen: () => !overlay.hidden,
  };
}

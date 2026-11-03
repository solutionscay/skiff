/**
 * Settings focus: a redraw that keeps scroll and focus, and the plain arrow
 * keys. DOM only. settings.ts passes in the tab, the redraw and the tab steps.
 */

/** The card finder of the theme grid. settings.ts passes it in. */
type NextCard = (stops: HTMLElement[], at: number, dir: "up" | "down" | "left" | "right", top: (c: HTMLElement) => number) => number;

/** Draws the page again and keeps the scroll, the focused field, button and row. */
export function redrawKeeping(root: HTMLElement, tab: string, draw: () => void) {
  // A rebuild keeps the list where it was scrolled.
  const scroll = root.querySelector(".set-main")?.scrollTop ?? 0;
  // So does the focused field: a save on blur finishes after the click that moved focus to the next field.
  const a = document.activeElement;
  const field = a instanceof HTMLInputElement && root.contains(a) ? { label: a.getAttribute("aria-label"), at: a.selectionStart, to: a.selectionEnd } : null;
  // Disk use redraws under its buttons after a count, and skiffd activity under a sort head on a sort or
  // a refresh. The keys stay on the same button, else the first one on.
  const button = a instanceof HTMLButtonElement && a.closest(".set-main") && (tab === "disk" || a.classList.contains("set-sort") || a.classList.contains("set-sink")) ? a.dataset.key ?? a.textContent : null;
  // A selected skiffd session row keeps the keys too; an ended one hands them to the current row.
  const row = a instanceof HTMLElement && a.classList.contains("set-daemon-row") ? a.dataset.id ?? null : null;
  // A Disk use tree row keeps the keys on the current row: a move, an open or a count.
  const diskRow = a instanceof HTMLElement && a.classList.contains("set-disk-row") && tab === "disk";
  draw();
  if (row !== null) (root.querySelector<HTMLElement>(`.set-daemon-row[data-id="${CSS.escape(row)}"]`) ?? root.querySelector<HTMLElement>(`.set-daemon-row[tabindex="0"]`))?.focus();
  if (diskRow) root.querySelector<HTMLElement>(`.set-disk-row[tabindex="0"]`)?.focus();
  if (button !== null) {
    const stops = [...root.querySelectorAll<HTMLButtonElement>(".set-main button:not(:disabled, .set-x)")];
    (stops.find((b) => (b.dataset.key ?? b.textContent) === button) ?? stops[0])?.focus();
  }
  const main = root.querySelector(".set-main");
  if (main) main.scrollTop = scroll;
  if (field?.label) {
    const next = [...root.querySelectorAll<HTMLInputElement>("input")].find((i) => i.getAttribute("aria-label") === field.label);
    next?.focus();
    if (next && field.at !== null && field.to !== null) next.setSelectionRange(field.at, field.to);
  }
}

/**
 * Plain arrows. On a tab, Up/Down change the section and Right goes into it.
 * In a section, Up/Down go to the nearest control above or below, and Left/Right
 * along the row. Text keeps its own keys: Left/Right move the caret, and a name
 * list lets Up/Down out only from its first or last line.
 */
export function arrowKeys(root: HTMLElement, hooks: { stepTab(dir: 1 | -1): void; enterSection(into: boolean): void; nextCard: NextCard }) {
  const { stepTab, enterSection, nextCard } = hooks;
  root.addEventListener("keydown", (e) => {
    if (e.ctrlKey || e.altKey || e.metaKey || e.shiftKey || !e.key.startsWith("Arrow")) return;
    const el = document.activeElement as HTMLElement | null;
    if (!el || !root.contains(el)) return;
    const up = e.key === "ArrowUp";
    const vertical = up || e.key === "ArrowDown";
    if (el.classList.contains("set-tab")) {
      if (vertical) stepTab(up ? -1 : 1);
      else if (e.key === "ArrowRight") enterSection(true);
      else return;
      return e.preventDefault();
    }
    if (!el.closest(".set-main")) return;
    if (el instanceof HTMLSelectElement) return;
    const text = el instanceof HTMLInputElement && el.type !== "range";
    // A text field moves its caret, and lets Left or Right out only at its start or end.
    if (!vertical && (el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && !text))) return;
    if (!vertical && text) {
      const edge = e.key === "ArrowLeft" ? 0 : el.value.length;
      if (el.selectionStart !== edge || el.selectionEnd !== edge) return;
    }
    if (el instanceof HTMLTextAreaElement) {
      const v = el.value;
      if (up ? v.lastIndexOf("\n", el.selectionStart - 1) >= 0 : v.indexOf("\n", el.selectionEnd) >= 0) return;
    }
    // The current skiffd session row is a stop too: Left/Right go between it and its Reveal.
    const stops = [...root.querySelectorAll<HTMLElement>(".set-main :is(button, input, select, textarea, .set-daemon-row[tabindex='0']):not(:disabled, .set-x)")];
    const at = stops.indexOf(el);
    if (at < 0) return;
    e.preventDefault();
    // A control's line is its row: a small button sits lower in its cell than a field.
    const top = (c: HTMLElement) => (c.closest(".set-row") ?? c).getBoundingClientRect().top;
    let to = nextCard(stops, at, up ? "up" : e.key === "ArrowDown" ? "down" : e.key === "ArrowLeft" ? "left" : "right", top);
    // Left and Right keep to the row.
    if (!vertical && Math.abs(top(stops[to]) - top(el)) > 1) to = at;
    // Left from the start of a row goes back to the tab.
    if (to === at && e.key === "ArrowLeft") return enterSection(false);
    const next = stops[to];
    next.focus();
    // Into a text field from the right, the caret starts at its end.
    if (next instanceof HTMLInputElement && next.type !== "range" && e.key === "ArrowLeft") next.setSelectionRange(next.value.length, next.value.length);
  });
}

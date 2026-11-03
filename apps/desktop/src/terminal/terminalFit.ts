import type { Terminal } from "@xterm/xterm";

interface Grid { cols: number; rows: number }
interface Size { width: number; height: number }

/** The same xterm internals used by FitAddon, kept at this boundary. */
interface FitCore {
  _renderService: { dimensions: { css: { cell: Size } }; clear(): void };
  _charSizeService: { hasValidSize: boolean; measure(): void };
}
const coreOf = (term: Terminal) => (term as unknown as { _core: FitCore })._core;

interface Box {
  element: HTMLElement;
  size?: Size;
  font?: string;
  paddingX: number;
  paddingY: number;
}

/** Observe the terminal's content box, so a fit never forces a size read. */
export class TerminalFitter {
  private boxes = new Map<Terminal, Box>();
  private cached = new WeakMap<Terminal, Box>();
  private targets = new Map<Element, Box>();
  private fontRevision = 0;
  private observer: ResizeObserver;

  constructor(requestFit: () => void) {
    this.observer = new ResizeObserver((entries) => {
      let changed = false;
      for (const entry of entries) {
        const box = this.targets.get(entry.target);
        if (!box) continue;
        const { width, height } = entry.contentRect;
        if (box.size?.width === width && box.size.height === height) continue;
        box.size = { width, height };
        changed = true;
      }
      if (changed) requestFit();
    });
    // A loaded font can replace a fallback without changing the option string.
    document.fonts.addEventListener("loadingdone", () => {
      this.fontRevision++;
      requestFit();
    });
  }

  sync(terms: Terminal[]) {
    const shown = new Set(terms);
    for (const [term, box] of this.boxes) {
      if (shown.has(term) && term.element?.parentElement === box.element) continue;
      this.observer.unobserve(box.element);
      this.targets.delete(box.element);
      this.boxes.delete(term);
      box.size = undefined;
    }
    for (const term of terms) {
      const element = term.element?.parentElement;
      if (!element || this.boxes.has(term)) continue;
      const previous = this.cached.get(term);
      const box: Box = previous?.element === element ? previous : { element, paddingX: 0, paddingY: 0 };
      this.cached.set(term, box);
      this.boxes.set(term, box);
      this.targets.set(element, box);
      this.observer.observe(element);
    }
  }

  /** Read every grid before applying any of them. Cell dimensions are xterm's cache. */
  read(term: Terminal, zoom: number): Grid | undefined {
    const box = this.boxes.get(term);
    if (!box?.size || box.size.width <= 0 || box.size.height <= 0 || !term.element) return;
    const core = coreOf(term);
    const o = term.options;
    const font = JSON.stringify([o.fontFamily, o.fontSize, o.fontWeight, o.fontWeightBold,
      o.letterSpacing, o.lineHeight, zoom, window.devicePixelRatio, this.fontRevision]);
    if (box.font !== font || !core._charSizeService.hasValidSize) {
      core._charSizeService.measure();
      const style = window.getComputedStyle(term.element);
      box.paddingX = parseInt(style.paddingLeft) + parseInt(style.paddingRight);
      box.paddingY = parseInt(style.paddingTop) + parseInt(style.paddingBottom);
      if (core._charSizeService.hasValidSize) box.font = font;
    }
    if (!core._charSizeService.hasValidSize) return;
    const cell = core._renderService.dimensions.css.cell;
    if (!(cell.width > 0 && cell.height > 0)) return;
    // Match FitAddon's rounding, minimum grid and default scrollbar width.
    const scrollbar = o.scrollback === 0 ? 0 : (o.overviewRuler?.width || 14);
    const cols = Math.max(2, Math.floor((Math.floor(box.size.width) - box.paddingX - scrollbar) / cell.width));
    const rows = Math.max(1, Math.floor((Math.floor(box.size.height) - box.paddingY) / cell.height));
    if (Number.isFinite(cols) && Number.isFinite(rows)) return { cols, rows };
  }
}

/** Keep FitAddon's full renderer clear, with no second geometry or character read. */
export function resizeTerminal(term: Terminal, grid: Grid) {
  if (term.cols === grid.cols && term.rows === grid.rows) return;
  const core = coreOf(term);
  const chars = core._charSizeService;
  const measure = chars.measure;
  // xterm's synchronous resize measures the same character again after reflow.
  // read() has already measured it if the font or zoom changed. Restore the
  // method even on failure, so xterm's font and DPR listeners keep working.
  chars.measure = () => {};
  try {
    core._renderService.clear();
    term.resize(grid.cols, grid.rows);
  } finally {
    chars.measure = measure;
  }
}

/** A color picker drawn in the page: the webview's own color input does not open on Linux. */

type Hsv = [number, number, number];

function toHsv(hex: string): Hsv {
  const n = parseInt(hex.replace("#", ""), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255);
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  let h = 0;
  if (d) h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [(h * 60 + 360) % 360, max ? d / max : 0, max];
}

function toHex([h, s, v]: Hsv): string {
  const f = (n: number) => {
    const k = (n + h / 60) % 6;
    return Math.round((v - v * s * Math.max(0, Math.min(k, 4 - k, 1))) * 255)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${f(5)}${f(3)}${f(1)}`;
}

/** Drag on `el` and report x and y as 0..1. */
function drag(el: HTMLElement, on: (x: number, y: number) => void) {
  const at = (e: PointerEvent) => {
    const r = el.getBoundingClientRect();
    on(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)));
  };
  el.addEventListener("pointerdown", (e) => {
    el.setPointerCapture(e.pointerId);
    at(e);
    const move = (m: PointerEvent) => at(m);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", () => el.removeEventListener("pointermove", move), { once: true });
  });
}

export function pickColor(o: { title: string; start: string; action: string; submit: (hex: string) => Promise<void>; onClose?: () => void }): void {
  let hsv = toHsv(/^#[0-9a-f]{6}$/i.test(o.start) ? o.start : "#7ee0cb");
  const el = (tag: string, cls: string, text?: string) => {
    const x = document.createElement(tag);
    x.className = cls;
    if (text) x.textContent = text;
    return x;
  };
  const overlay = el("div", "confirm-overlay");
  const panel = el("div", "confirm-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-modal", "true");
  panel.setAttribute("aria-label", o.title);
  const area = el("div", "cp-area");
  const areaDot = el("div", "cp-dot");
  area.append(areaDot);
  const hue = el("div", "cp-hue");
  const hueDot = el("div", "cp-bar");
  hue.append(hueDot);
  const readout = el("div", "cp-readout");
  const chip = el("span", "cp-chip");
  const code = el("input", "cp-code") as HTMLInputElement;
  code.spellcheck = false;
  code.maxLength = 7;
  code.setAttribute("aria-label", "Hex color");
  readout.append(chip, code);
  const error = el("div", "prompt-error");
  error.setAttribute("role", "alert");
  const foot = el("div", "confirm-foot");
  const cancel = el("button", "confirm-cancel", "Cancel") as HTMLButtonElement;
  const act = el("button", "confirm-act go", o.action) as HTMLButtonElement;
  cancel.type = act.type = "button";
  foot.append(cancel, act);
  panel.append(el("div", "confirm-title", o.title), area, hue, readout, error, foot);
  overlay.append(panel);
  document.body.append(overlay);

  const paint = (skipCode = false) => {
    const hex = toHex(hsv);
    area.style.background = `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, transparent), ${toHex([hsv[0], 1, 1])}`;
    areaDot.style.left = `${hsv[1] * 100}%`;
    areaDot.style.top = `${(1 - hsv[2]) * 100}%`;
    hueDot.style.left = `${(hsv[0] / 360) * 100}%`;
    chip.style.background = hex;
    if (!skipCode) code.value = hex;
  };
  drag(area, (x, y) => ((hsv = [hsv[0], x, 1 - y]), paint()));
  drag(hue, (x) => ((hsv = [x * 359.99, hsv[1], hsv[2]]), paint()));
  code.addEventListener("input", () => {
    const v = "#" + code.value.replace(/[^0-9a-f]/gi, "").slice(0, 6);
    if (/^#[0-9a-f]{6}$/i.test(v)) {
      hsv = toHsv(v);
      paint(true);
    }
  });

  let busy = false;
  const close = () => {
    overlay.remove();
    o.onClose?.();
  };
  const run = async () => {
    if (busy) return;
    busy = act.disabled = true;
    try {
      await o.submit(toHex(hsv));
      close();
    } catch (e) {
      busy = act.disabled = false;
      error.textContent = String(e);
    }
  };
  act.addEventListener("click", () => void run());
  cancel.addEventListener("click", close);
  overlay.addEventListener("mousedown", (e) => e.target === overlay && close());
  overlay.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter" && document.activeElement !== cancel) {
      e.preventDefault();
      void run();
    }
  });
  paint();
  act.focus();
}

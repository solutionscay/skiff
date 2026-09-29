/** DOM helpers: element builders, icons, the terminal host. */
import type { Project } from "./types";

export const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export const host = $<HTMLDivElement>("terminal-host");

/** Terminals not in the layout wait here, hidden, with their scrollback. */
export const park = document.createElement("div");

park.className = "park";

host.appendChild(park);

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  if (text !== undefined) el.textContent = text;
  return el;
}

export function button(cls: string, text: string, onClick: () => void): HTMLButtonElement {
  const b = h("button", cls, text);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

export function toBytes(m: unknown): Uint8Array {
  if (m instanceof ArrayBuffer) return new Uint8Array(m);
  if (m instanceof Uint8Array) return m;
  if (Array.isArray(m)) return Uint8Array.from(m as number[]);
  return new TextEncoder().encode(String(m));
}

const trimmed = new Map<string, Promise<string>>();

/**
 * The icon with its transparent border cut off, centered in a square at 2x
 * the shown size. All icons then fill the same box. Done once per image.
 */
function trimIcon(src: string, size: number): Promise<string> {
  const key = size + " " + src;
  let done = trimmed.get(key);
  if (!done) {
    done = new Promise<string>((resolve) => {
      const img = new Image();
      img.onload = () => {
        try {
          const w = img.naturalWidth || 128;
          const ht = img.naturalHeight || 128;
          const c = document.createElement("canvas");
          c.width = w;
          c.height = ht;
          const g = c.getContext("2d", { willReadFrequently: true })!;
          g.drawImage(img, 0, 0, w, ht);
          const px = g.getImageData(0, 0, w, ht).data;
          let x0 = w, y0 = ht, x1 = -1, y1 = -1;
          for (let y = 0; y < ht; y++)
            for (let x = 0; x < w; x++)
              if (px[(y * w + x) * 4 + 3] > 16) {
                if (x < x0) x0 = x;
                if (x > x1) x1 = x;
                if (y < y0) y0 = y;
                if (y > y1) y1 = y;
              }
          if (x1 < 0) return resolve(src);
          const bw = x1 - x0 + 1, bh = y1 - y0 + 1;
          const out = document.createElement("canvas");
          out.width = out.height = size * 2;
          const k = (size * 2) / Math.max(bw, bh);
          const o = out.getContext("2d")!;
          o.imageSmoothingQuality = "high";
          o.drawImage(c, x0, y0, bw, bh, (size * 2 - bw * k) / 2, (size * 2 - bh * k) / 2, bw * k, bh * k);
          resolve(out.toDataURL("image/png"));
        } catch {
          resolve(src);
        }
      };
      img.onerror = () => resolve(src);
      img.src = src;
    });
    trimmed.set(key, done);
  }
  return done;
}

export function projectIcon(p: Project, size: number): HTMLImageElement {
  const img = h("img", "project-icon");
  img.width = size;
  img.height = size;
  img.alt = "";
  img.draggable = false;
  // Hidden until trimmed, so the rail does not jump from one size to the next.
  img.style.visibility = "hidden";
  void trimIcon(p.icon!, size).then((src) => {
    img.src = src;
    img.style.visibility = "";
  });
  return img;
}

export function branchIcon(): SVGSVGElement {
  const t = document.createElement("template");
  t.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="6" cy="5" r="2"></circle><circle cx="6" cy="19" r="2"></circle><circle cx="18" cy="8" r="2"></circle><path d="M6 7v10M18 10c0 4-6 3-10 7"></path></svg>`;
  return t.content.firstChild as SVGSVGElement;
}

export function plusIcon(): SVGSVGElement {
  const t = document.createElement("template");
  t.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>`;
  return t.content.firstChild as SVGSVGElement;
}

export function chevron(open: boolean): SVGSVGElement {
  const t = document.createElement("template");
  t.innerHTML = `<svg class="chev${open ? "" : " closed"}" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"></path></svg>`;
  return t.content.firstChild as SVGSVGElement;
}

export function icon(paths: string): SVGSVGElement {
  const t = document.createElement("template");
  t.innerHTML = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" aria-hidden="true">${paths}</svg>`;
  return t.content.firstChild as SVGSVGElement;
}

export function showError(e: unknown) {
  console.error(e);
  $("daemon-label").textContent = String(e);
}

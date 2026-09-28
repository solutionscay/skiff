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

export function projectIcon(p: Project, size: number): HTMLImageElement {
  const img = h("img", "project-icon");
  img.src = p.icon!;
  img.width = size;
  img.height = size;
  img.alt = "";
  img.draggable = false;
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

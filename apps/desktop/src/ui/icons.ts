/** SVG builders for shared interface icons. */
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

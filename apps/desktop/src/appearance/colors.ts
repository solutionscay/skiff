export type Rgb = [number, number, number];
export const rgb = (hex: string): Rgb => {
  const n = parseInt(hex.replace("#", "").slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
export const DEFAULT_THEME = "builtin:foot";

export const mix = (a: string, b: string, t: number) => {
  const x = rgb(a), y = rgb(b);
  return "#" + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("");
};

/** Relative luminance in linear sRGB. */
export const luminance = (hex: string) => {
  const [r, g, b] = rgb(hex).map((v) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

export const contrast = (a: string, b: string) => {
  const x = luminance(a), y = luminance(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
};

/** The polarity with the most available contrast. */
export const isLight = (bg: string) => contrast("#000000", bg) > contrast("#ffffff", bg);

export function readable(c: string, on: string[], ratio: number): string {
  const end = isLight(on[0]) ? "#000000" : "#ffffff";
  for (let i = 0; i <= 100; i++) {
    const candidate = mix(c, end, i / 100);
    if (on.every((bg) => contrast(candidate, bg) >= ratio)) return candidate;
  }
  return end;
}

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

/** Hue in degrees, saturation and lightness from 0 to 1. */
function hsl(hex: string): [number, number, number] {
  const [r, g, b] = rgb(hex).map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2, d = max - min;
  if (!d) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  const h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}

/**
 * A vivid color: saturation 0.55 or more, lightness 0.30 to 0.92. Warm hues
 * (20° to 50°) need 0.78: there a pale or muted color reads as tan or beige.
 */
export function isVivid(hex: string): boolean {
  const [h, s, l] = hsl(hex);
  return l >= 0.3 && l <= 0.92 && s >= (h >= 20 && h <= 50 ? 0.78 : 0.55);
}

/** The colors for sample faces: the vivid ones, else all of them (repeats, never gray). */
export function vividPalette(colors: string[]): string[] {
  const vivid = colors.filter(isVivid);
  return vivid.length ? vivid : colors;
}

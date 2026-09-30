export type Rgb = [number, number, number];
export const rgb = (hex: string): Rgb => {
  const n = parseInt(hex.replace("#", "").slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
export const HARBOR = "builtin:harbor";

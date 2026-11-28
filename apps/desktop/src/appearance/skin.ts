import { reapplyAppTheme } from "./appTheme";

/** The app frame. Color themes only set palette variables. Bubble keeps a light frame on a dark theme, so a change applies the theme again. */
export const skins = [{ id: "default", label: "skin.default" }, { id: "bubble", label: "skin.bubble" }] as const;

export function applySkin(id?: string | null) {
  document.documentElement.dataset.skin = skins.find((skin) => skin.id === id)?.id ?? "default";
  reapplyAppTheme();
}

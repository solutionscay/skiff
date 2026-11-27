/** The app frame. Color themes only set palette variables. */
export const skins = [{ id: "default", label: "skin.default" }, { id: "bubble", label: "skin.bubble" }] as const;

export function applySkin(id?: string | null) {
  document.documentElement.dataset.skin = skins.find((skin) => skin.id === id)?.id ?? "default";
}

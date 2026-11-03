import { t } from "../i18n";
import type { MenuEntry } from "../ui/menu";

export const ICON_OUT = "indicators-square-arrow-out-up-right";
export const ICON_FOLDER = "documents-folder-open";

/** The "Show in file manager" entry. Each caller keeps its own action, icon and gate. */
export const revealEntry = (run: () => void, o: { icon?: string; hint?: string } = {}): MenuEntry =>
  ({ icon: o.icon ?? ICON_OUT, label: t("menu.showInFileManager"), ...(o.hint ? { hint: o.hint } : {}), run });

/** The "Copy path" entry. */
export const copyPathEntry = (run: () => void, hint?: string): MenuEntry =>
  ({ icon: "code-copy", label: t("menu.copyPath"), ...(hint ? { hint } : {}), run });

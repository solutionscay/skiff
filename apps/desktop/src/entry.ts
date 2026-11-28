/**
 * The page's entry. It sets the language before any UI module loads, so
 * words that a module builds when it loads, such as the palette and the
 * Images drop-down, come out in that language. A change to the language
 * shows when the window opens again.
 */
import { invoke } from "./platform/invoke";
import { applySkin } from "./appearance/skin";
import { setLanguage } from "./i18n";
import { setTimeDisplay } from "./i18n/time";
import type { Appearance } from "./platform/types";

void (async () => {
  const look = await invoke<Appearance>("get_appearance").catch(() => null);
  applySkin(look?.skin);
  setLanguage(look?.language ?? navigator.language);
  setTimeDisplay(look?.time_zone, look?.clock);
  await import("./main");
})();

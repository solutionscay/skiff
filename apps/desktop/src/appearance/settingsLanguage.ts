/**
 * The Language and Menu rows of Settings › Appearance. Rows only: settings.ts
 * keeps the state and saves each choice. The window reads both when it opens.
 */
import { h } from "../ui/dom";
import { picker } from "../ui/controls";
import { catalogFor, language, languageName, languages, systemLanguage, t } from "../i18n";

export type MenuLayout = { available: boolean; chosen: string | null; auto: string; current: string };

/** "On restart" until the window opens again. */
const restartCell = (pending: boolean) => h("span", "c-mode", pending ? t("settings.onRestart") : "");

/** Linux: the GNOME header bar or the menu bar. GTK sets it when the window opens. */
export function menuLayout(m: MenuLayout, pick: (v: string | null) => void): HTMLElement {
  const row = h("div", "set-row set-open set-agent on");
  const auto = t(m.auto === "header-bar" ? "settings.menuAutoHeader" : "settings.menuAutoMenu");
  const cell = h("span", "c-field");
  cell.append(picker({
    label: t("settings.menuLayout"),
    value: m.chosen ?? "",
    entries: () => [{ value: "", label: auto }, { value: "header-bar", label: t("settings.headerBar") }, { value: "menu-bar", label: t("settings.menuBar") }],
    pick: (v) => pick(v || null),
  }));
  row.append(h("span", "c-name", t("settings.menu")), cell, restartCell((m.chosen ?? m.auto) !== m.current));
  return row;
}

/** The app's language: Auto, then each catalog. A set code picks its catalog: "es-MX" is Español, "fr" is English. */
export function languageRow(langSet: string | null, pick: (code: string | null) => void): HTMLElement {
  const row = h("div", "set-row set-open set-agent on");
  const chosen = langSet === null ? null : catalogFor(langSet);
  const cell = h("span", "c-field");
  cell.append(picker({
    label: t("settings.language"),
    value: chosen ?? "",
    entries: () => [{ value: "", label: t("settings.languageAuto", { name: languageName(systemLanguage()) }) }, ...languages().map((code) => ({ value: code, label: languageName(code) }))],
    pick: (v) => pick(v || null),
  }));
  row.append(h("span", "c-name", t("settings.language")), cell, restartCell((chosen ?? systemLanguage()) !== language()));
  return row;
}

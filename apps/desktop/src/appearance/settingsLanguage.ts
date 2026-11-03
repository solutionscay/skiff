/**
 * Settings › Language, and the Menu layout row of Settings › Appearance.
 * Pages only: settings.ts keeps the state and saves each choice.
 */
import { h } from "../ui/dom";
import { catalogFor, language, languageName, languages, locale, systemLanguage, t, upper, wordCount } from "../i18n";

export type MenuLayout = { available: boolean; chosen: string | null; auto: string; current: string };

/**
 * A row of choices that the window reads when it opens: Menu and Language.
 * `pending` shows "On restart" until the window opens again.
 */
function segRow<V>(name: string, aria: string, choices: [V, string][], chosen: V, pending: boolean, pick: (v: V) => void): HTMLElement {
  const row = h("div", "set-row set-opacity set-menu");
  const seg = h("div", "c-cmd set-seg");
  seg.setAttribute("role", "radiogroup");
  seg.setAttribute("aria-label", aria);
  for (const [value, label] of choices) {
    const b = h("button", "set-seg-item" + (chosen === value ? " on" : ""), label);
    b.type = "button";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(chosen === value));
    b.addEventListener("click", () => pick(value));
    seg.appendChild(b);
  }
  row.append(h("span", "c-name", name), seg, h("span", "c-status", pending ? t("settings.onRestart") : ""));
  return row;
}

/** Linux: the GNOME header bar or the menu bar. GTK sets it when the window opens. */
export function menuLayout(m: MenuLayout, pick: (v: string | null) => void): HTMLElement {
  const auto = t(m.auto === "header-bar" ? "settings.menuAutoHeader" : "settings.menuAutoMenu");
  const choices: [string | null, string][] = [[null, auto], ["header-bar", t("settings.headerBar")], ["menu-bar", t("settings.menuBar")]];
  const pending = (m.chosen ?? m.auto) !== m.current;
  return segRow(t("settings.menu"), t("settings.menuLayout"), choices, m.chosen, pending, pick);
}

/** The app's language, on its own page. */
export function languagePage(o: { langSet: string | null; error: string | null; pick(code: string | null): void }): HTMLElement {
  const main = h("div", "set-main");
  const intro = h("div", "set-head");
  intro.append(h("div", "set-h", t("settings.tab.language")), h("div", "set-sub", t("settings.appLanguage")));
  main.append(intro, languagePick(o.langSet, o.pick));
  if (o.error) {
    const err = h("div", "set-error", o.error);
    err.setAttribute("role", "alert");
    main.appendChild(err);
  }
  return main;
}

/**
 * The app's language as a table: Auto, then each catalog. LANGUAGE | CODE |
 * WORDS | STATE. The window reads the choice when it opens. A set code
 * lights the catalog it picks: "es-MX" lights Español, "fr" lights English.
 */
function languagePick(langSet: string | null, pick: (code: string | null) => void): HTMLElement {
  const table = h("div", "set-lang");
  table.setAttribute("role", "radiogroup");
  table.setAttribute("aria-label", t("settings.language"));
  const cols = h("div", "set-row set-cols");
  cols.append(...(["lang.colLanguage", "lang.colCode", "lang.colWords", "lang.colState"] as const).map((k, i) => h("span", i === 2 ? "c-num" : "", upper(t(k)))));
  table.append(cols);
  const chosen = langSet === null ? null : catalogFor(langSet);
  const choices: [string | null, string, string][] = [
    [null, t("settings.languageAuto", { name: languageName(systemLanguage()) }), navigator.language],
    ...languages().map((code): [string, string, string] => [code, languageName(code), code]),
  ];
  const n = (x: number) => x.toLocaleString(locale());
  for (const [value, name, code] of choices) {
    const on = chosen === value;
    // The catalog this row gives, and whether the window shows it now.
    const gives = value ?? systemLanguage();
    const shown = on && gives === language();
    const r = h("button", "set-row set-lang-row" + (on ? " cur" : ""));
    r.type = "button";
    r.dataset.key = `lang:${value ?? "auto"}`;
    r.setAttribute("role", "radio");
    r.setAttribute("aria-checked", String(on));
    const w = wordCount(gives);
    const state = h("span", "c-state");
    if (on) state.append(h("i", "lang-mk" + (shown ? "" : " pending")), h("span", "", t(shown ? "lang.inUse" : "settings.onRestart")));
    r.append(h("span", "c-name", name), h("span", "mono dim", code), h("span", "c-num", t("lang.words", { has: n(w.has), all: n(w.all) })), state);
    r.addEventListener("click", () => {
      if (!on) pick(value);
    });
    table.append(r);
  }
  return table;
}

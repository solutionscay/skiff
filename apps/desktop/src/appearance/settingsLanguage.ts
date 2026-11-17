/**
 * The Language, Time zone, Clock and Menu rows of Settings › Appearance. Rows only: settings.ts
 * keeps the state and saves each choice. The window reads both when it opens.
 */
import { h } from "../ui/dom";
import { picker } from "../ui/controls";
import { catalogFor, language, languageName, languages, systemLanguage, t } from "../i18n";
import { languageHour12, systemZone, validZone, zones } from "../i18n/time";

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

/** The UTC offset of a zone now: "UTC+01:00", "UTC". */
function offset(zone: string, now: number): string {
  const name = new Intl.DateTimeFormat("en-US", { timeZone: zone, timeZoneName: "longOffset" }).formatToParts(now).find((p) => p.type === "timeZoneName")?.value ?? "";
  return name.replace("GMT", "UTC");
}

/** The time zone times show in: Auto (the system's), then each IANA zone. Type to filter. A change shows at once. */
export function timeZoneRow(zoneSet: string | null, pick: (zone: string | null) => void): HTMLElement {
  const row = h("div", "set-row set-open set-agent on");
  const cell = h("span", "c-field");
  const chosen = zoneSet && validZone(zoneSet) ? zoneSet : null;
  cell.append(picker({
    label: t("settings.timeZone"),
    value: chosen ?? "",
    shows: chosen ?? t("settings.timeZoneAuto", { zone: systemZone() }),
    filter: true,
    entries: () => {
      const now = Date.now();
      return [{ value: "", label: t("settings.timeZoneAuto", { zone: systemZone() }) }, { sep: true }, ...zones().map((z) => ({ value: z, label: z, hint: offset(z, now) }))];
    },
    pick: (v) => pick(v || null),
  }));
  row.append(h("span", "c-name", t("settings.timeZone")), cell, h("span", "c-mode", ""));
  return row;
}

/** The clock: Auto (the app language's), 24-hour or 12-hour. A change shows at once. */
export function clockRow(clockSet: string | null, pick: (clock: string | null) => void): HTMLElement {
  const row = h("div", "set-row set-open set-agent on");
  const cell = h("span", "c-field");
  const auto = t("settings.clockAuto", { clock: t(languageHour12() ? "settings.clock12" : "settings.clock24") });
  cell.append(picker({
    label: t("settings.clock"),
    value: clockSet ?? "",
    entries: () => [{ value: "", label: auto }, { value: "24", label: t("settings.clock24") }, { value: "12", label: t("settings.clock12") }],
    pick: (v) => pick(v || null),
  }));
  row.append(h("span", "c-name", t("settings.clock")), cell, h("span", "c-mode", ""));
  return row;
}

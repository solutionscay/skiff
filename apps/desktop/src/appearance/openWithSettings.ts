import { h } from "../ui/dom";
import { picker, type PickEntry } from "../ui/controls";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { t, type MessageKey } from "../i18n";

export type OpenKey = "diff" | "text" | "markdown" | "html" | "image";
export type OpenSettings = Record<OpenKey, string> & { default_diff: string; peek: string[] };
/** One thing a row can pick. `command` is what `[open]` stores. */
export interface OpenChoice { label: string; command: string }
/** What each row can pick, found on this computer. */
export type OpenChoices = Partial<Record<OpenKey, { peek: OpenChoice[]; apps: OpenChoice[] }>>;

const OPEN_ROWS: { key: OpenKey; label: MessageKey }[] = [
  { key: "diff", label: "settings.open.diff" },
  { key: "text", label: "settings.open.text" },
  { key: "markdown", label: "settings.open.markdown" },
  { key: "html", label: "settings.open.html" },
  { key: "image", label: "settings.open.image" },
];

export interface OpenWithActs {
  /** Stores a command, and whether it opens in the peek. Empty: the default. */
  set(key: OpenKey, command: string, peek: boolean): void;
  /** Stores a program from the file picker. */
  setApp(key: OpenKey, path: string): void;
}

/**
 * Which apps Skiff hands things to. Each row is a picker: the default, the
 * tools and apps found on this computer, then the file picker. Where it opens
 * follows the choice. A command set by hand in projects.toml shows as it is.
 */
export function openWithPage(o: OpenSettings | null, found: OpenChoices, error: string | null, acts: OpenWithActs): HTMLElement {
  const main = h("div", "set-main");
  const intro = h("div", "set-head");
  intro.append(h("div", "set-h", t("settings.tab.open")));
  const cols = h("div", "set-row set-open set-cols");
  cols.append(h("span", "", t("settings.open.colShows")), h("span", "", t("settings.open.colWith")), h("span", "", t("settings.open.colOpensIn")));
  main.append(intro, cols);
  if (o) for (const { key, label: labelKey } of OPEN_ROWS) {
    const label = t(labelKey);
    const row = h("div", "set-row set-open set-agent on");
    const list = found[key] ?? { peek: [], apps: [] };
    const none = key === "diff" ? t("settings.open.gitDiff") : t("settings.open.defaultApp");
    const known = [...list.peek, ...list.apps].find((c) => c.command === o[key]);
    const entries = (): PickEntry[] => [
      { value: "", label: none },
      ...(list.peek.length ? [{ head: t("settings.open.inPeek") }, ...list.peek.map((c) => ({ value: c.command, label: c.label }))] : []),
      ...(list.apps.length ? [{ head: t("settings.open.inWindow") }, ...list.apps.map((c) => ({ value: c.command, label: c.label }))] : []),
      { sep: true as const },
      { label: t("settings.open.chooseApp"), run: () => void chooseApp().then((path) => { if (path) acts.setApp(key, path); }) },
    ];
    const cell = h("span", "c-field");
    cell.append(picker({
      label,
      value: o[key],
      shows: o[key] ? (known?.label ?? o[key]) : none,
      quiet: !o[key],
      entries,
      pick: (v) => acts.set(key, v, list.peek.some((c) => c.command === v)),
    }));
    // Diffs run in the peek unless the pick is an app with a window. Images always open in an app.
    const inPeek = key === "diff" ? !list.apps.some((c) => c.command === o[key]) : key !== "image" && o.peek.includes(key);
    row.append(h("span", "c-name", label), cell, h("span", "c-mode", t(inPeek ? "settings.open.peek" : "settings.open.app")));
    main.appendChild(row);
  }
  if (error) {
    const err = h("div", "set-error", error);
    err.setAttribute("role", "alert");
    main.appendChild(err);
  }
  return main;
}

async function chooseApp(): Promise<string | null> {
  const path = await openFile({ multiple: false, directory: false }).catch(() => null);
  return typeof path === "string" ? path : null;
}

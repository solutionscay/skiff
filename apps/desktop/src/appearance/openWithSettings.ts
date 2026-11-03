import { h } from "../ui/dom";
import { t, upper, type MessageKey } from "../i18n";

export type OpenKey = "diff" | "text" | "markdown" | "html" | "image";
export type OpenSettings = Record<OpenKey, string> & { default_diff: string; peek: string[] };

const OPEN_ROWS: { key: OpenKey; label: MessageKey }[] = [
  { key: "diff", label: "settings.open.diff" },
  { key: "text", label: "settings.open.text" },
  { key: "markdown", label: "settings.open.markdown" },
  { key: "html", label: "settings.open.html" },
  { key: "image", label: "settings.open.image" },
];


/** Which apps Skiff hands things to. */
export function openWithPage(o: OpenSettings | null, error: string | null, setOpen: (key: OpenKey, command: string) => Promise<void>, setOpenPeek: (key: OpenKey, peek: boolean) => Promise<void>): HTMLElement {
  const main = h("div", "set-main");
  const intro = h("div", "set-head");
  intro.append(h("div", "set-h", t("settings.tab.open")));
  const cols = h("div", "set-row set-open set-cols");
  cols.append(h("span", "c-name", upper(t("settings.open.colShows"))), h("span", "c-cmd", upper(t("settings.agents.colCommand"))), h("span", "c-mode", upper(t("settings.open.colOpensIn"))));
  main.append(intro, cols);
  if (o) for (const { key, label: labelKey } of OPEN_ROWS) {
    const label = t(labelKey);
    const row = h("div", "set-row set-open set-agent on");
    const cmd = h("input", "c-cmd mono");
    cmd.value = o[key];
    cmd.placeholder = key === "diff" ? o.default_diff : t("settings.open.defaultApp");
    cmd.spellcheck = false;
    cmd.setAttribute("aria-label", t("settings.open.commandAria", { kind: label }));
    cmd.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        cmd.blur();
      } else if (e.key === "Escape") {
        cmd.value = o[key];
        cmd.blur();
      }
    });
    cmd.addEventListener("blur", () => {
      if (cmd.value.trim() !== o[key].trim()) void setOpen(key, cmd.value);
    });
    row.append(h("span", "c-name", label), cmd);
    if (key === "diff") row.append(h("span", "c-mode", t("settings.open.peek")));
    else if (key === "image") row.append(h("span", "c-mode", t("settings.open.app")));
    else {
      const mode = h("select", "c-mode");
      mode.setAttribute("aria-label", t("settings.open.modeAria", { kind: label }));
      for (const [value, label] of [["app", t("settings.open.app")], ["peek", t("settings.open.peek")]]) {
        const option = h("option", "", label);
        option.value = value;
        mode.append(option);
      }
      mode.value = o.peek.includes(key) ? "peek" : "app";
      mode.addEventListener("change", () => void setOpenPeek(key, mode.value === "peek"));
      row.append(mode);
    }
    main.appendChild(row);
  }
  if (error) {
    const err = h("div", "set-error", error);
    err.setAttribute("role", "alert");
    main.appendChild(err);
  }
  return main;
}


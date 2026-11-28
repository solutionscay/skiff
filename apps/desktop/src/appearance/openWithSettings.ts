import { h } from "../ui/dom";
import { picker, textBox, type PickEntry } from "../ui/controls";
import { actionDialog } from "../ui/dialogParts";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { t, type MessageKey } from "../i18n";

export type OpenKey = "diff" | "text" | "markdown" | "html" | "image";
/** `[open.commands.<key>]`: a program Skiff runs with no shell. `input: "patch"`: the diff on its standard input. */
export interface OpenCommand { program: string; args: string[]; input: string; opens?: string }
export type OpenSettings = Record<OpenKey, string> & { default_diff: string; peek: string[]; commands?: Partial<Record<OpenKey, OpenCommand>> };
/** One thing a row can pick. `command` is what `[open]` stores, unless `run` is set: then `[open.commands]` stores `run`. */
interface OpenChoice { label: string; command: string; run?: OpenCommand }
/** What each row can pick, found on this computer. */
export type OpenChoices = Partial<Record<OpenKey, { peek: OpenChoice[]; apps: OpenChoice[] }>>;

const OPEN_ROWS: { key: OpenKey; label: MessageKey }[] = [
  { key: "diff", label: "settings.open.diff" },
  { key: "text", label: "settings.open.text" },
  { key: "markdown", label: "settings.open.markdown" },
  { key: "html", label: "settings.open.html" },
  { key: "image", label: "settings.open.image" },
];

interface OpenWithActs {
  /** Stores a command, and whether it opens in the peek. Empty: the default. */
  set(key: OpenKey, command: string, peek: boolean): void;
  /** Stores a program that Skiff runs with no shell. */
  setCommand(key: OpenKey, run: OpenCommand): void;
  /** Stores a program from the file picker. */
  setApp(key: OpenKey, path: string): void;
  /** Stores a program from the command editor. Rejects with the error to show. */
  saveCommand(key: OpenKey, run: OpenCommand): Promise<void>;
  /** Puts the row back to its default. Rejects with the error to show. */
  reset(key: OpenKey): Promise<void>;
}

/**
 * Which apps Skiff hands things to. Each row is a picker: the default, the
 * tools and apps found on this computer, then the file picker. Where it opens
 * follows the choice. A command set by hand in projects.toml shows as it is.
 */
/** The picker value of a program: one string per program, arguments and input. */
const runValue = (r: OpenCommand) => "run:" + JSON.stringify([r.program, r.args, r.input]);
/** A program as its words. */
const runWords = (r: OpenCommand) => [r.program, ...r.args].join(" ");

/** One word as the command field shows it: in quotes when it has a space or a quote. */
const quoteWord = (w: string) => (w !== "" && !/[\s'"]/.test(w) ? w : w.includes("'") ? `"${w}"` : `'${w}'`);
/** A program and its arguments as one line for the command field. */
export const commandLine = (r: OpenCommand) => [r.program, ...r.args].map(quoteWord).join(" ");

/** The signs a shell reads. Skiff runs the program with no shell, so a word that is only one of them is a fault. */
const SHELL_SIGNS = ["|", "||", "&", "&&", ";", "<", ">", ">>", "2>"];

/**
 * The command field as words: spaces part them, and single or double quotes
 * group. Throws the line to show for an empty field, an open quote or a
 * shell sign.
 */
export function commandWords(line: string): string[] {
  const words: string[] = [];
  let word: string | null = null;
  let quote: string | null = null;
  for (const c of line) {
    if (quote) {
      if (c === quote) quote = null;
      else word += c;
    } else if (c === "'" || c === '"') {
      quote = c;
      word ??= "";
    } else if (/\s/.test(c)) {
      if (word !== null) words.push(word);
      word = null;
    } else word = (word ?? "") + c;
  }
  if (quote) throw t("settings.open.cmdQuote");
  if (word !== null) words.push(word);
  if (!words.length || !words[0].trim()) throw t("settings.open.cmdEmpty");
  const sign = words.find((w) => SHELL_SIGNS.includes(w));
  if (sign) throw t("settings.open.cmdShell", { sign });
  return words;
}

/** The placeholder of each input: the word that takes the target or the file. */
const INPUT_HINT: Record<string, string> = { patch: "delta --side-by-side", target: "difft {target}", path: "code --wait {path}" };

/**
 * The command editor of one Open with row: the command line, what the
 * program reads (Diffs only) and where it opens (Text, Markdown and HTML).
 * Save stores it as `[open.commands.<key>]`; Reset to default clears the row.
 */
export function editCommand(key: OpenKey, label: string, current: OpenCommand | null, line: string, peek: boolean, acts: Pick<OpenWithActs, "saveCommand" | "reset">) {
  let input = current?.input ?? (key === "diff" ? (line.includes("{target}") ? "target" : "patch") : "path");
  if (key === "diff" ? input !== "patch" && input !== "target" : input !== "path") input = key === "diff" ? "patch" : "path";
  let opens = current?.opens ?? (peek ? "peek" : "app");
  const field = textBox({ label: t("settings.open.cmdField"), value: current ? commandLine(current) : line, placeholder: INPUT_HINT[input], mono: true });
  const row = (name: string, control: HTMLElement) => {
    const r = h("div", "prompt-from open-cmd-row");
    r.append(h("span", "open-cmd-name", name), control);
    return r;
  };
  const rows = [row(t("settings.open.cmdField"), field)];
  if (key === "diff") {
    const inputs = [{ value: "patch", label: t("settings.open.inputPatch") }, { value: "target", label: t("settings.open.inputTarget") }];
    const pick = (): HTMLElement => picker({
      label: t("settings.open.cmdInput"),
      value: input,
      entries: () => inputs,
      pick: (v) => {
        input = v;
        field.placeholder = INPUT_HINT[input];
        inputRow.replaceChild(pick(), inputRow.lastChild!);
      },
    });
    const inputRow = row(t("settings.open.cmdInput"), pick());
    rows.push(inputRow);
  }
  if (key !== "diff" && key !== "image") {
    const places = [{ value: "peek", label: t("settings.open.peek") }, { value: "app", label: t("settings.open.app") }];
    const pick = (): HTMLElement => picker({
      label: t("settings.open.colOpensIn"),
      value: opens,
      entries: () => places,
      pick: (v) => {
        opens = v;
        placeRow.replaceChild(pick(), placeRow.lastChild!);
      },
    });
    const placeRow = row(t("settings.open.colOpensIn"), pick());
    rows.push(placeRow);
  }
  actionDialog({
    title: t("settings.open.cmdTitle", { row: label }),
    content: rows,
    action: t("settings.open.save"),
    tone: "go",
    focus: field,
    also: { label: t("settings.open.reset"), run: () => acts.reset(key) },
    submit: async () => {
      const [program, ...args] = commandWords(field.value);
      const run: OpenCommand = { program, args, input, ...(key !== "diff" && key !== "image" ? { opens } : {}) };
      try {
        await acts.saveCommand(key, run);
      } catch (e) {
        throw String(e) === "program-missing" ? t("settings.open.cmdMissing", { program }) : e;
      }
    },
  });
}

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
    const run = o.commands?.[key];
    const value = run ? runValue(run) : o[key];
    const choiceValue = (c: OpenChoice) => (c.run ? runValue(c.run) : c.command);
    const all = [...list.peek, ...list.apps];
    const known = all.find((c) => choiceValue(c) === value);
    // Diffs run in the peek unless the pick is an app with a window. Images always open in an app.
    // A program's own place wins over the peek list.
    const inPeek = key === "diff" ? !list.apps.some((c) => c.command === o[key]) : key !== "image" && (run?.opens ? run.opens === "peek" : o.peek.includes(key));
    const entries = (): PickEntry[] => [
      { value: "", label: none },
      ...(list.peek.length ? [{ head: t("settings.open.inPeek") }, ...list.peek.map((c) => ({ value: choiceValue(c), label: c.label }))] : []),
      ...(list.apps.length ? [{ head: t("settings.open.inWindow") }, ...list.apps.map((c) => ({ value: choiceValue(c), label: c.label }))] : []),
      { sep: true as const },
      { label: t("settings.open.custom"), run: () => editCommand(key, label, null, "", inPeek, acts) },
      ...(value ? [{ label: t("settings.open.edit"), run: () => editCommand(key, label, run ?? null, run ? "" : o[key], inPeek, acts) }] : []),
      { label: t("settings.open.chooseApp"), run: () => void chooseApp().then((path) => { if (path) acts.setApp(key, path); }) },
    ];
    const cell = h("span", "c-field");
    cell.append(picker({
      label,
      value,
      shows: value ? (known?.label ?? (run ? runWords(run) : o[key])) : none,
      quiet: !value,
      entries,
      pick: (v) => {
        const c = all.find((c) => choiceValue(c) === v);
        if (c?.run) acts.setCommand(key, c.run);
        else acts.set(key, v, list.peek.some((c) => c.command === v));
      },
    }));
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

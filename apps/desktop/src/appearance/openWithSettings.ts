import { h } from "../ui/dom";
import { comboBox, type PickEntry } from "../ui/controls";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { t, type MessageKey } from "../i18n";

export type OpenKey = "diff" | "text" | "markdown" | "html" | "image";
/**
 * `[open.commands.<key>]`: a program Skiff runs with no shell. A diff program
 * with `{target}` takes what Git compares there; without it, it reads the
 * patch on its standard input.
 */
export interface OpenCommand { program: string; args: string[]; opens?: string; git_args?: string[] }
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
  /** Stores a typed program. Rejects with the error to show. */
  saveCommand(key: OpenKey, run: OpenCommand): Promise<void>;
  /** Stores a typed command line, and whether it opens in the peek. Empty: the default. Rejects with the error to show. */
  saveLine(key: OpenKey, line: string, peek: boolean): Promise<void>;
}

/**
 * Which apps Skiff hands things to. Each row is a command field with a list:
 * the default, the tools and apps found on this computer, then the file
 * picker. The field shows what runs. Where it opens follows the choice. A
 * command set by hand in client.toml shows as it is.
 */
/** The list value of a program: one string per program and arguments. */
const runValue = (r: OpenCommand) => "run:" + JSON.stringify([r.program, r.args]);
/** One word as the command field shows it: in quotes when it has a space or a quote. */
const quoteWord = (w: string) => (w !== "" && !/[\s'"]/.test(w) ? w : w.includes("'") ? `"${w}"` : `'${w}'`);
/** A program and its arguments as one line for the command field. */
export const commandLine = (r: OpenCommand) => [r.program, ...r.args].map(quoteWord).join(" ");
/** A program as it runs, as the field shows it: after the Git it reads from when it has Git options. */
const runLine = (r: OpenCommand) => (r.git_args?.length ? `git diff ${r.git_args.map(quoteWord).join(" ")} {target} | ` : "") + commandLine(r);

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

/** The characters a shell reads outside single quotes, as skiff-core's `shell_words` sees them. */
const SHELL_CHARS = ";&<>()$`\\*?[#~|\n";

/** Whether a saved command line needs the shell: Skiff runs it with `sh -c`, not as one program. */
export function shellLine(line: string): boolean {
  let quote: string | null = null;
  for (const c of line) {
    if (quote === "'") {
      if (c === "'") quote = null;
    } else if (quote === '"') {
      if (c === '"') quote = null;
      else if ("$`\\".includes(c)) return true;
    } else if (c === "'" || c === '"') quote = c;
    else if (SHELL_CHARS.includes(c)) return true;
  }
  return false;
}

/**
 * A diff command line that only feeds `git diff {target}` to one renderer,
 * as skiff-core's `patch_pipeline` reads it: `git diff OPTS {target} | PROGRAM
 * ARGS`, or `git -c core.pager=PROGRAM diff OPTS {target}`. Skiff runs it as
 * its own Git with OPTS and the renderer reading the patch. Anything else: null.
 */
export function patchPipeline(line: string): { git: string[]; renderer: string[] } | null {
  const parts = line.split("|");
  if (parts.length > 2 || shellLine(parts.join(" "))) return null;
  let git: string[], piped: string[] | null;
  try {
    git = commandWords(parts[0]);
    piped = parts.length === 2 ? commandWords(parts[1]) : null;
  } catch {
    return null;
  }
  let pager: string[] | null = null;
  if (git[0] !== "git") return null;
  if (git[1] === "-c" && git[2]?.startsWith("core.pager=")) {
    pager = git[2].slice("core.pager=".length).split(/\s+/).filter(Boolean);
    git = git.slice(3);
  } else git = git.slice(1);
  if (git[0] !== "diff" || git[git.length - 1] !== "{target}" || git.length < 2) return null;
  const opts = git.slice(1, -1);
  if (!opts.every((o) => o.startsWith("-") && !o.includes("{target}"))) return null;
  const renderer = piped && !pager ? piped : pager && !piped ? pager : null;
  if (!renderer?.length || renderer.some((w) => w === "" || w.includes("{") || w.includes("}"))) return null;
  return { git: opts, renderer };
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
    const choiceText = (c: OpenChoice) => (c.run ? runLine(c.run) : c.command);
    const all = [...list.peek, ...list.apps];
    const inPeekList = (c: OpenChoice) => list.peek.includes(c);
    // Diffs run in the peek unless the pick is an app with a window. Images always open in an app.
    // A program's own place wins over the peek list.
    const inPeek = key === "diff" ? !list.apps.some((c) => c.command === o[key]) : key !== "image" && (run?.opens ? run.opens === "peek" : o.peek.includes(key));
    const entries = (): PickEntry[] => [
      { value: "", label: none },
      ...(list.peek.length ? [{ head: t("settings.open.inPeek") }, ...list.peek.map((c) => ({ value: choiceValue(c), label: c.label }))] : []),
      ...(list.apps.length ? [{ head: t("settings.open.inWindow") }, ...list.apps.map((c) => ({ value: choiceValue(c), label: c.label }))] : []),
      { sep: true as const },
      { label: t("settings.open.chooseApp"), run: () => void chooseApp().then((path) => { if (path) acts.setApp(key, path); }) },
    ];
    const opens = run?.opens ?? (inPeek ? "peek" : "app");
    const saveRun = (r: OpenCommand) => acts.saveCommand(key, r).catch((e) => {
      throw String(e) === "program-missing" ? t("settings.open.cmdMissing", { program: r.program }) : e;
    });
    /** Typed text: empty is the default, a preset's text is the preset, else a program, or a shell line where the row has one. */
    const save = async (line: string) => {
      if (!line) return acts.saveLine(key, "", false);
      const c = all.find((c) => choiceText(c) === line);
      if (c) return c.run ? saveRun(c.run) : acts.saveLine(key, c.command, inPeekList(c));
      const pipe = key === "diff" ? patchPipeline(line) : null;
      if (pipe) return saveRun({ program: pipe.renderer[0], args: pipe.renderer.slice(1), ...(pipe.git.length ? { git_args: pipe.git } : {}) });
      if (!run && shellLine(o[key]) && shellLine(line)) return acts.saveLine(key, line, opens === "peek");
      const [program, ...args] = commandWords(line);
      return saveRun({ program, args, ...(key !== "diff" && key !== "image" ? { opens } : {}) });
    };
    const cell = h("span", "c-field");
    cell.append(comboBox({
      label,
      value: run ? runLine(run) : o[key],
      placeholder: none,
      mono: true,
      picked: value,
      entries,
      save,
      pick: (v) => {
        const c = all.find((c) => choiceValue(c) === v);
        if (c?.run) acts.setCommand(key, c.run);
        else acts.set(key, v, !!c && inPeekList(c));
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

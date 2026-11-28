import { h } from "../ui/dom";
import { picker, smallButton, textBox, type PickEntry } from "../ui/controls";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { t, type MessageKey } from "../i18n";

export type OpenKey = "diff" | "text" | "markdown" | "html" | "image";
/** `[open.commands.<key>]`: a program Skiff runs with no shell. `input: "patch"`: the diff on its standard input. */
export interface OpenCommand { program: string; args: string[]; input: string; opens?: string; git_args?: string[] }
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
  /** Stores a command line that needs the shell, from the command editor. Rejects with the error to show. */
  saveLine(key: OpenKey, line: string, peek: boolean): Promise<void>;
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

/** What the editor of one row starts with. */
interface Draft { line: string; input: string; opens: string; git: string[]; shell: boolean }

/**
 * What a row's editor shows. A program shows as it is. A saved command line
 * that Skiff runs as Git and a renderer shows as the renderer with the patch
 * on standard input, and keeps its Git options. Any other line that needs the
 * shell shows as it is, marked so.
 */
export function draftOf(key: OpenKey, current: OpenCommand | null, line: string, peek: boolean): Draft {
  const opens = current?.opens ?? (peek ? "peek" : "app");
  if (current) {
    const ok = key === "diff" ? current.input === "patch" || current.input === "target" : current.input === "path";
    return { line: commandLine(current), input: ok ? current.input : key === "diff" ? "patch" : "path", opens, git: current.git_args ?? [], shell: false };
  }
  const pipe = key === "diff" ? patchPipeline(line) : null;
  if (pipe) return { line: commandLine({ program: pipe.renderer[0], args: pipe.renderer.slice(1), input: "patch" }), input: "patch", opens, git: pipe.git, shell: false };
  const input = key === "diff" ? (line.includes("{target}") ? "target" : "patch") : "path";
  return { line, input, opens, git: [], shell: shellLine(line) };
}

/**
 * The editor of one Open with row, in the table under the row: the command,
 * what a diff program reads (Diffs) and where a file opens (Text, Markdown and
 * HTML). Save stores a program as `[open.commands.<key>]`, or a line that
 * needs the shell as it is; Reset to default clears the row. Enter in the
 * field saves. Esc and Cancel close it and give the keys back to the picker.
 */
export function editCommand(key: OpenKey, label: string, draft: Draft, acts: Pick<OpenWithActs, "saveCommand" | "saveLine" | "reset">, close: (saved: boolean) => void): HTMLElement {
  let { input, opens } = draft;
  const box = h("div", "open-edit");
  box.setAttribute("role", "group");
  box.setAttribute("aria-label", t("settings.open.cmdTitle", { row: label }));
  const field = textBox({ label: t("settings.open.cmdField"), value: draft.line, placeholder: INPUT_HINT[input], mono: true, enter: () => void save() });
  const cmdRow = h("div", "set-row open-edit-row");
  const cmdCell = h("span", "c-field c-wide");
  cmdCell.append(field);
  cmdRow.append(h("span", "c-name", t("settings.open.cmdField")), cmdCell);
  const error = h("div", "set-error");
  error.setAttribute("role", "alert");
  error.hidden = true;
  const note = h("div", "set-note", t("settings.open.inShell"));
  note.hidden = !draft.shell;
  const choice = (options: { value: string; label: string }[], value: () => string, set: (v: string) => void) => {
    const cell = h("span", "c-choice");
    const draw = () => cell.replaceChildren(...options.map((o) => {
      const b = smallButton(o.label, () => {
        set(o.value);
        draw();
        (cell.children[options.indexOf(o)] as HTMLElement | undefined)?.focus?.();
      });
      b.setAttribute("aria-pressed", String(o.value === value()));
      return b;
    }));
    draw();
    return cell;
  };
  let name = "";
  let pick: HTMLElement = h("span", "c-choice");
  if (key === "diff") {
    name = t("settings.open.cmdInput");
    pick = choice([{ value: "patch", label: t("settings.open.inputPatch") }, { value: "target", label: t("settings.open.inputTarget") }], () => input, (v) => {
      input = v;
      field.placeholder = INPUT_HINT[input];
    });
  } else if (key !== "image") {
    name = t("settings.open.colOpensIn");
    pick = choice([{ value: "peek", label: t("settings.open.peek") }, { value: "app", label: t("settings.open.app") }], () => opens, (v) => { opens = v; });
  }
  const reset = smallButton(t("settings.open.reset"), () => void run(() => acts.reset(key)));
  const cancel = smallButton(t("settings.open.cancel"), () => !busy && close(false));
  const ok = smallButton(t("settings.open.save"), () => void save(), { go: true });
  const actCell = h("span", "c-acts");
  actCell.append(reset, cancel, ok);
  const lastRow = h("div", "set-row open-edit-row");
  lastRow.append(h("span", "c-name", name), pick, actCell);
  box.append(cmdRow, error, note, lastRow);

  let busy = false;
  const fail = (e: unknown) => {
    error.textContent = String(e);
    error.hidden = false;
    field.setAttribute("aria-invalid", "true");
  };
  async function run(job: () => Promise<void>) {
    if (busy) return;
    busy = reset.disabled = cancel.disabled = ok.disabled = true;
    error.hidden = true;
    field.removeAttribute("aria-invalid");
    try {
      await job();
      close(true);
    } catch (e) {
      fail(e);
    } finally {
      busy = reset.disabled = cancel.disabled = ok.disabled = false;
    }
  }
  function save() {
    const line = field.value.trim();
    const peek = key !== "diff" && key !== "image" ? { opens } : {};
    const pipe = key === "diff" ? patchPipeline(line) : null;
    let run1: OpenCommand;
    if (pipe) run1 = { program: pipe.renderer[0], args: pipe.renderer.slice(1), input: "patch", ...(pipe.git.length ? { git_args: pipe.git } : {}) };
    else if (draft.shell && line && shellLine(line)) return run(() => acts.saveLine(key, line, opens === "peek"));
    else {
      let words: string[];
      try {
        words = commandWords(line);
      } catch (e) {
        return fail(e);
      }
      const [program, ...args] = words;
      run1 = { program, args, input, ...peek, ...(input === "patch" && draft.git.length ? { git_args: draft.git } : {}) };
    }
    const program = run1.program;
    return run(() => acts.saveCommand(key, run1).catch((e) => {
      throw String(e) === "program-missing" ? t("settings.open.cmdMissing", { program }) : e;
    }));
  }
  box.addEventListener("keydown", (e: KeyboardEvent) => {
    if (e.key !== "Escape" || busy) return;
    e.preventDefault();
    e.stopPropagation();
    close(false);
  });
  return box;
}

export function openWithPage(o: OpenSettings | null, found: OpenChoices, error: string | null, acts: OpenWithActs): HTMLElement {
  const main = h("div", "set-main");
  const intro = h("div", "set-head");
  intro.append(h("div", "set-h", t("settings.tab.open")));
  const cols = h("div", "set-row set-open set-cols");
  cols.append(h("span", "", t("settings.open.colShows")), h("span", "", t("settings.open.colWith")), h("span", "", t("settings.open.colOpensIn")));
  main.append(intro, cols);
  /** The open row editor: one at a time. */
  let editing: { box: HTMLElement; pick: HTMLElement } | null = null;
  const shut = () => {
    editing?.box.remove();
    editing = null;
  };
  const edit = (key: OpenKey, label: string, row: HTMLElement, pick: HTMLElement, draft: Draft) => {
    // Another row's editor closes, and its edits drop.
    shut();
    const box = editCommand(key, label, draft, acts, (saved) => {
      if (editing?.box === box) shut();
      // A save draws the page again: the new picker of the row takes the keys.
      const back = saved ? (globalThis.document?.querySelector<HTMLElement>(`.set-open .ctl-pick[data-key="${CSS.escape(label)}"]`) ?? pick) : pick;
      back.focus?.();
    });
    editing = { box, pick };
    row.after(box);
    (box.querySelector?.("input") as HTMLElement | null)?.focus();
  };
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
      { label: t("settings.open.custom"), run: () => edit(key, label, row, pick, draftOf(key, null, "", inPeek)) },
      ...(value ? [{ label: t("settings.open.edit"), run: () => edit(key, label, row, pick, draftOf(key, run ?? null, run ? "" : o[key], inPeek)) }] : []),
      { label: t("settings.open.chooseApp"), run: () => void chooseApp().then((path) => { if (path) acts.setApp(key, path); }) },
    ];
    const cell = h("span", "c-field");
    const pick = picker({
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
    });
    cell.append(pick);
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

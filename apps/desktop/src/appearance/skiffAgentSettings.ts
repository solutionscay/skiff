import { h } from "../ui/dom";
import { picker, smallButton, switchButton, textBox, type PickEntry } from "../ui/controls";
import { promptAction } from "../ui/confirm";
import { showError } from "../ui/alerts";
import type { AgentInfo } from "../platform/types";
import type { InstructionFile, SkiffAgentSettings, TaskType } from "../app/skiffAgent";
import { DEFAULT_NAME, mainName, skiffAgentChoice } from "../app/skiffAgent";
import { continueLine } from "../terminal/resume";
import { rowTable, sectionHead, type RowTableState } from "./rowTable";
import { t } from "../i18n";
import { badTaskTypeName, fileSize, modelError, modelFlag, TASK_TYPE_NAME } from "./taskTypes";
import { reveal } from "../platform/fileActions";
import { isRemote } from "../app/hosts";
import { S } from "../app/state";
import { avatarFinish, avatarScopes, setAvatarDialog, setAvatarFinish, skipperFace } from "../workspace/skipperFace";
import { FINISHES, parseFinish } from "./skipperAvatar";
import { scopeName } from "../workspace/model";
import { vividPalette } from "./colors";

/** A task type row as `set_skiff_agent` takes it. Null model: the agent's default. */
export interface TaskEdit {
  type: string;
  agent: string;
  model: string | null;
}

/** One edit, as `set_skiff_agent` takes it. */
export type SkiffAgentEdit =
  | { key: "name" | "skipper_name" | "agent" | "model" | "project_agent" | "project_model"; value: string | null }
  | { key: "continue_on_restore"; value: boolean }
  | { key: "add_task" | "task"; value: TaskEdit }
  | { key: "rename_task"; value: { from: string; to: string } }
  | { key: "remove_task"; value: string }
  | { key: "cache_days"; value: number };

/** What a Skipper's row does with its instruction file. */
interface InstructionActs {
  files: InstructionFile[];
  /** One error line under a row, by its file. */
  errors: Map<string, string>;
  /** False on a Host whose Skiff cannot send its files: the rows show, with no buttons. */
  editable: boolean;
  edit: (f: InstructionFile) => void;
  reset: (f: InstructionFile) => void;
}

/** Task types › Edit: opens a row's file. `errors`: one line under a row, by type. */
interface TypeFileActs {
  edit: (t: TaskType) => void;
  errors: Map<string, string>;
}

/** What the Skippers page does with its rows. */
export interface SkippersPageActs {
  save: (e: SkiffAgentEdit) => void;
  /** Draws the page again after a face changes. */
  redraw: () => void;
  files: InstructionActs;
}

/** What the Task types page does with its rows. */
export interface TaskTypesPageActs {
  saveTask: (e: SkiffAgentEdit) => Promise<string | null>;
  render: () => void;
  tasksTable: RowTableState;
  types: TypeFileActs;
}

/**
 * Settings › Appearance: the Finish of every face (plush, gummy or flat), an
 * app look kept in localStorage, then sample faces in it, in one row. It
 * changes every face at once. `seeds`: the strip's random faces, one per face;
 * the strip adds seeds as it needs more. `colors`: the theme colors the faces wear.
 */
export function finishRow(redraw: () => void, seeds: number[], colors: string[]): HTMLElement {
  const row = h("div", "set-row set-open set-agent on set-finish");
  row.append(
    h("span", "c-name", t("skipper.finish")),
    fieldCell(picker({
      label: t("skipper.finish"),
      value: avatarFinish(),
      entries: () => FINISHES.map((f) => ({ value: f, label: t(`skipper.finish.${f}`) })),
      pick: (v) => {
        setAvatarFinish(parseFinish(v));
        redraw();
      },
    })),
    finishSamples(seeds, colors),
  );
  return row;
}

/**
 * The sample strip: the Admiral, then random crew faces, in the finish, at
 * one row's size. As many faces as fit the strip's width, on one line; a resize
 * or a zoom adds or drops faces. Each crew face wears another vivid one of
 * `colors`, in an order the first seed shuffles.
 */
const SAMPLE_PX = 28;
function finishSamples(seeds: number[], colors: string[]): HTMLElement {
  const strip = h("div", "set-finish-faces");
  const admiral = skipperFace(null, SAMPLE_PX);
  strip.append(admiral);
  const fill = () => {
    if (!strip.isConnected) return seen.disconnect();
    const css = getComputedStyle(strip);
    const face = admiral.getBoundingClientRect().width;
    const gap = parseFloat(css.columnGap) || 0;
    const room = strip.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight);
    if (!face) return;
    const crew = Math.max(0, Math.floor((room + gap) / (face + gap)) - 1);
    while (seeds.length < Math.max(crew, 1)) seeds.push(1 + Math.floor(Math.random() * 0xffffff));
    const order = shuffled(vividPalette(colors), seeds[0]);
    while (strip.children.length > crew + 1) strip.lastElementChild!.remove();
    for (let i = strip.children.length - 1; i < crew; i++) {
      const f = skipperFace("", SAMPLE_PX, seeds[i]);
      if (order.length) f.style.setProperty("--pc", order[i % order.length]);
      strip.append(f);
    }
  };
  // The strip for its width, the Admiral for the zoom.
  const seen = new ResizeObserver(fill);
  seen.observe(strip);
  seen.observe(admiral);
  return strip;
}

/** `colors` without repeats, in an order that `seed` picks. */
function shuffled(colors: string[], seed: number): string[] {
  const out = [...new Set(colors.map((c) => c.toLowerCase()))];
  let x = seed | 0 || 1;
  for (let i = out.length - 1; i > 0; i--) {
    x ^= x << 13; x ^= x >>> 17; x ^= x << 5;
    const j = (x >>> 0) % (i + 1);
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

/** A cell a control fills. */
function fieldCell(el: HTMLElement): HTMLElement {
  const c = h("span", "c-field");
  c.append(el);
  return c;
}

/** The agents to pick from, after `first` (value "") when given. */
function agentEntries(choices: AgentInfo[], first: string | null): PickEntry[] {
  const list = choices.map((a) => ({ value: a.id, label: a.custom ? t("skipper.custom", { id: a.id }) : a.id }));
  return first === null ? list : [{ value: "", label: first }, ...list];
}

/**
 * The model of a row: one word in a field. Empty is the agent's default. An
 * agent with no model flag shows "in the command" in place of the field.
 */
function modelCell(agent: AgentInfo | undefined, model: string | null, label: string, save: (v: string | null) => void): HTMLElement {
  if (!modelFlag(agent)) return h("span", "c-dim", agent ? t("skipper.inCommand") : "");
  return fieldCell(textBox({
    label,
    value: model ?? "",
    placeholder: t("skipper.agentDefault"),
    mono: true,
    save: (v) => {
      const bad = modelError(v);
      if (bad) return void showError(bad);
      save(v || null);
    },
  }));
}

/** The STATUS cell: a square and the word. */
function fileState(f: InstructionFile): HTMLElement {
  const cls = f.status === "none" ? " none" : f.status === "template" ? " tpl" : "";
  const cell = h("span", "c-state" + cls);
  cell.append(h("span", "sq"), t(f.status === "none" ? "skipper.status.none" : f.status === "template" ? "skipper.status.template" : "skipper.status.edited"));
  if (f.status === "edited-template-changed") cell.append(h("span", "dim", t("skipper.status.templateChanged")));
  return cell;
}

/** The Instructions cell: the file's state. A hover shows the file. */
function fileCell(f: InstructionFile | undefined): HTMLElement {
  const cell = h("span", "c-file");
  if (!f) return cell;
  cell.title = f.file;
  cell.append(fileState(f));
  return cell;
}

/** Edit and Reset… of one file. Reset… shows on an edited file only. */
function fileActs(f: InstructionFile | undefined, acts: InstructionActs): HTMLElement {
  const box = h("span", "c-acts");
  if (!f || !acts.editable) return box;
  box.append(smallButton(t("skipper.edit"), () => acts.edit(f), { label: t("skipper.editFile", { file: f.file }) }));
  if (f.status.startsWith("edited")) box.append(smallButton(t("skipper.reset"), () => acts.reset(f), { label: t("skipper.resetFile", { file: f.file }) }));
  return box;
}

/** One error row under the row it is about. */
function errorLine(why: string): HTMLElement {
  const meta = h("div", "set-error", why);
  meta.setAttribute("role", "alert");
  return meta;
}

/**
 * Settings › Skippers: one table, one row for each Skipper: its face, name,
 * agent, model and instructions. The Admiral first, then the row all project
 * and fleet Skippers take their agent and model from, then each of them, in
 * tree order. A dim value comes from the all row. A project's `skipper_agent`
 * in projects.toml wins for its project Skipper: its row shows it bright, read
 * only.
 */
export function skippersPage(set: SkiffAgentSettings, agents: AgentInfo[], a: SkippersPageActs): HTMLElement {
  // Enabled agents and the user's own. A custom agent is on when the user added it.
  const choices = agents.filter((x) => x.enabled || x.custom);
  const chosen = skiffAgentChoice(agents, set.agent) ?? undefined;
  const main = mainName(set.name);
  const word = set.skipper_name || "Skipper";
  const fileOf = (scope: string | null) =>
    a.files.files.find((f) => (scope === null ? f.scope.kind === "skipper" : f.scope.kind !== "skipper" && f.scope.kind !== "type" && (f.scope.kind === "fleet" ? `fleet:${f.scope.name}` : f.scope.name) === scope));
  const errorsOf = (f: InstructionFile | undefined) => {
    const why = f && a.files.errors.get(f.file);
    return why ? [errorLine(why)] : [];
  };

  const table = h("div", "g-skip");
  table.setAttribute("role", "table");
  table.setAttribute("aria-label", t("settings.tab.skippers"));
  const cols = h("div", "set-row set-cols");
  cols.append(h("span", "", t("skipper.colSkipper")), h("span", "", t("settings.agents.colAgent")), h("span", "", t("tasks.colModel")), h("span", "", t("tasks.colInstructions")), h("span"));
  table.append(cols);

  const who = (face: HTMLElement, rest: HTMLElement) => {
    const c = h("span", "c-who");
    c.append(face, rest);
    return c;
  };

  // The Admiral.
  const adminFile = fileOf(null);
  const admiral = h("div", "set-row set-agent on");
  admiral.append(
    who(skipperFace(null, 28), textBox({ label: t("skipper.nameAria"), value: set.name ?? "", placeholder: DEFAULT_NAME, name: true, save: (v) => a.save({ key: "name", value: v || null }) })),
    fieldCell(picker({
      label: t("skipper.agentOf", { name: main }),
      value: chosen?.id ?? "",
      shows: chosen ? undefined : t("skipper.noAgentOnShort"),
      disabled: !choices.length,
      entries: () => agentEntries(choices, null),
      pick: (v) => a.save({ key: "agent", value: v }),
    })),
    modelCell(chosen, set.model, t("skipper.modelOf", { name: main }), (v) => a.save({ key: "model", value: v })),
    fileCell(adminFile),
    fileActs(adminFile, a.files),
  );
  table.append(admiral, ...errorsOf(adminFile));

  // Continue on restore: the Admiral's. The line it runs, and the switch. A custom agent has no line: off.
  const line = chosen ? continueLine(chosen.id, chosen.command) : null;
  const on = !!line && set.continue_on_restore;
  const cont = h("div", "set-row set-agent set-cont" + (on ? " on" : "") + (line ? "" : " missing"));
  cont.append(
    h("span", "c-who-sub", t("skipper.continue")),
    h("span", "c-line mono", line ?? (chosen ? t("skipper.noContinue", { id: chosen.id }) : t("skipper.noAgentOn"))),
    switchButton({ label: t(on ? "skipper.continueOn" : "skipper.continueOff"), on, disabled: !line, toggle: () => a.save({ key: "continue_on_restore", value: !on }) }),
  );
  table.append(cont);

  // Project and fleet Skippers, in the order of their files, else of their faces.
  const scopes = a.files.files.flatMap((f) => (f.scope.kind === "fleet" ? [`fleet:${f.scope.name}`] : f.scope.kind === "project" ? [f.scope.name] : []));
  const rows = scopes.length ? scopes : avatarScopes().filter((s) => s);
  table.append(sectionHead(t("skipper.projectSkippers"), undefined, String(rows.length)));
  const own = choices.find((x) => x.id === set.project_agent);
  const shared = own ?? chosen;
  const sharedRow = h("div", "set-row set-agent on");
  sharedRow.append(
    who(h("span", "c-kind-all", t("skipper.all")), textBox({ label: t("skipper.skipperNameAria"), value: set.skipper_name ?? "", placeholder: "Skipper", name: true, save: (v) => a.save({ key: "skipper_name", value: v || null }) })),
    fieldCell(picker({
      label: t("skipper.projectAgent"),
      value: own?.id ?? "",
      entries: () => agentEntries(choices, t("skipper.sameAs", { name: main })),
      pick: (v) => a.save({ key: "project_agent", value: v || null }),
    })),
    modelCell(shared, set.project_model, t("skipper.projectModel"), (v) => a.save({ key: "project_model", value: v })),
    h("span", "c-file"),
    h("span", "c-acts"),
  );
  table.append(sharedRow);

  for (const scope of rows) {
    const name = t(scope.startsWith("fleet:") ? "skipper.avatarFleet" : "skipper.avatarProject", { scope: scopeName(scope), skipper: word });
    const face = skipperFace(scope, 28);
    const label = h("span", "c-who-name", name);
    // The faces are kept in those folders on this computer: a remote Host's row has no picker.
    let cell: HTMLElement = who(face, label);
    if (!isRemote()) {
      const title = t("skipper.avatarTitle", { name });
      const b = h("button", "c-who");
      b.type = "button";
      b.title = title;
      b.setAttribute("aria-label", title);
      b.append(face, label);
      b.addEventListener("click", () => setAvatarDialog(scope, title, a.redraw));
      cell = b;
    }
    // A project's own agent starts with the agent's default model, while that agent is on.
    const over = S.projects.find((p) => p.name === scope)?.skipper_agent;
    const overAgent = over ? choices.find((x) => x.id === over) : undefined;
    const f = fileOf(scope);
    const r = h("div", "set-row set-agent on");
    r.append(
      cell,
      h("span", overAgent ? "c-own" : "c-dim", overAgent?.id ?? shared?.id ?? ""),
      h("span", "c-dim", overAgent || !set.project_model ? t("skipper.agentDefault") : set.project_model),
      fileCell(f),
      fileActs(f, a.files),
    );
    table.append(r, ...errorsOf(f));
  }

  return table;
}

/** Settings › Task types › Add task type…: asks for the name. The new row starts the Admiral's agent. */
export function addTaskButton(set: SkiffAgentSettings, agents: AgentInfo[], a: TaskTypesPageActs): HTMLElement {
  const choices = agents.filter((x) => x.enabled || x.custom);
  const chosen = skiffAgentChoice(agents, set.agent) ?? undefined;
  return smallButton(t("skipper.addTask"), () => {
    if (!choices.length) return void showError(t("skipper.noAgentOn"));
    promptAction({
      title: t("skipper.addTask"),
      placeholder: t("table.taskType.newName"),
      action: t("settings.names.addAction"),
      submit: async (name) => {
        if (!TASK_TYPE_NAME.test(name)) throw badTaskTypeName();
        const err = await a.saveTask({ key: "add_task", value: { type: name, agent: chosen?.id ?? choices[0].id, model: null } });
        if (err) throw err;
        a.render();
      },
    });
  }, { go: true });
}

/** Settings › Task types: one row for each task type. */
export function taskTable(set: SkiffAgentSettings, agents: AgentInfo[], a: TaskTypesPageActs): HTMLElement {
  const choices = agents.filter((x) => x.enabled || x.custom);
  const byId = (id: string) => agents.find((x) => x.id === id);
  const row = (task: TaskType, change: Partial<TaskEdit>): TaskEdit => ({ type: task.type, agent: task.agent, model: task.model, ...change });
  const table = rowTable<TaskType>(
    {
      grid: "g-tasks",
      labelledBy: "set-h-tasks",
      rows: set.tasks ?? [],
      id: (task) => task.type,
      rowClass: () => "set-agent on",
      cols: [
        { key: "type", head: t("tasks.colType"), name: t("tasks.type"), kind: "key", get: (task) => task.type, check: (v) => (TASK_TYPE_NAME.test(v) ? null : badTaskTypeName()) },
        {
          key: "agent",
          head: t("settings.agents.colAgent"),
          name: t("tasks.agent"),
          kind: "select",
          empty: t("skipper.noAgentOnShort"),
          options: (task) => {
            const list: [string, string][] = choices.map((x) => [x.id, x.custom ? t("skipper.custom", { id: x.id }) : x.id]);
            // A row keeps an agent that is off or gone, so the picker shows the truth.
            if (!choices.some((x) => x.id === task.agent)) list.unshift([task.agent, t(byId(task.agent) ? "tasks.agentOff" : "tasks.agentGone", { id: task.agent })]);
            return list;
          },
          get: (task) => task.agent,
        },
        {
          key: "model",
          head: t("tasks.colModel"),
          name: t("tasks.model"),
          kind: "text",
          mono: true,
          placeholder: () => t("skipper.agentDefault"),
          get: (task) => task.model ?? "",
          // A custom agent sets its model itself. A gone agent has none.
          note: (task) => {
            const x = byId(task.agent);
            return modelFlag(x) ? null : x ? t("tasks.setByAgent") : "";
          },
          check: modelError,
        },
        {
          key: "instructions",
          head: t("tasks.colInstructions"),
          name: t("tasks.instructions"),
          kind: "lead",
          draw: (task) => {
            const none = task.instructions == null;
            const state = h("span", "c-state" + (none ? " none" : ""));
            state.append(h("span", "sq"), none ? t("tasks.none") : fileSize(task.instructions!));
            return state;
          },
        },
      ],
      status: (task) => smallButton(t("skipper.edit"), () => a.types.edit(task), { label: t("tasks.editInstructions", { type: task.type }) }),
      edit: (task, key, v) => {
        if (key === "type") return a.saveTask({ key: "rename_task", value: { from: task.type, to: v } });
        // A new agent with no model flag drops the model: skiff-core saves none.
        return a.saveTask({ key: "task", value: row(task, key === "agent" ? { agent: v } : { model: v || null }) });
      },
      remove: (task) => a.saveTask({ key: "remove_task", value: task.type }),
    },
    a.tasksTable,
    a.render,
  );
  // One 40px error row under the row it is about.
  for (const [type, why] of a.types.errors) {
    table.querySelector<HTMLElement>(`.set-row[data-id="${CSS.escape(type)}"]`)?.after(errorLine(why));
  }
  return table;
}

/**
 * Settings › Disk use › Folder: the path, Show in file manager and Change…. A
 * 40px error row under it says why only when the folder is not usable; the
 * counts and safety checks show in the Change… preview. The path is a read-only
 * text field, so it selects and copies as any field does, with the keys and the
 * field's own right-click menu. Only Change… sets the folder, after its preview.
 */
export function folderRows(set: SkiffAgentSettings, change: () => void): HTMLElement[] {
  const row = h("div", "set-row set-open set-agent set-folder on");
  const path = h("input", "ctl-field mono set-path");
  path.type = "text";
  path.readOnly = true;
  path.spellcheck = false;
  path.value = set.configured;
  path.title = set.configured;
  path.setAttribute("aria-label", t("skipper.folderAria"));
  const acts = h("span", "c-acts");
  // A remote Host's folder is on that Host: no file manager here opens it.
  if (set.folder_status === "ready" && !isRemote()) acts.append(smallButton(t("menu.showInFileManager"), () => reveal(set.configured)));
  acts.append(smallButton(t("skipper.change"), change));
  row.append(h("span", "c-name", t("skipper.folder")), fieldCell(path), acts);
  const why =
    set.folder_status === "other"
      ? t("skipper.folderOther")
      : set.folder_status === "refused" || set.folder_status === "missing"
        ? (set.folder_error ?? t("skipper.refused"))
        : null;
  if (!why) return [row];
  const meta = errorLine(why);
  meta.id = "set-folder-why";
  path.setAttribute("aria-invalid", "true");
  path.setAttribute("aria-describedby", meta.id);
  return [row, meta];
}

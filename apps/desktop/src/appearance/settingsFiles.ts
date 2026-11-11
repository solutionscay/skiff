/**
 * Settings › Skippers › Instructions and Settings › Task types › Edit: read
 * each file's status, open it in the editor, reset it. settings.ts owns the
 * state and passes every Host call and window action in as `env`.
 */
import { t } from "../i18n";
import { confirmAction } from "../ui/confirm";
import { basename } from "../workspace/paths";
import { fileSize } from "./taskTypes";

/** An instruction file row. The types come from settings.ts, which imports the Host side. */
export interface FileRow {
  file: string;
  scope: { kind: "skipper" } | { kind: "fleet" | "project" | "type"; name: string };
  modified: number | null;
  size: number | null;
}

/** A task type row. */
export interface TypeRow {
  type: string;
  file: string;
}

/** The state settings.ts reads to draw the pages. This module reads and writes it in place. */
export interface FilesState<F extends FileRow = FileRow> {
  files: F[];
  fileErrors: Map<string, string>;
  /** Task types › Edit faults, by type. */
  typeErrors: Map<string, string>;
}

type TabName = "skippers" | "task-types";

export interface FilesEnv<A> {
  /** A command on the app side. */
  ask<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  /** Shows a file in the peek over Settings; `done` runs when the peek ends. */
  peek(script: string, cwd: string, file: string, done: () => unknown): void;
  hostName(): string;
  /** True when Settings is open on this tab. */
  showing(tab: TabName): boolean;
  render(): void;
  setError(message: string): void;
  setAgentSet(a: A): void;
}

type Opened = { kind: "run"; script: string; cwd: string } | { kind: "default" } | { kind: "no_app"; message: string };

export function createSettingsFiles<F extends FileRow, A, T extends TypeRow>(env: FilesEnv<A>, st: FilesState<F>) {
  /** Settings › Skippers › Instructions: each file's status, read again on each show. */
  async function loadFiles() {
    try {
      st.files = await env.ask<F[]>("skipper_files");
      // A save of a remote Host's copy that did not go back: one line under its row.
      const host = env.hostName();
      for (const f of await env.ask<{ file: string; conflict: boolean; message: string }[]>("skipper_file_errors")) {
        st.fileErrors.set(f.file, f.conflict ? t("settings.remote.fileConflict", { host }) : f.message);
      }
    } catch (e) {
      st.files = [];
      env.setError(String(e));
    }
    if (env.showing("skippers")) env.render();
  }

  /**
   * Edit: the file opens as Open with › Text files says, written from the
   * template first when there is none. A terminal tool runs in the peek over
   * Settings, and the status reads again when it ends.
   */
  async function editFile(f: F) {
    st.fileErrors.delete(f.file);
    let o: Opened;
    try {
      o = await env.ask<Opened>("edit_skipper_file", { scope: f.scope });
    } catch (e) {
      st.fileErrors.set(f.file, String(e));
      return void loadFiles();
    }
    if (o.kind === "run") return env.peek(o.script, o.cwd, `${o.cwd}/${basename(f.file)}`, loadFiles);
    if (o.kind === "no_app") st.fileErrors.set(f.file, t("settings.noApp", { file: f.file }));
    void loadFiles();
  }

  /** Task types: each row's file size, read again on each show. */
  async function loadTypes() {
    const next = await env.ask<A>("get_skiff_agent").catch(() => null);
    if (next) env.setAgentSet(next);
    if (env.showing("task-types")) env.render();
  }

  /**
   * Task types › Edit: the file opens as Open with › Text files says, with
   * its `# <type>` line written first when there is none. A terminal tool
   * runs in the peek over Settings, and the size reads again when it ends.
   */
  async function editTypeFile(task: T) {
    st.typeErrors.delete(task.type);
    let o: Opened;
    try {
      o = await env.ask<Opened>("edit_task_type_file", { name: task.type });
    } catch (e) {
      st.typeErrors.set(task.type, String(e));
      return void loadTypes();
    }
    if (o.kind === "run") return env.peek(o.script, o.cwd, task.file, loadTypes);
    if (o.kind === "no_app") st.typeErrors.set(task.type, t("settings.noApp", { file: `${task.type}.md` }));
    void loadTypes();
  }

  /** Reset…: asks, with the file's date and size, then writes the template again. */
  async function resetFile(f: F) {
    const base = basename(f.file);
    const date = f.modified ? new Date(f.modified).toISOString().slice(0, 10) : "";
    const who = f.scope.kind === "skipper" ? t("settings.reset.everySkipper") : t("settings.reset.oneSkipper", { name: f.scope.name });
    const ok = await confirmAction({
      title: t("settings.reset.title", { file: f.file }),
      body: t("settings.reset.body", { date, size: fileSize(f.size ?? 0), base, who }),
      action: t("settings.reset.action"),
    });
    if (!ok) return;
    st.fileErrors.delete(f.file);
    try {
      await env.ask("reset_skipper_file", { scope: f.scope });
    } catch (e) {
      st.fileErrors.set(f.file, String(e));
    }
    void loadFiles();
  }

  return { loadFiles, editFile, loadTypes, editTypeFile, resetFile };
}

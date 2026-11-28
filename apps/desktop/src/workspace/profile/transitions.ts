/** Opening, creating, renaming and closing workspaces, the save after each change, and changes made on disk. */
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { render } from "../../app/renderRequest";
import { S } from "../../app/state";
import { showError } from "../../ui/alerts";
import { treeSections, useTreeView } from "../tree";
import { selectProject } from "../view";
import { t } from "../../i18n";
import { dirname } from "../paths";
import { EXT, nameOf, trimPath, type Profile, type createWorkspaceState } from "./state";
import { fileName, forget, forgetLast, moved, ownDir, read, recents, remember, rename, revision, write as writeFile, type Loaded } from "./storage";

type State = ReturnType<typeof createWorkspaceState>;

/** Why a name cannot be used: empty, or another workspace has it. */
export type NameFault = "empty" | "taken";

/** A name as a file name: no folder separators, no leading dot. */
const safeName = (name: string) => name.trim().replace(/[/\\]/g, "-").replace(/^\.+/, "");

export function createTransitions(state: State, hooks: { announce: (text: string) => void; home: () => HTMLElement | null }) {
  const { announce, home } = hooks;
  const clone = (p: Profile): Profile => JSON.parse(JSON.stringify(p));

  /** Makes the read file the open workspace and shows it. `keep`: the app start, where the restored place stays when it shows. */
  function commit(l: Loaded | null, focus = true, keep = false) {
    state.doc = l ? { path: l.path, profile: l.profile, revision: l.revision, written: JSON.stringify(l.profile), missing: false, failed: null } : null;
    state.autoOpened.clear();
    state.allWork = false;
    useTreeView(state.doc ? state.treeView : null);
    if (l) remember(l.path);
    showView(focus, keep);
  }

  /** After a switch: the saved pick if it shows, else the first project that shows, else none. The keys go to the sidebar's current row. `keep`: the project on screen stays when it shows. */
  function showView(focus = true, keep = false) {
    const shown = treeSections().flatMap((s) => s.projects);
    const here = keep && !!S.selectedProject && shown.some((p) => p.name === S.selectedProject);
    const want = here ? undefined : state.doc?.profile.selected ? shown.find((p) => trimPath(p.path) === trimPath(state.doc!.profile.selected!)) : undefined;
    const target = want ?? (S.selectedProject && shown.some((p) => p.name === S.selectedProject) ? undefined : shown[0]);
    state.quietly(() => {
      // A restored workspace can omit the pane just opened from another Host.
      if (target && !(keep && S.focused)) selectProject(target.name);
      render();
    });
    announce(state.doc ? t("workspace.openedSay", { name: state.workspaceName() }) : t("workspace.allProjects"));
    if (focus) requestAnimationFrame(() => home()?.focus());
  }

  // ── Save ───────────────────────────────────────────────────────────────────

  let writing = false;
  let again = false;

  /**
   * Writes the open workspace when it differs from the file. One write runs
   * at a time; a change during it writes again after. A file another window
   * wrote is replaced: the last write wins. A failure keeps the change in
   * memory, says so once, and the next change or disk check tries again.
   */
  async function save() {
    const d = state.doc;
    if (!d || d.missing) return;
    if (writing) {
      again = true;
      return;
    }
    const snap = clone(d.profile);
    const json = JSON.stringify(snap);
    if (json === d.written && !d.failed) return;
    writing = true;
    try {
      let rev: string;
      try {
        rev = await writeFile(d.path, snap, d.revision);
      } catch (e) {
        if (!String(e).startsWith("conflict:")) throw e;
        const now = await revision(d.path);
        if (now === null) {
          d.missing = true;
          return;
        }
        rev = await writeFile(d.path, snap, now);
      }
      d.revision = rev;
      d.written = json;
      d.failed = null;
    } catch (e) {
      const error = String(e).replace(/^conflict: /, "");
      if (!d.failed) {
        showError(t("workspace.cannotSave", { name: nameOf(d.path), error }));
        announce(t("workspace.notSavedSay", { error }));
      }
      d.failed = error;
    } finally {
      writing = false;
      render();
      if (again) {
        again = false;
        void save();
      }
    }
  }

  /** The file was gone: write the workspace to it again. */
  function saveAgain() {
    const d = state.doc;
    if (!d) return;
    d.missing = false;
    d.revision = null;
    d.written = "";
    void save();
  }

  // ── Create, rename, open, close ────────────────────────────────────────────

  /** A new workspace in Skiff's own folder, opened. A name another workspace there has is refused. */
  async function create(name: string, profile: Profile): Promise<NameFault | null> {
    const safe = safeName(name);
    if (!safe) return "empty";
    let path: string;
    let rev: string;
    try {
      path = `${await ownDir()}/${safe}.${EXT}`;
      rev = await writeFile(path, profile, null);
    } catch (e) {
      if (String(e).startsWith("conflict:")) return "taken";
      showError(e);
      return null;
    }
    commit({ path, profile, revision: rev });
    return null;
  }

  /** Renames the open workspace's file in its folder. */
  async function renameTo(name: string): Promise<NameFault | null> {
    const d = state.doc;
    if (!d) return null;
    const safe = safeName(name);
    if (!safe) return "empty";
    const to = `${dirname(d.path)}/${safe}.${EXT}`;
    if (to === d.path) return null;
    try {
      await rename(d.path, to);
    } catch (e) {
      if (String(e).endsWith(" exists")) return "taken";
      showError(e);
      return null;
    }
    moved(d.path, to);
    d.path = to;
    remember(to);
    // The file keeps a copy of its name for older Skiff builds: write it again under the new one.
    d.written = "";
    void save();
    return null;
  }

  /** Reads `path` and opens it. A bad file leaves the open workspace as it was. */
  async function openNow(path: string): Promise<boolean> {
    let l: Loaded;
    try {
      l = await read(path);
    } catch (e) {
      const known = recents().includes(path);
      showError(t("workspace.cannotOpen", { error: String(e) }), known ? { label: t("workspace.removeRecent"), run: async () => forget(path, render) } : undefined);
      return false;
    }
    commit(l);
    return true;
  }

  let picking = false;

  async function openWorkspace(path?: string) {
    if (picking) return;
    let target: string | string[] | null = path ?? null;
    if (!target) {
      picking = true;
      target = await openDialog({ title: t("workspace.openTitle"), filters: [{ name: t("workspace.fileType"), extensions: [EXT] }], multiple: false, directory: false })
        .catch((e) => {
          showError(e);
          return null;
        })
        .finally(() => (picking = false));
    }
    if (typeof target !== "string") return;
    await openNow(target);
  }

  /** Show all projects: no workspace open. */
  function closeWorkspace() {
    if (!state.doc) return;
    commit(null);
    forgetLast();
  }

  // ── Changes on disk ────────────────────────────────────────────────────────

  /** Reads the file again when something else changed it. Gone or unreadable: the mark says so, and changes wait. */
  async function checkDisk() {
    const d = state.doc;
    if (!d || writing) return;
    let now: string | null;
    try {
      now = await revision(d.path);
    } catch {
      now = null;
    }
    if (state.doc !== d || writing) return;
    if (now === null) {
      if (d.missing) return;
      d.missing = true;
      announce(t("workspace.unavailableSay", { file: fileName(d.path) }));
      render();
      return;
    }
    if (now !== d.revision || d.missing) {
      let l: Loaded;
      try {
        l = await read(d.path);
      } catch {
        return;
      }
      if (state.doc !== d || writing) return;
      d.profile = l.profile;
      d.revision = l.revision;
      d.written = JSON.stringify(l.profile);
      d.missing = false;
      d.failed = null;
      announce(t("workspace.reloadedSay", { name: nameOf(d.path) }));
      showView(false, true);
      return;
    }
    if (d.failed) void save();
  }

  return { commit, showView, save, saveAgain, create, renameTo, openWorkspace, closeWorkspace, checkDisk };
}

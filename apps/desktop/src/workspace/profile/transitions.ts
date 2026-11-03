/** File transitions and their leave guard. */
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { render } from "../../app/renderRequest";
import { S } from "../../app/state";
import { showError } from "../../ui/alerts";
import { treeSections, useTreeView } from "../tree";
import { selectProject } from "../view";
import { t } from "../../i18n";
import { canon, emptyProfile, trimPath, type Doc, type Profile, type createWorkspaceState } from "./state";
import { fileName, forget, forgetLast, read, recents, remember, revision, write as writeFile, type Loaded } from "./storage";
import { choose } from "./dialogs";

type State = ReturnType<typeof createWorkspaceState>;
export function createTransitions(state: State, hooks: { announce: (text: string) => void; changed: () => void; home: () => HTMLElement | null; editWorkspace: () => void }) {
  const { announce, changed, home, editWorkspace } = hooks;
  const workspaceEdited = state.workspaceEdited;
  const workspaceName = state.workspaceName;
  const EXT = "skiffw";
  const clone = (p: Profile): Profile => JSON.parse(JSON.stringify(p));
  /** Makes the read file the document, Clean, and shows it. */
  function commit(l: Loaded | null, focus = true) {
    state.doc = l
      ? { path: l.path, profile: l.profile, base: canon(l.profile), revision: l.revision, disk: "ok", kept: false, answered: false }
      : null;
    state.autoOpened.clear();
    state.allWork = false;
    useTreeView(state.doc ? state.treeView : null);
    if (l) remember(l.path);
    state.wasEdited = workspaceEdited();
    showView(focus);
  }

  /** A new Untitled workspace: empty, Clean, no file. */
  function commitUntitled() {
    state.doc = { path: null, profile: emptyProfile(), base: canon(emptyProfile()), revision: null, disk: "ok", kept: false, answered: false };
    state.autoOpened.clear();
    state.allWork = false;
    useTreeView(state.treeView);
    forgetLast();
    state.wasEdited = false;
    showView();
  }

  /** After a switch: the saved pick if it shows, else the first project that shows, else none. The keys go to the sidebar's current row. */
  function showView(focus = true) {
    const shown = treeSections().flatMap((s) => s.projects);
    const want = state.doc?.profile.selected ? shown.find((p) => trimPath(p.path) === trimPath(state.doc!.profile.selected!)) : undefined;
    const target = want ?? (S.selectedProject && shown.some((p) => p.name === S.selectedProject) ? undefined : shown[0]);
    state.quietly(() => {
      if (target) selectProject(target.name);
      render();
    });
    announce(state.doc ? t("workspace.openedSay", { name: workspaceName() }) : t("workspace.allProjects"));
    if (focus) requestAnimationFrame(() => home()?.focus());
  }

  /** Writes the document to `path`. `expect`: the revision there now, null for no file. */
  async function write(path: string, expect: string | null): Promise<boolean> {
    if (!state.doc) return false;
    const d = state.doc;
    const snap = clone(d.profile);
    state.saving = true;
    render();
    announce(t("workspace.savingSay"));
    try {
      const revision = await writeFile(path, snap, expect);
      d.path = path;
      d.revision = revision;
      d.base = canon(snap);
      d.disk = "ok";
      d.kept = false;
      d.answered = false;
      remember(path);
      announce(t("workspace.savedSay", { file: fileName(path) }));
      return true;
    } catch (e) {
      const text = String(e);
      if (text.startsWith("conflict:") && path === d.path) d.disk = "changed";
      announce(t("workspace.notSavedSay", { error: text }));
      showError(t("workspace.notSaved", { error: text.replace(/^conflict: /, "") }));
      return false;
    } finally {
      state.saving = false;
      changed();
    }
  }

  /** Save: to the file, else Save As. A file that changed on disk or is gone goes to Save As too. */
  async function saveNow(): Promise<boolean> {
    if (!state.doc) return false;
    if (!state.doc.path || state.doc.disk !== "ok") return saveAsNow();
    const path = state.doc.path;
    const ok = await write(path, state.doc.revision);
    // The file changed under the save: keep it, and offer another file.
    if (!ok && state.doc.disk !== "ok") return saveAsNow();
    return ok;
  }

  async function saveAsNow(): Promise<boolean> {
    if (!state.doc) return false;
    const name = workspaceName().replace(/[/\\]/g, "-");
    const picked = await saveDialog({
      title: t("workspace.saveAsTitle"),
      defaultPath: state.doc.path ?? `${name}.${EXT}`,
      filters: [{ name: t("workspace.fileType"), extensions: [EXT] }],
    }).catch((e) => {
      showError(e);
      return null;
    });
    if (!picked) return false;
    const path = picked.endsWith(`.${EXT}`) ? picked : `${picked}.${EXT}`;
    if (path === state.doc.path) {
      if (state.doc.disk !== "ok") {
        showError(t("workspace.changedOrGone", { file: fileName(path) }));
        return false;
      }
      return write(path, state.doc.revision);
    }
    let expect: string | null;
    try {
      expect = await revision(path);
    } catch (e) {
      showError(e);
      return false;
    }
    // The file dialog asked about `picked`. With the extension added, the file it replaces was never named.
    if (expect !== null && path !== picked) {
      const ok = await choose({
        title: t("workspace.replaceTitle"),
        body: t("workspace.replaceBody", { path }),
        choices: [{ id: "replace", label: t("workspace.replace"), tone: "danger" }],
      });
      if (ok !== "replace") return false;
    }
    return write(path, expect);
  }

  // ── The guard ──────────────────────────────────────────────────────────────

  /** Runs `f` alone: a request while another runs is dropped. */
  async function exclusive(f: () => Promise<boolean>): Promise<boolean> {
    if (state.busy) return false;
    state.busy = true;
    try {
      return await f();
    } finally {
      state.busy = false;
    }
  }

  /**
   * Leaves the document: asks first when it is Edited. Quit and the restart
   * after an update use it too. `go` does the transition and reports success.
   * Save runs first and must succeed; Cancel, a closed dialog or a failed
   * write stops here and the keys go back.
   */
  function leave(what: string, go: () => Promise<boolean>): Promise<boolean> {
    const back = document.activeElement as HTMLElement | null;
    const restore = () => requestAnimationFrame(() => (back?.isConnected ? back : home())?.focus());
    return exclusive(async () => {
      if (workspaceEdited()) {
        const where = state.doc!.path ?? t("workspace.notSavedYet");
        const c = await choose({
          title: t("workspace.saveTitle", { name: workspaceName() }),
          body: t("workspace.saveBody", { where, what }),
          choices: [
            { id: "save", label: t("workspace.save"), tone: "go" },
            { id: "discard", label: t("workspace.dontSave"), tone: "plain" },
          ],
        });
        if (c === "cancel") return (restore(), false);
        if (c === "save" && !(await saveNow())) return (restore(), false);
      }
      const ok = await go();
      if (!ok) restore();
      return ok;
    });
  }

  /** Reads `path` and opens it. A bad file leaves the document as it was. */
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

  function newWorkspace() {
    void leave(t("workspace.leaveNew"), async () => {
      commitUntitled();
      return true;
    }).then((ok) => ok && editWorkspace());
  }

  async function openWorkspace(path?: string) {
    if (state.busy || state.picking) return;
    let target: string | string[] | null = path ?? null;
    if (!target) {
      state.picking = true;
      target = await openDialog({ title: t("workspace.openTitle"), filters: [{ name: t("workspace.fileType"), extensions: [EXT] }], multiple: false, directory: false })
        .catch((e) => {
          showError(e);
          return null;
        })
        .finally(() => (state.picking = false));
    }
    if (typeof target !== "string") return;
    await leave(t("workspace.leaveOpen", { file: fileName(target) }), () => openNow(target));
  }

  /** Close workspace: back to All projects. */
  function closeWorkspace() {
    if (!state.doc) return;
    void leave(t("workspace.leaveClose"), async () => {
      commit(null);
      forgetLast();
      return true;
    });
  }

  function saveWorkspace() {
    void exclusive(saveNow);
  }

  function saveWorkspaceAs() {
    void exclusive(saveAsNow);
  }

  /** Quit and the window's close: the same guard. Sessions keep running in skiffd. */
  function quitApp() {
    void leave(t("workspace.leaveQuit"), async () => {
      try {
        await invoke("quit_app");
        return true;
      } catch (e) {
        showError(e);
        return false;
      }
    });
  }

  // ── Outside changes ────────────────────────────────────────────────────────

  async function checkDisk() {
    if (!state.doc?.path || state.saving || state.busy) return;
    const d = state.doc;
    let now: Doc["disk"];
    try {
      const rev = await revision(d.path!);
      now = rev === null ? "missing" : rev === d.revision ? "ok" : "changed";
    } catch {
      now = "missing";
    }
    if (state.doc !== d || state.saving || now === d.disk) return;
    d.disk = now;
    d.answered = false;
    if (now === "changed") announce(t("workspace.changedSay", { file: fileName(d.path!) }));
    else if (now === "missing") announce(t("workspace.unavailableSay", { file: fileName(d.path!) }));
    changed();
  }

  /** Reload from disk: read and check first, then ask before edits go. */
  async function reload() {
    await exclusive(async () => {
      if (!state.doc?.path) return false;
      let l: Loaded;
      try {
        l = await read(state.doc.path);
      } catch (e) {
        showError(t("workspace.cannotReload", { error: String(e) }));
        return false;
      }
      if (workspaceEdited()) {
        const c = await choose({
          title: t("workspace.reloadTitle", { name: workspaceName() }),
          body: t("workspace.reloadBody", { path: state.doc.path }),
          choices: [{ id: "reload", label: t("workspace.reload"), tone: "danger" }],
        });
        if (c !== "reload") return false;
      }
      commit(l);
      return true;
    });
  }

  /** Keep current: the view stays. While the disk differs, it counts as an edit. */
  function keepCurrent() {
    if (!state.doc) return;
    state.doc.kept = true;
    state.doc.answered = true;
    changed();
    requestAnimationFrame(() => home()?.focus());
  }

  // ── Edit dialog ────────────────────────────────────────────────────────────


    return { commit, showView, leave, openWorkspace, newWorkspace, closeWorkspace, saveWorkspace, saveWorkspaceAs, quitApp, checkDisk, reload, keepCurrent };
}

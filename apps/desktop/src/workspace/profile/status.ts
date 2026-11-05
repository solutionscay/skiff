/** Workspace status, menus and startup. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { render } from "../../app/renderRequest";
import { showError } from "../../ui/alerts";
import { ctxMenu } from "../../ui/contextMenu";
import { h, setHidden } from "../../ui/dom";
import type { MenuEntry } from "../../ui/menu";
import { t, tn, upper } from "../../i18n";
import type { createWorkspaceState } from "./state";
import { fileName, forget, forgetLast, last as lastPath, read, recents } from "./storage";
import type { createTransitions } from "./transitions";

export function createStatus(state: ReturnType<typeof createWorkspaceState>, transitions: ReturnType<typeof createTransitions>, hooks: { home: () => HTMLElement | null; editWorkspace: () => void }) {
  const { home, editWorkspace } = hooks;
  const { commit, checkDisk, reload, keepCurrent, quitApp, newWorkspace, openWorkspace, saveWorkspace, saveWorkspaceAs, closeWorkspace } = transitions;
  const { workspaceName, workspaceEdited, setLedgerAllWork } = state;
  const recentEntries = (): MenuEntry[] =>
    recents().map((path) => ({
      label: path === state.doc?.path ? t("workspace.recentOpen", { file: fileName(path) }) : fileName(path),
      run: () => void openWorkspace(path),
    }));

  /** File › Open recent workspace…: the recent files, under the Projects head as Open closed project does. */
  function recentWorkspacesMenu() {
    const r = document.querySelector<HTMLElement>("#sidebar-scroll .projects-head")?.getBoundingClientRect();
    const entries = recentEntries();
    if (!entries.length) entries.push({ label: t("workspace.noRecent"), disabled: true });
    ctxMenu.open(r ? r.left + 24 : 80, r ? r.bottom : 80, t("workspace.recentTitle"), entries);
  }

  /** What the open workspace needs a look for, for the File menu: a disk change, and missing references. */
  function workspaceNotice(): { disk: "changed" | "missing" | null; missing: number } {
    return { disk: state.doc && state.doc.disk !== "ok" && !state.doc.answered ? state.doc.disk : null, missing: state.missing().length };
  }

  const reloadWorkspace = () => void reload();
  const keepWorkspace = () => keepCurrent();

  /** The status bar item's menu: the same workspace commands as the File menu. */
  function workspaceMenu(at: HTMLElement) {
    const r = at.getBoundingClientRect();
    // The keys come back to the item: on Esc, and before a pick runs, so its dialog returns them here.
    const back = () => (at.isConnected && !at.hidden ? at : home())?.focus();
    const wrap = (es: MenuEntry[]): MenuEntry[] =>
      es.map((e) => ("head" in e || "sep" in e ? e : { ...e, run: e.run && (() => (back(), e.run!())), sub: e.sub && wrap(e.sub) }));
    ctxMenu.open(r.left, r.top, state.doc?.path ?? workspaceName(), wrap(menuEntries()), () => requestAnimationFrame(back));
  }

  function menuEntries(): MenuEntry[] {
    const on = !!state.doc;
    const recent = recentEntries();
    const n = workspaceNotice();
    const notice: MenuEntry[] = [];
    if (n.disk) {
      notice.push({ head: upper(t(n.disk === "changed" ? "workspace.changedOnDisk" : "workspace.fileMissing")) });
      if (n.disk === "changed") notice.push({ label: t("workspace.reloadFromDisk"), run: reloadWorkspace });
      notice.push({ label: t("workspace.keepCurrent"), run: keepWorkspace });
    }
    if (n.missing) notice.push({ icon: "tools-pencil", label: tn("workspace.repair", n.missing), run: editWorkspace });
    if (notice.length) notice.push({ head: upper(t("workspace.head")) });
    return [
      ...notice,
      { icon: "indicators-plus", label: t("cmd.newWorkspace"), run: newWorkspace },
      { icon: "documents-folder-open", label: t("cmd.openWorkspace"), run: () => void openWorkspace() },
      { icon: "documents-folder-open", label: t("workspace.openRecent"), sub: recent.length ? recent : [{ label: t("workspace.noRecent"), disabled: true }], disabled: !recent.length },
      { icon: "tools-pencil", label: t("cmd.editWorkspace"), run: editWorkspace, disabled: !on },
      { label: t("workspace.save"), run: saveWorkspace, disabled: !on },
      { label: t("workspace.saveAs"), run: saveWorkspaceAs, disabled: !on },
      { icon: "code-ungroup", label: t("workspace.closeToAll"), run: closeWorkspace, disabled: !on },
      { label: state.allWork ? t("workspace.ledgerShowWorkspace") : t("workspace.ledgerShowAll"), run: () => setLedgerAllWork(!state.allWork), disabled: !on },
    ];
  }

  /** The one short mark beside the name: what needs a look first. */
  function markOf(lost: number): { text: string; cls: string; say: string } | null {
    if (state.saving) return { text: t("workspace.markSaving"), cls: "", say: t("workspace.saySaving") };
    if (state.doc && state.doc.disk !== "ok" && !state.doc.answered) {
      const changedOnDisk = state.doc.disk === "changed";
      return {
        text: t(changedOnDisk ? "workspace.changedOnDisk" : "workspace.fileMissing"),
        cls: "edited",
        say: t(changedOnDisk ? "workspace.sayChanged" : "workspace.sayFileMissing"),
      };
    }
    if (workspaceEdited()) return { text: t("workspace.markEdited"), cls: "edited", say: t("workspace.sayEdited") };
    if (lost) return { text: tn("workspace.markMissing", lost), cls: "", say: tn("workspace.sayMissing", lost) };
    return state.doc ? { text: "", cls: "", say: t("workspace.saySaved") } : null;
  }

  /**
   * The status bar's sign that a workspace is open: its name and one mark.
   * Hidden in All projects. A click opens the workspace menu. Runs on each render.
   */
  function renderWorkspaceStatus() {
    if (!document.getElementById("ws-live")) {
      const live = h("div", "visually-hidden");
      live.id = "ws-live";
      live.setAttribute("role", "status");
      live.setAttribute("aria-live", "polite");
      document.body.append(live);
    }
    const el = document.getElementById("ws-status");
    if (!el) return;
    setHidden(el, !state.doc);
    if (!state.doc) return;
    const lost = state.missing();
    const mark = markOf(lost.length);
    const name = workspaceName();
    const why = lost.map((m) => t(m.kind === "project" ? "workspace.missingProject" : "workspace.missingFleet", { ref: m.ref, why: m.why }));
    const sig = JSON.stringify([name, state.doc.path, mark, why]);
    if (el.dataset.sig === sig) return;
    el.dataset.sig = sig;
    el.replaceChildren(h("span", "ws-status-name", name));
    if (mark?.text) el.append(h("span", `ws-status-mark ${mark.cls}`.trim(), mark.text));
    el.title = [state.doc.path ?? t("workspace.notSavedYet"), ...why].join("\n");
    el.setAttribute("aria-label", mark ? t("workspace.statusLabelSay", { name, state: mark.say }) : t("workspace.statusLabel", { name }));
    if (!el.dataset.wired) {
      el.dataset.wired = "1";
      el.addEventListener("click", () => workspaceMenu(el));
    }
  }

  // ── Start ──────────────────────────────────────────────────────────────────

  /** Register both exit routes before menus or documents become interactive. */
  async function startWorkspaceQuitGuard() {
    await listen("skiff:quit-requested", () => quitApp());
    await getCurrentWindow().onCloseRequested((e) => {
      e.preventDefault();
      quitApp();
    });
    await invoke("guard_quit");
  }

  /**
   * At start, once projects are in: the file the app was started with, else
   * the last saved workspace, read from disk as Clean. A file that cannot
   * open says why and leaves All projects; its recent entry stays.
   */
  async function startWorkspaces() {
    void listen<string>("skiff:open-workspace", (e) => {
      // Before the start is done, the start takes it from the pending list.
      if (!state.ready) return;
      void invoke("workspace_took", { path: e.payload }).catch(() => null);
      void openWorkspace(e.payload);
    });
    window.addEventListener("focus", () => void checkDisk());
    setInterval(() => void checkDisk(), 2000);

    const launch = await invoke<string | null>("workspace_launch_file").catch(() => null);
    let last: string | null = null;
    try {
      last = lastPath();
    } catch {
      /* none */
    }
    const path = launch ?? last;
    if (path) {
      try {
        commit(await read(path), false);
      } catch (e) {
        forgetLast();
        const known = recents().includes(path);
        showError(t("workspace.cannotOpenStart", { error: String(e) }), known ? { label: t("workspace.removeRecent"), run: async () => forget(path, render) } : undefined);
      }
    }
    state.ready = true;
    render();
    // A file the OS sent while the first one opened.
    const late = await invoke<string | null>("workspace_launch_file").catch(() => null);
    if (late) void openWorkspace(late);
  }

    return { recentWorkspacesMenu, workspaceNotice, reloadWorkspace, keepWorkspace, renderWorkspaceStatus, startWorkspaceQuitGuard, startWorkspaces, workspaceMenu };
}

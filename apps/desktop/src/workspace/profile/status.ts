/** The workspace list, the status bar switcher and the start. */
import { invoke } from "../../platform/invoke";
import { listen } from "@tauri-apps/api/event";
import { render } from "../../app/renderRequest";
import { shownHost } from "../../app/hosts";
import { showError } from "../../ui/alerts";
import { ctxMenu } from "../../ui/contextMenu";
import { h, setHidden } from "../../ui/dom";
import type { MenuEntry } from "../../ui/menu";
import { t, tn, upper } from "../../i18n";
import { nameOf, type createWorkspaceState } from "./state";
import { forget, forgetLast, last as lastPath, ownList, read, recents } from "./storage";
import type { createTransitions } from "./transitions";

export function createStatus(state: ReturnType<typeof createWorkspaceState>, transitions: ReturnType<typeof createTransitions>, hooks: {
  home: () => HTMLElement | null; newWorkspace: () => void; editWorkspace: () => void;
}) {
  const { home, newWorkspace, editWorkspace } = hooks;
  const { commit, checkDisk, openWorkspace, closeWorkspace, saveAgain } = transitions;

  /** The files in Skiff's own folder, read at start, on focus and after a change. */
  let own: string[] = [];
  async function refreshOwn() {
    // The own folder is this computer's: a remote Host's list is its recent files alone.
    const next = shownHost() ? [] : await ownList().catch(() => []);
    if (JSON.stringify(next) === JSON.stringify(own)) return;
    own = next;
    render();
  }

  /** Every workspace to switch to: Skiff's own folder and the recent files, by name. */
  function workspacePaths(): string[] {
    const open = state.doc?.path;
    const all = [...new Set([...own, ...recents(), ...(open ? [open] : [])])];
    return all.sort((a, b) => nameOf(a).localeCompare(nameOf(b), undefined, { sensitivity: "base" }));
  }

  /** File › Open recent workspace: each workspace, the open one marked. */
  function recentWorkspaceEntries(): MenuEntry[] {
    const open = state.doc?.path;
    const entries: MenuEntry[] = workspacePaths().map((path) => ({
      label: path === open ? t("workspace.recentOpen", { file: nameOf(path) }) : nameOf(path),
      run: () => void openWorkspace(path),
    }));
    return entries.length ? entries : [{ label: t("workspace.noRecent"), disabled: true }];
  }

  /** What the status mark and the menu say: the file is gone, the last write failed, references not found. */
  function workspaceNotice(): { missing: boolean; failed: string | null; notFound: number } {
    const d = state.doc;
    return { missing: !!d?.missing, failed: d?.failed ?? null, notFound: state.missing().length };
  }

  /** The status bar item's menu: switch workspace, or edit, make or open one. */
  function workspaceMenu(at: HTMLElement) {
    const r = at.getBoundingClientRect();
    // The keys come back to the item: on Esc, and before a pick runs, so its dialog returns them here.
    const back = () => (at.isConnected && !at.hidden ? at : home())?.focus();
    const wrap = (es: MenuEntry[]): MenuEntry[] => es.map((e) => ("head" in e || "sep" in e ? e : { ...e, run: e.run && (() => (back(), e.run!())) }));
    ctxMenu.open(r.left, r.top, t("workspace.headList"), wrap(menuEntries()), () => requestAnimationFrame(back));
  }

  function menuEntries(): MenuEntry[] {
    const open = state.doc?.path ?? null;
    const n = workspaceNotice();
    const out: MenuEntry[] = [];
    if (n.missing) {
      out.push({ head: upper(t("workspace.fileMissing")) });
      out.push({ label: t("workspace.saveAgain"), run: saveAgain });
      out.push({ sep: true });
    }
    out.push({ head: upper(t("workspace.headList")) });
    for (const path of workspacePaths()) {
      out.push({ label: nameOf(path), current: path === open, ...(path === open ? { icon: "indicators-check" } : {}), run: () => void (path !== open && openWorkspace(path)) });
    }
    out.push({ label: t("workspace.allProjects"), current: !open, ...(!open ? { icon: "indicators-check" } : {}), run: closeWorkspace });
    out.push({ sep: true });
    if (open) {
      const name = state.workspaceName();
      out.push({ icon: "tools-pencil", label: n.notFound ? tn("workspace.editNamedNotFound", n.notFound, { name }) : t("workspace.editNamed", { name }), run: editWorkspace });
    }
    out.push({ icon: "indicators-plus", label: t("cmd.newWorkspace"), run: newWorkspace });
    out.push({ icon: "documents-folder-open", label: t("cmd.openWorkspace"), run: () => void openWorkspace() });
    return out;
  }

  /** The one short mark beside the name: what needs a look first. */
  function markOf(): { text: string; cls: string; say: string } | null {
    const n = workspaceNotice();
    if (n.missing) return { text: t("workspace.fileMissing"), cls: "warn", say: t("workspace.sayFileMissing") };
    if (n.failed) return { text: t("workspace.markNotSaved"), cls: "warn", say: t("workspace.sayNotSaved") };
    if (n.notFound) return { text: tn("workspace.markNotFound", n.notFound), cls: "", say: tn("workspace.sayNotFound", n.notFound) };
    return null;
  }

  /**
   * The status bar's sign of the workspace: its name and one mark, or All
   * projects. A click opens the switcher. Runs on each render.
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
    setHidden(el, false);
    const d = state.doc;
    const mark = markOf();
    const name = state.workspaceName();
    const lost = state.missing().map((m) => t(m.kind === "project" ? "workspace.missingProject" : "workspace.missingFleet", { ref: m.ref, why: m.why }));
    const sig = JSON.stringify([name, d?.path, mark, lost, d?.failed]);
    if (el.dataset.sig === sig) return;
    el.dataset.sig = sig;
    el.replaceChildren(h("span", "ws-status-name", name));
    if (mark) el.append(h("span", `ws-status-mark ${mark.cls}`.trim(), mark.text));
    el.title = d ? [d.path, ...(d.failed ? [d.failed] : []), ...lost].join("\n") : name;
    el.setAttribute("aria-label", mark ? t("workspace.statusLabelSay", { name, state: mark.say }) : t("workspace.statusLabel", { name }));
    if (!el.dataset.wired) {
      el.dataset.wired = "1";
      el.addEventListener("click", () => workspaceMenu(el));
    }
  }

  // ── Start ──────────────────────────────────────────────────────────────────

  /**
   * At start, once projects are in: the file the app was started with, else
   * the last workspace. A file that cannot open says why and leaves All
   * projects; its recent entry stays.
   */
  async function startWorkspaces() {
    void listen<string>("skiff:open-workspace", (e) => {
      // Before the start is done, the start takes it from the pending list.
      if (!state.ready) return;
      void invoke("workspace_took", { path: e.payload }).catch(() => null);
      void openWorkspace(e.payload);
    });
    window.addEventListener("focus", () => {
      void checkDisk();
      void refreshOwn();
    });
    setInterval(() => void checkDisk(), 2000);
    void refreshOwn();

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
        commit(await read(path), false, true);
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

  return { recentWorkspaceEntries, workspaceNotice, renderWorkspaceStatus, startWorkspaces, workspaceMenu, refreshOwn };
}

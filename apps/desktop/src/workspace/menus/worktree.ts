import { ctxMenu } from "../../ui/contextMenu";
import { confirmAction, promptAction } from "../../ui/confirm";
import { h } from "../../ui/dom";
import { keyLabel } from "../../app/keys";
import { t } from "../../i18n";
import { type MenuEntry } from "../../ui/menu";
import { isRemote } from "../../app/hosts";
import { branchName } from "../model";
import type { HostBranches, Project, Worktree } from "../../platform/types";
import { openPath } from "../../platform/os";
import { loadProjects } from "../../app/daemon";
import { launchMenu } from "../../app/panels";
import { showError } from "../../ui/alerts";
import { hasChanges, reviewChanges, showsChanges } from "../changes";
import { render } from "../../app/render";
import { removeErrors, removing, S, selectedWorktree } from "../../app/state";
import { refocusTerminal } from "../view";
import { hostCan, hostInvoke } from "../../app/hostCaps";

/** Ask for a branch, then make a worktree for it. A name the list shows is an
 *  existing branch with no worktree: git checks it out. Any other name is a new
 *  branch from the From branch. Errors stay in the dialog. */
export async function newWorktree(p: Project) {
  S.justAdded = null;
  // A Host without `branch_list` still makes new branches from the folder's.
  const b = await hostInvoke<HostBranches>("worktrees", "list_branches", { project: p.name }).catch(() => null);
  const local = new Set(b?.branches.filter((x) => !x.remote).map((x) => x.name));
  // A remote's branch is offered by its own name, as git checks it out.
  const free = [
    ...new Set(
      (b?.branches ?? []).flatMap((x) => {
        if (!x.remote) return x.worktree ? [] : [x.name];
        const name = x.name.slice(x.name.indexOf("/") + 1);
        return local.has(name) ? [] : [name];
      }),
    ),
  ];
  const isFree = (v: string) => free.some((n) => n.toLowerCase() === v.toLowerCase());

  const from = h("label", "prompt-from");
  const base = h("select", "");
  base.setAttribute("aria-label", t("newWorktree.fromAria"));
  const head = b?.head ?? "";
  for (const name of [...(b && !b.head ? [""] : []), ...(b?.branches.map((x) => x.name) ?? [])]) {
    const o = h("option", "", name || "HEAD");
    o.value = name;
    base.append(o);
  }
  base.value = head;
  from.append(h("span", "", t("newWorktree.from")), base);
  from.hidden = !b;

  promptAction({
    title: t("newWorktree.title", { name: p.name }),
    body: free.length
      ? t("newWorktree.bodyFree")
      : t("newWorktree.body"),
    placeholder: t("newWorktree.placeholder"),
    ...(free.length ? { options: free, optionsHead: t("newWorktree.freeHead") } : {}),
    after: [from],
    onInput: (v) => (from.hidden = !b || isFree(v)),
    action: t("newWorktree.action"),
    submit: async (branch) => {
      const checkout = isFree(branch);
      const start = checkout || base.value === head ? null : base.value;
      const w = await hostInvoke<Worktree>("worktrees", "add_worktree", { project: p.name, branch, base: start, checkout });
      selectedWorktree.set(p.name, w.path);
      await loadProjects();
    },
    onClose: refocusTerminal,
  });
}

/** The + on a worktree's row in the sidebar, if it shows. */
export function plusOf(w: Worktree): HTMLElement | undefined {
  return [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-plus")].find((b) => b.dataset.wt === w.path);
}

/** A right-click on a worktree's branch row. */
export function worktreeMenu(p: Project, w: Worktree, x: number, y: number) {
  const plus = plusOf(w);
  // A remote Host's path: start a session there or copy it, never open it on this Mac.
  const local = !isRemote();
  const off = !!p.error || !hostCan("session_lifecycle");
  const entries: (MenuEntry | false)[] = [
    { icon: "indicators-plus", label: t("cmd.newSession"), hint: keyLabel("new-session"), disabled: off, run: () => launchMenu.open(plus ?? { x, y }, p, w) },
    // A new group stands on its own, beside New session: its layouts.
    { icon: "code-group", label: t("select.newGroup"), disabled: off, sub: launchMenu.groups(p, w) },
    showsChanges(p) && hasChanges(w) && { icon: "code-git-branch", label: t("menu.reviewChanges"), run: () => reviewChanges(w) },
    local && { icon: "indicators-square-arrow-out-up-right", label: t("menu.showInFileManager"), run: () => void openPath(w.path).catch(showError) },
    { icon: "code-copy", label: t("menu.copyPath"), run: () => void navigator.clipboard.writeText(w.path).catch(showError) },
    // The Host removes it, and refuses while one of its sessions runs there.
    !w.is_main && hostCan("worktrees") && { icon: "tools-trash-2", label: t("menu.removeWorktree"), danger: true, disabled: removing.has(w.path), run: () => void removeWorktree(p, w) },
  ];
  ctxMenu.open(x, y, branchName(w), entries.filter((e): e is MenuEntry => !!e));
}

async function removeWorktree(p: Project, w: Worktree) {
  if (removing.has(w.path)) return;
  const ok = await confirmAction({
    title: t("removeWorktree.title", { name: branchName(w) }),
    body: t("removeWorktree.body", { path: w.path }),
    action: t("removeWorktree.action"),
  });
  if (!ok) return refocusTerminal();
  removing.add(w.path);
  removeErrors.delete(w.path);
  render();
  try {
    try {
      await hostInvoke("worktrees", "remove_worktree", { project: p.name, path: w.path, force: false });
    } catch (e) {
      // The Host refuses uncommitted changes. Ask again, and force only on a yes:
      // a new request the person chose, never a retry of the first.
      if (!String(e).includes("uncommitted changes")) throw e;
      const force = await confirmAction({
        title: t("dirtyWorktree.title", { name: branchName(w) }),
        body: t("dirtyWorktree.body"),
        action: t("dirtyWorktree.action"),
      });
      if (!force) return refocusTerminal();
      await hostInvoke("worktrees", "remove_worktree", { project: p.name, path: w.path, force: true });
    }
    if (selectedWorktree.get(p.name) === w.path) selectedWorktree.delete(p.name);
    await loadProjects();
  } catch (e) {
    removeErrors.set(w.path, String(e));
  } finally {
    removing.delete(w.path);
    render();
  }
}

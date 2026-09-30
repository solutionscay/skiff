/**
 * The Changes section under each worktree, and the +/- counts on its row.
 * It lists what git reports as changed against HEAD and hands files to
 * other apps. No diff view: reviewing a change is another app's job.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Worktree } from "./types";
import { button, chevron, h, icon, showError } from "./dom";
import type { MenuEntry } from "./menu";
import { ctxMenu } from "./menus";
import { branchName } from "./model";
import { showDiff } from "./peek";
import { render, scheduleRender } from "./render";

type Change = { path: string; status: "M" | "A" | "D" | "U"; added: number | null; removed: number | null };

/** The last read of each worktree. The sidebar renders from here, never waiting. */
const lists = new Map<string, Change[]>();
const loading = new Set<string>();
/** Worktrees whose Changes section is open. Closed by default. */
const shown = new Set<string>();
/** The row a single click picked. A double click opens the file. */
let picked: string | null = null;

function load(wt: string) {
  if (loading.has(wt)) return;
  loading.add(wt);
  invoke<Change[]>("git_changes", { path: wt })
    .then((list) => lists.set(wt, list))
    // Not a repository, or git is missing: the worktree shows no changes.
    .catch(() => lists.set(wt, []))
    .finally(() => {
      loading.delete(wt);
      scheduleRender();
    });
}

/** Reads every known worktree again. */
export function reloadChanges() {
  for (const wt of lists.keys()) load(wt);
}

function listFor(w: Worktree): Change[] | undefined {
  const list = lists.get(w.path);
  if (!list) load(w.path);
  return list;
}

/** `+12 −3` for the worktree row, or null when nothing changed. */
export function changeCounts(w: Worktree): HTMLElement | null {
  const list = listFor(w);
  if (!list?.length) return null;
  let add = 0;
  let del = 0;
  for (const c of list) {
    add += c.added ?? 0;
    del += c.removed ?? 0;
  }
  const el = h("span", "wt-diff");
  el.title = `${list.length} changed ${list.length === 1 ? "file" : "files"}`;
  el.append(h("span", "diff-add", `+${add}`), h("span", "diff-del", `−${del}`));
  return el;
}

/** Whether the worktree has changes, as last read. */
export const hasChanges = (w: Worktree) => !!lists.get(w.path)?.length;

const join = (wt: string, rel: string) => `${wt}/${rel}`;
const open = (path: string) => void invoke("open_file", { path }).catch(showError);
const reveal = (path: string) => void invoke("reveal_file", { path }).catch(showError);
const copy = (path: string) => void navigator.clipboard.writeText(path).catch(showError);

function toggle(wt: string) {
  if (shown.has(wt)) shown.delete(wt);
  else {
    shown.add(wt);
    load(wt);
  }
  render();
}

/** The section, or null when the worktree has no changes. */
export function changesBlock(w: Worktree): HTMLElement | null {
  const list = listFor(w);
  if (!list?.length) return null;
  const isOpen = shown.has(w.path);
  const block = h("div", "files-block changes-block");
  const head = button("wt-count files-head", "", () => toggle(w.path));
  head.dataset.key = `changes:${w.path}`;
  head.setAttribute("aria-expanded", String(isOpen));
  const tgl = h("span", "files-toggle");
  tgl.appendChild(chevron(isOpen));
  const glyph = icon('<path d="M12 3v12"></path><path d="M6 9h12"></path><path d="M6 21h12"></path>');
  glyph.classList.add("files-icon");
  head.append(tgl, glyph, h("span", "", "Changes"), h("span", "changes-n", String(list.length)));
  head.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    e.stopPropagation();
    ctxMenu.open(e.clientX, e.clientY, "Changes", [
      { icon: "code-git-branch", label: "Review all changes", run: () => showDiff(w.path, branchName(w)) },
      { icon: "schedule-refresh-cw", label: "Refresh", run: () => load(w.path) },
    ]);
  });
  block.appendChild(head);
  if (isOpen) {
    const rows = h("div", "files-rows");
    for (const c of list) rows.appendChild(row(w, c));
    block.appendChild(rows);
  }
  return block;
}

function row(w: Worktree, c: Change): HTMLElement {
  const wt = w.path;
  const path = join(wt, c.path);
  // The row whose diff shows is the picked row, however the diff was asked for.
  const show = () => {
    picked = path;
    render();
    showDiff(wt, branchName(w), c.path);
  };
  const gone = c.status === "D";
  const slash = c.path.lastIndexOf("/");
  const name = c.path.slice(slash + 1);
  const dir = slash < 0 ? "" : c.path.slice(0, slash);
  const r = h("button", "file-row change-row" + (gone ? " st-gone" : "") + (path === picked ? " picked" : ""));
  r.type = "button";
  r.dataset.key = `change:${path}`;
  r.title = `${c.path}${gone ? "\nDeleted" : ""}\nDouble-click to open the diff`;
  r.addEventListener("click", (m) => {
    picked = path;
    // detail counts clicks across the re-render the first click causes.
    if (m.detail >= 2) show();
    else render();
  });
  r.addEventListener("keydown", (k) => {
    if (k.key !== "Enter") return;
    k.preventDefault();
    k.stopPropagation();
    show();
  });
  const st = h("span", `change-st st-${c.status}`, c.status);
  const counts = h("span", "change-counts");
  if (c.added === null) counts.textContent = "bin";
  else {
    if (c.added) counts.appendChild(h("span", "diff-add", `+${c.added}`));
    if (c.removed) counts.appendChild(h("span", "diff-del", `−${c.removed}`));
  }
  r.append(st, h("span", "file-name", name), h("span", "change-dir", dir), counts);
  r.addEventListener("contextmenu", (m) => {
    m.preventDefault();
    m.stopPropagation();
    const items: MenuEntry[] = [
      { icon: "code-git-branch", label: "Show diff", hint: "Enter", run: show },
    ];
    if (!gone) items.push(
      { icon: "indicators-square-arrow-out-up-right", label: "Open file", run: () => open(path) },
      { icon: "documents-folder-open", label: "Show in file manager", run: () => reveal(path) },
    );
    items.push({ icon: "code-copy", label: "Copy path", run: () => copy(path) });
    ctxMenu.open(m.clientX, m.clientY, name, items);
  });
  return r;
}

/**
 * The Files section under each worktree, for projects with `files = true`.
 * It lists folders and opens files with their Open with command or default app. No edits, no
 * previews, no watching: a folder is read again when it opens, or on Refresh.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Project, Worktree } from "./types";
import { button, h } from "./dom";
import { chevron, icon } from "./icons";
import { showError } from "./alerts";
import { ctxMenu } from "./contextMenu";
import { render } from "./render";
import { storedSet } from "./stored";

type Entry = { name: string; dir: boolean };
type Listing = Entry[] | { error: string };

/** Projects that show the Files section, from projects.toml. */
const on = new Set<string>();
/** The last listing of each folder read. The sidebar renders from here, never waiting. */
const listings = new Map<string, Listing>();
/** Worktrees whose Files section is open. Closed by default. */
const shown = storedSet("skiff.filesOpen");
/** Open folders, by path. */
const expanded = storedSet("skiff.foldersOpen");
/** Folders being read. */
const reading = new Set<string>();

const join = (dir: string, name: string) => `${dir}/${name}`;

export const showsFiles = (p: Project) => on.has(p.name);

/** Reads which projects show files. Called with every project reload. */
export async function loadFilesSetting() {
  const names = await invoke<string[]>("files_projects").catch(() => []);
  on.clear();
  for (const n of names) on.add(n);
}

export async function setShowFiles(p: Project, show: boolean) {
  try {
    await invoke("set_project_files", { project: p.name, on: show });
    if (show) on.add(p.name);
    else on.delete(p.name);
    render();
  } catch (e) {
    showError(e);
  }
}

function load(dir: string) {
  if (reading.has(dir)) return;
  reading.add(dir);
  invoke<Entry[]>("list_dir", { path: dir })
    .then((entries) => listings.set(dir, entries))
    .catch((e) => listings.set(dir, { error: String(e) }))
    .finally(() => {
      reading.delete(dir);
      render();
    });
}

/** Reads again `root` and every open folder under it. */
function refresh(root: string) {
  load(root);
  for (const d of expanded) if (d.startsWith(root + "/")) load(d);
}

/** Opening reads again, so the tree is as fresh as the last click. */
function toggle(dir: string, set: Set<string>) {
  if (set.has(dir)) set.delete(dir);
  else {
    set.add(dir);
    refresh(dir);
  }
  render();
}

const open = (path: string) => void invoke("open_file", { path }).catch(showError);
const reveal = (path: string) => void invoke("reveal_file", { path }).catch(showError);
const copy = (path: string) => void navigator.clipboard.writeText(path).catch(showError);

export function filesBlock(w: Worktree): HTMLElement {
  const isOpen = shown.has(w.path);
  const block = h("div", "files-block");
  const head = button("wt-count files-head", "", () => toggle(w.path, shown));
  head.dataset.key = `files:${w.path}`;
  head.setAttribute("aria-expanded", String(isOpen));
  const tgl = h("span", "files-toggle");
  tgl.appendChild(chevron(isOpen));
  const glyph = icon('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"></path><path d="M14 3v5h5"></path>');
  glyph.classList.add("files-icon");
  head.append(tgl, glyph, h("span", "", "Files"));
  head.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    e.stopPropagation();
    ctxMenu.open(e.clientX, e.clientY, "Files", [
      { icon: "schedule-refresh-cw", label: "Refresh", run: () => refresh(w.path) },
      { icon: "indicators-square-arrow-out-up-right", label: "Show in file manager", run: () => open(w.path) },
    ]);
  });
  block.appendChild(head);
  if (isOpen) {
    const rows = h("div", "files-rows");
    folder(rows, w.path, 0);
    block.appendChild(rows);
  }
  return block;
}

function folder(into: HTMLElement, dir: string, depth: number) {
  const l = listings.get(dir);
  if (!l) {
    // Open from a past run: read it the first time it shows.
    load(dir);
    into.appendChild(note("…", depth));
  }
  else if (!Array.isArray(l)) into.appendChild(note(l.error, depth, true));
  else if (!l.length) into.appendChild(note("Empty", depth));
  else for (const e of l) {
    const path = join(dir, e.name);
    into.appendChild(row(e, path, depth));
    if (e.dir && expanded.has(path)) folder(into, path, depth + 1);
  }
}

function row(e: Entry, path: string, depth: number): HTMLElement {
  const cls = ["file-row"];
  if (e.dir) cls.push("dir");
  if (e.name.startsWith(".")) cls.push("hidden-file");
  const r = h("button", cls.join(" "));
  r.type = "button";
  r.dataset.key = `file:${path}`;
  r.style.setProperty("--depth", String(depth));
  r.title = e.dir ? path : `${path}\nDouble-click to open`;
  if (e.dir) r.setAttribute("aria-expanded", String(expanded.has(path)));
  r.addEventListener("click", (m) => {
    // detail counts clicks across the re-render the first click causes.
    if (e.dir) toggle(path, expanded);
    else if (m.detail >= 2) open(path);
  });
  r.addEventListener("keydown", (k) => {
    if (k.key !== "Enter" || e.dir) return;
    k.preventDefault();
    k.stopPropagation();
    open(path);
  });
  const lead = h("span", "file-lead");
  if (e.dir) lead.appendChild(chevron(expanded.has(path)));
  r.append(lead, h("span", "file-name", e.name));
  r.addEventListener("contextmenu", (m) => {
    m.preventDefault();
    m.stopPropagation();
    ctxMenu.open(m.clientX, m.clientY, e.name, e.dir
      ? [
          { icon: "schedule-refresh-cw", label: "Refresh", run: () => refresh(path) },
          { icon: "indicators-square-arrow-out-up-right", label: "Show in file manager", run: () => open(path) },
          { icon: "code-copy", label: "Copy path", run: () => copy(path) },
        ]
      : [
          { icon: "indicators-square-arrow-out-up-right", label: "Open", run: () => open(path) },
          { icon: "documents-folder-open", label: "Show in file manager", run: () => reveal(path) },
          { icon: "code-copy", label: "Copy path", run: () => copy(path) },
        ]);
  });
  return r;
}

function note(text: string, depth: number, error = false): HTMLElement {
  const n = h("div", "file-note" + (error ? " error" : ""), text);
  n.style.setProperty("--depth", String(depth));
  return n;
}

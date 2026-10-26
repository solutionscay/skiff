/**
 * Images: one library of this computer's image cache, newest first. Every
 * drop and Skiff Paste puts its image there. The pane header button shows
 * the newest one and opens the library. Each row opens the image, inserts
 * its path into the focused pane, pins it, copies its path or deletes it.
 * Drag a thumbnail onto any pane to insert the path there.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { hostName, isRemote } from "../app/hosts";
import { scheduleRender } from "../app/render";
import { S, sessions } from "../app/state";
import { accent, place } from "../app/stateQueries";
import { copyText } from "../platform/clipboard";
import { showError } from "../ui/alerts";
import { button, h } from "../ui/dom";
import { icon } from "../ui/icons";
import { menuOpened } from "../ui/menu";
import { refocusTerminal } from "../workspace/view";
import { endImageDrag, showImageDrag } from "./fileDrop";
import { focusPane } from "./paneActions";
import { type Typed, typeFiles, whoOf } from "./terminalClipboard";
import { panes } from "./terminalState";

/** One library image, as `images` answers. */
interface Image {
  path: string;
  file: string;
  /** The dropped file's own name. Null: a paste, or a file Skiff has no entry for. */
  name: string | null;
  session: string | null;
  title: string | null;
  project: string | null;
  /** Unix ms of the last drop, paste or Insert. */
  at: number;
  pinned: boolean;
}

let list: Image[] = [];

/** Thumbnails by path: a small data URL, or "" for a format the webview cannot draw. */
const thumbs = new Map<string, Promise<string>>();

const MIME: Record<string, string> = { svg: "image/svg+xml", png: "image/png", jpg: "image/jpeg", gif: "image/gif", webp: "image/webp", bmp: "image/bmp", tif: "image/tiff", heic: "image/heic", avif: "image/avif" };

/** Draws the image into a small canvas, so the page keeps no full-size copy. */
function thumb(path: string): Promise<string> {
  let t = thumbs.get(path);
  if (!t) {
    t = invoke<ArrayBuffer>("image_thumb", { path })
      .then((bytes) => new Promise<string>((done) => {
        const ext = path.split(".").pop()?.toLowerCase() ?? "";
        const url = URL.createObjectURL(new Blob([bytes], { type: MIME[ext] ?? "" }));
        const img = new Image();
        img.onload = () => {
          const c = document.createElement("canvas");
          const scale = Math.min(1, 160 / Math.max(img.naturalWidth, img.naturalHeight, 1));
          c.width = Math.max(1, Math.round(img.naturalWidth * scale));
          c.height = Math.max(1, Math.round(img.naturalHeight * scale));
          c.getContext("2d")?.drawImage(img, 0, 0, c.width, c.height);
          URL.revokeObjectURL(url);
          done(c.toDataURL());
        };
        // HEIC on Linux: an empty cell.
        img.onerror = () => {
          URL.revokeObjectURL(url);
          done("");
        };
        img.src = url;
      }))
      .catch(() => "");
    thumbs.set(path, t);
  }
  return t;
}

/** Fills `box` with the thumbnail when it is ready. */
function fillThumb(box: HTMLElement, path: string) {
  void thumb(path).then((url) => {
    if (!url || box.dataset.path !== path) return;
    const img = h("img");
    img.src = url;
    img.alt = "";
    img.draggable = false;
    box.replaceChildren(img);
  });
}

export const imageCount = () => list.length;
export const newestImage = (): string | null => list[0]?.path ?? null;

async function load() {
  try {
    list = await invoke<Image[]>("images");
  } catch (e) {
    list = [];
    console.warn("images:", e);
  }
  const live = new Set(list.map((i) => i.path));
  for (const p of thumbs.keys()) if (!live.has(p)) thumbs.delete(p);
  scheduleRender();
  if (!lib.hidden) {
    if (!list.length) closeImages();
    else draw();
  }
}

void load();
void listen("skiff:images-changed", () => void load());

// The library: one floating surface, under the button that opened it.
const lib = h("div");
lib.id = "images-lib";
lib.setAttribute("role", "menu");
lib.setAttribute("aria-label", "Images");
lib.tabIndex = -1;
lib.hidden = true;
const libCount = h("span", "lib-count");
const libHead = h("div", "lib-head");
libHead.append(h("b", "", "Images"), libCount);
const rows = h("div", "lib-list");
lib.append(libHead, rows);
document.body.appendChild(lib);

/** The lit row, by path, so a redraw keeps it. */
let lit: string | null = null;
/** The path whose time cell says "Copied". */
let copied: string | null = null;
let copiedTimer = 0;

/** Pinned first, then the rest; each newest first. */
const ordered = () => [...list.filter((i) => i.pinned), ...list.filter((i) => !i.pinned)];

const seen = new IntersectionObserver((es) => {
  for (const e of es) {
    if (!e.isIntersecting) continue;
    const box = e.target as HTMLElement;
    seen.unobserve(box);
    fillThumb(box, box.dataset.path!);
  }
}, { root: rows });

function when(at: number): string {
  const d = new Date(at);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

/** Line 2: the project letters in its color and the session title. Dim once that session ended. */
function whoLine(i: Image): HTMLElement {
  const s = i.session ? sessions.get(i.session) : undefined;
  const line = h("span", "lib-who" + (!s || s.state === "done" ? " ended" : ""));
  const p = i.project ? S.projects.find((x) => x.name === i.project) : undefined;
  if (p) {
    const mark = h("span", "lib-proj", p.short);
    mark.style.color = accent(p);
    line.append(mark);
  }
  line.append(i.title ?? "");
  return line;
}

function act(label: string, run: () => void, cls = ""): HTMLButtonElement {
  const b = button("lib-act" + cls, label, run);
  // The row's own click opens the image.
  b.addEventListener("click", (e) => e.stopPropagation());
  b.tabIndex = -1;
  return b;
}

/** The dropped file's name; "Pasted image" for a paste. An image from
 *  before the index has no entry: "PNG image", or "Image" with no extension,
 *  never its cache hash. */
function shownName(i: Image): string {
  if (i.name) return i.name;
  if (i.session) return "Pasted image";
  const ext = i.file.includes(".") ? i.file.split(".").pop()!.toUpperCase() : "";
  return ext ? `${ext} image` : "Image";
}

function row(i: Image): HTMLElement {
  const r = h("div", "lib-row" + (i.path === lit ? " lit" : "") + (i.path === copied ? " copied" : ""));
  r.setAttribute("role", "menuitem");
  r.dataset.path = i.path;
  const th = h("span", "lib-th");
  th.dataset.path = i.path;
  seen.observe(th);
  th.addEventListener("pointerdown", (e) => startDrag(e, i, th));
  const text = h("span", "lib-text");
  text.append(h("span", "lib-name", shownName(i)));
  if (i.session) text.append(whoLine(i));
  const right = h("span", "lib-right");
  const x = act("", () => void remove(i), " x");
  x.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
  x.setAttribute("aria-label", "Delete");
  right.append(
    h("span", "lib-time" + (i.path === copied ? " copied" : ""), i.path === copied ? "Copied" : when(i.at)),
    act("Insert", () => void insertImage(i)),
    act(i.pinned ? "Unpin" : "Pin", () => void pin(i)),
    act("Copy path", () => void copyPath(i)),
    x,
  );
  r.append(th, text, right);
  r.addEventListener("mousemove", () => light(i.path));
  r.addEventListener("click", () => openImage(i));
  return r;
}

function section(name: string, n: number): HTMLElement {
  const s = h("div", "lib-sec");
  s.append(name, h("span", "", String(n)));
  return s;
}

function draw() {
  const o = ordered();
  if (!o.some((i) => i.path === lit)) lit = o[0]?.path ?? null;
  libCount.textContent = String(o.length);
  const pins = o.filter((i) => i.pinned).length;
  const kids: HTMLElement[] = [];
  o.forEach((i, n) => {
    if (pins && n === 0) kids.push(section("PINNED", pins));
    if (pins && n === pins) kids.push(section("RECENT", o.length - pins));
    kids.push(row(i));
  });
  seen.disconnect();
  rows.replaceChildren(...kids);
  rows.querySelectorAll<HTMLElement>(".lib-th").forEach((t) => seen.observe(t));
}

function light(path: string | null) {
  if (lit === path) return;
  lit = path;
  rows.querySelectorAll<HTMLElement>(".lib-row").forEach((r) => r.classList.toggle("lit", r.dataset.path === path));
  rows.querySelector<HTMLElement>(".lib-row.lit")?.scrollIntoView({ block: "nearest" });
}

const litImage = () => list.find((i) => i.path === lit);

function openImage(i: Image) {
  invoke("open_image", { path: i.path }).catch(showError);
}

/** Inserts the path into `id`'s pane, as a drop types one. */
async function insertInto(id: string, i: Image) {
  try {
    const t = await invoke<Typed>("image_insert", { path: i.path, who: whoOf(id), host: isRemote() ? hostName() : null });
    typeFiles(id, t);
  } catch (e) {
    showError(e);
  }
}

async function insertImage(i: Image) {
  const id = S.focused;
  if (id && panes.has(id)) await insertInto(id, i);
}

async function pin(i: Image) {
  await invoke("image_pin", { path: i.path, on: !i.pinned }).catch(showError);
}

async function copyPath(i: Image) {
  try {
    await copyText(i.path);
  } catch (e) {
    return showError(e);
  }
  copied = i.path;
  clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => {
    copied = null;
    if (!lib.hidden) draw();
  }, 1500);
  draw();
}

async function remove(i: Image) {
  // The row below takes the light.
  const o = ordered();
  const n = o.findIndex((x) => x.path === i.path);
  lit = (o[n + 1] ?? o[n - 1])?.path ?? null;
  await invoke("image_delete", { path: i.path }).catch(showError);
}

lib.addEventListener("keydown", (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const o = ordered();
  const n = o.findIndex((i) => i.path === lit);
  const i = litImage();
  const k = e.key.toLowerCase();
  if (e.key === "Escape") closeImages();
  else if (e.key === "ArrowDown") light(o[(n + 1) % o.length]?.path ?? null);
  else if (e.key === "ArrowUp") light(o[(n - 1 + o.length) % o.length]?.path ?? null);
  else if (e.key === "Enter" && i) openImage(i);
  else if (k === "i" && i) void insertImage(i);
  else if (k === "p" && i) void pin(i);
  else if (k === "c" && i) void copyPath(i);
  else if (e.key === "Delete" && i) void remove(i);
  else if (e.key !== "Tab") return;
  e.preventDefault();
  e.stopPropagation();
});

document.addEventListener("mousedown", (e) => {
  const t = e.target as Element;
  if (!lib.hidden && !lib.contains(t) && !t.closest(".head-images")) closeImages(false);
}, true);

function expanded(on: boolean) {
  document.querySelectorAll(".head-images").forEach((b) => b.setAttribute("aria-expanded", String(on)));
}

/** Opens the library under `at`, its right edge on the button's right edge. */
function openAt(at: DOMRect) {
  if (!list.length) return;
  menuOpened();
  lit = ordered()[0]?.path ?? null;
  const s = S.focused ? sessions.get(S.focused) : undefined;
  lib.style.setProperty("--pc", accent(s ? place(s)?.project : null));
  lib.hidden = false;
  draw();
  const w = lib.offsetWidth;
  const hgt = lib.offsetHeight;
  lib.style.left = `${Math.max(0, Math.min(at.right - w, window.innerWidth - w - 8))}px`;
  lib.style.top = `${Math.max(0, Math.min(at.bottom, window.innerHeight - hgt - 8))}px`;
  expanded(true);
  lib.focus();
}

export function closeImages(refocus = true) {
  if (lib.hidden) return;
  lib.hidden = true;
  seen.disconnect();
  expanded(false);
  if (refocus) refocusTerminal();
}

/** Palette and pane menu: under the focused pane's button, else at its top right. */
export function openImages() {
  const id = S.focused;
  if (!id) return;
  const cell = document.querySelector<HTMLElement>(`.cell[data-session="${CSS.escape(id)}"]`);
  const b = cell?.querySelector<HTMLElement>(".head-images");
  const at = (b ?? cell?.querySelector<HTMLElement>(".cell-head") ?? cell)?.getBoundingClientRect();
  if (at) openAt(at);
}

/** The pane header button: the newest image, on the focused pane and a hovered header. */
export function imagesButton(id: string): HTMLElement | null {
  const newest = newestImage();
  if (!newest) return null;
  const b = button("head-btn head-images", "", () => {
    if (!lib.hidden) return closeImages();
    const at = b.getBoundingClientRect();
    if (id !== S.focused) focusPane(id);
    openAt(at);
  });
  b.setAttribute("aria-label", "Images");
  b.setAttribute("aria-haspopup", "menu");
  b.setAttribute("aria-expanded", String(!lib.hidden));
  const mini = h("span", "head-mini");
  mini.dataset.path = newest;
  fillThumb(mini, newest);
  b.append(mini);
  return b;
}

/** A thumbnail dragged out of the library: a ghost row under the pointer,
 *  the drop look on the pane under it, and Insert into that pane on release. */
function startDrag(e: PointerEvent, i: Image, th: HTMLElement) {
  if (e.button !== 0) return;
  e.preventDefault();
  const [x0, y0] = [e.clientX, e.clientY];
  let ghost: HTMLElement | null = null;
  const url = th.querySelector("img")?.src ?? null;
  const move = (m: PointerEvent) => {
    if (!ghost && Math.hypot(m.clientX - x0, m.clientY - y0) < 4) return;
    if (!ghost) {
      ghost = h("div", "lib-ghost");
      const mini = h("span", "head-mini");
      if (url) mini.append(Object.assign(h("img"), { src: url, alt: "" }));
      ghost.append(mini, shownName(i));
      document.body.appendChild(ghost);
    }
    ghost.style.left = `${m.clientX + 12}px`;
    ghost.style.top = `${m.clientY + 12}px`;
    showImageDrag(m.clientX, m.clientY, url);
  };
  const up = (u: PointerEvent) => {
    window.removeEventListener("pointermove", move, true);
    window.removeEventListener("pointerup", up, true);
    if (!ghost) return;
    ghost.remove();
    const id = endImageDrag(u.clientX, u.clientY);
    if (!id) return;
    if (id !== S.focused) focusPane(id);
    void insertInto(id, i);
  };
  window.addEventListener("pointermove", move, true);
  window.addEventListener("pointerup", up, true);
}

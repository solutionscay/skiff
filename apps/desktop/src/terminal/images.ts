/**
 * Images: one library of this computer's image cache, newest first. Every
 * drop and Skiff Paste puts its image there. Settings › Images and the Images
 * dialog (`appearance/imagesPage.ts`, `appearance/imagesDialog.ts`) show it
 * as a gallery.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { hostName, isRemote } from "../app/hosts";
import { scheduleRender } from "../app/render";
import { S, sessions } from "../app/state";
import { accent } from "../app/stateQueries";
import { showError } from "../ui/alerts";
import { h } from "../ui/dom";
import { type Typed, typeFiles, whoOf } from "./terminalClipboard";
import { panes } from "./terminalState";
import { locale, t } from "../i18n";

/** One library image, as `images` answers. */
export interface Image {
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
  /** The file's length in bytes. */
  size: number;
}

let list: Image[] = [];

/** Thumbnails by size and path ("160 /path"): a small data URL, or "" for a format the webview cannot draw. */
const thumbs = new Map<string, Promise<string>>();

const MIME: Record<string, string> = { svg: "image/svg+xml", png: "image/png", jpg: "image/jpeg", gif: "image/gif", webp: "image/webp", bmp: "image/bmp", tif: "image/tiff", heic: "image/heic", avif: "image/avif" };

/** Draws the image into a canvas `size` px on its long side, so the page keeps no full-size copy. */
function thumb(path: string, size: number): Promise<string> {
  const key = `${size} ${path}`;
  let t = thumbs.get(key);
  if (!t) {
    t = invoke<ArrayBuffer>("image_thumb", { path, size })
      .then((bytes) => new Promise<string>((done) => {
        const ext = path.split(".").pop()?.toLowerCase() ?? "";
        const url = URL.createObjectURL(new Blob([bytes], { type: MIME[ext] ?? "" }));
        const img = new Image();
        img.onload = () => {
          const c = document.createElement("canvas");
          const scale = Math.min(1, size / Math.max(img.naturalWidth, img.naturalHeight, 1));
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
    thumbs.set(key, t);
  }
  return t;
}

/** Fills `box` with the thumbnail when it is ready. */
export function fillThumb(box: HTMLElement, path: string, size = 160) {
  void thumb(path, size).then((url) => {
    if (!url || box.dataset.path !== path) return;
    const img = h("img");
    img.src = url;
    img.alt = "";
    img.draggable = false;
    box.replaceChildren(img);
  });
}

/** Why the library did not load, as one line. Null when it loaded. */
let loadError: string | null = null;
export const imagesError = () => loadError;

export const imageCount = () => list.length;
export const newestImage = (): string | null => list[0]?.path ?? null;
export const allImages = (): readonly Image[] => list;

/** Called after each load, so Settings › Images redraws too. */
const watchers = new Set<() => void>();
export const onImagesChanged = (f: () => void) => watchers.add(f);

async function load() {
  try {
    list = await invoke<Image[]>("images");
    loadError = null;
  } catch (e) {
    list = [];
    loadError = String(e);
  }
  const live = new Set(list.map((i) => i.path));
  for (const k of thumbs.keys()) if (!live.has(k.slice(k.indexOf(" ") + 1))) thumbs.delete(k);
  changed();
}

/** Redraws Settings › Images and the Images dialog from `list`. */
function changed() {
  scheduleRender("images");
  watchers.forEach((f) => f());
}

/** Loads the library and follows its changes. main.ts calls it once, after every module has loaded: load() draws through render.ts. */
export function startImages() {
  void load();
  void listen("skiff:images-changed", () => void load());
}

/** The gallery's search field: placeholder "Search". */
export function searchField(): HTMLInputElement {
  const f = h("input", "lib-search");
  f.type = "text";
  f.placeholder = t("images.search");
  f.spellcheck = false;
  f.autocomplete = "off";
  f.setAttribute("aria-label", t("images.search"));
  return f;
}

/** Pinned first, then the rest; each newest first. */
export const ordered = () => [...list.filter((i) => i.pinned), ...list.filter((i) => !i.pinned)];

/** Lower case with no accents. Each character stays one character, so a match maps back to the text. */
const foldChar = (c: string) => (c.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase()[0] ?? c);
const fold = (t: string) => [...t].map(foldChar).join("");

/** The query's words, folded. */
export const queryWords = (q: string) => fold(q).split(/\s+/).filter(Boolean);

/** Every word is in the shown name, the session title, the project letters or the project name. */
export function matches(i: Image, words: string[]): boolean {
  if (!words.length) return true;
  const p = i.project ? S.projects.find((x) => x.name === i.project) : undefined;
  const hay = fold([shownName(i), i.title ?? "", p?.short ?? "", i.project ?? ""].join("\n"));
  return words.every((w) => hay.includes(w));
}

/** The matched parts in weight 600, with no color. */
export function marked(text: string, words: string[]): (string | HTMLElement)[] {
  if (!words.length) return [text];
  const chars = [...text];
  const f = chars.map(foldChar).join("");
  const on = new Array<boolean>(chars.length).fill(false);
  for (const w of words) for (let at = f.indexOf(w); at >= 0; at = f.indexOf(w, at + 1)) on.fill(true, at, at + w.length);
  const out: (string | HTMLElement)[] = [];
  for (let n = 0; n < chars.length; ) {
    let e = n;
    while (e < chars.length && on[e] === on[n]) e++;
    const part = chars.slice(n, e).join("");
    out.push(on[n] ? h("mark", "", part) : part);
    n = e;
  }
  return out;
}

export function when(at: number): string {
  const d = new Date(at);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return d.toLocaleDateString(locale(), { month: "short", day: "numeric" });
}

/** Line 2: the project letters in its color and the session title. Dim once that session ended. */
export function whoLine(i: Image, words: string[] = []): HTMLElement {
  const s = i.session ? sessions.get(i.session) : undefined;
  const line = h("span", "lib-who" + (!s || s.state === "done" ? " ended" : ""));
  const p = i.project ? S.projects.find((x) => x.name === i.project) : undefined;
  if (p) {
    const mark = h("span", "lib-proj");
    mark.append(...marked(p.short, words));
    mark.style.color = accent(p);
    line.append(mark);
  }
  line.append(...marked(i.title ?? "", words));
  return line;
}

/** The dropped file's name; "Pasted image" for a paste. An image from
 *  before the index has no entry: "PNG image", or "Image" with no extension,
 *  never its cache hash. */
export function shownName(i: Image): string {
  if (i.name) return i.name;
  if (i.session) return t("images.pasted");
  const ext = i.file.includes(".") ? i.file.split(".").pop()!.toUpperCase() : "";
  return ext ? t("images.ofType", { ext }) : t("images.image");
}

export function section(name: string, n: number): HTMLElement {
  const s = h("div", "lib-sec");
  s.append(name, h("span", "", String(n)));
  return s;
}

/** "8", or "2 of 8" with a query. */
export const countText = (n: number, words: string[]) => (words.length ? t("images.countOf", { n, total: list.length }) : String(list.length));

export function openImage(i: Image) {
  invoke("open_image", { path: i.path }).catch(showError);
}

/** Inserts the paths into `id`'s pane, as a drop of these files types them. False: it failed, and the error shows. */
export async function insertInto(id: string, imgs: readonly Image[]): Promise<boolean> {
  try {
    const t = await invoke<Typed>("image_insert", { paths: imgs.map((i) => i.path), who: whoOf(id), host: isRemote() ? hostName() : null });
    typeFiles(id, t);
    return true;
  } catch (e) {
    showError(e);
    return false;
  }
}

/** The pane Insert types into: the focused one. Null when it has no terminal. */
export const insertTarget = (): string | null => (S.focused && panes.has(S.focused) ? S.focused : null);

export async function pinImages(imgs: readonly Image[], on: boolean) {
  await invoke("image_pin", { paths: imgs.map((i) => i.path), on }).catch(showError);
}

/** The rows go at once; the load after the delete brings back any that stay. */
export async function deleteImages(imgs: readonly Image[]) {
  const gone = new Set(imgs.map((i) => i.path));
  list = list.filter((i) => !gone.has(i.path));
  changed();
  await invoke("image_delete", { paths: [...gone] }).catch((e) => {
    showError(e);
    void load();
  });
}

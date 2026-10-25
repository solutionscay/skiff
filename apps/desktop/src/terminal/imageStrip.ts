/**
 * The image strip: the bottom edge of a pane whose session has sent an
 * image. One cell per image, newest on the left. A click opens it as
 * Settings › Open with › Images says. The list is the session's, in skiffd,
 * so it moves with the session to another pane and survives a restart.
 */
import { invoke } from "@tauri-apps/api/core";

import { S, sessions } from "../app/state";
import { showError } from "../ui/alerts";
import { button, h } from "../ui/dom";
import { icon } from "../ui/icons";

/** Thumbnails by path: a small data URL, or null for a file that is gone. */
const thumbs = new Map<string, Promise<string | null>>();

const MIME: Record<string, string> = { svg: "image/svg+xml", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", bmp: "image/bmp", tif: "image/tiff", heic: "image/heic", avif: "image/avif" };

/** Draws the image into a canvas twice the cell size, so the page keeps no full-size copy. */
function thumb(path: string): Promise<string | null> {
  let t = thumbs.get(path);
  if (!t) {
    t = invoke<ArrayBuffer>("read_image", { path })
      .then((bytes) => new Promise<string | null>((done) => {
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
        // A format the webview cannot draw (HEIC on Linux) shows as an empty cell.
        img.onerror = () => {
          URL.revokeObjectURL(url);
          done("");
        };
        img.src = url;
      }))
      .catch(() => null);
    thumbs.set(path, t);
  }
  return t;
}

function openImage(path: string) {
  invoke("open_image", { path }).catch((e) => {
    // Gone since the strip drew it: the cell shows that now.
    thumbs.set(path, Promise.resolve(null));
    document.querySelectorAll<HTMLElement>(".strip-cell").forEach((c) => {
      if (c.dataset.path === path) c.classList.add("gone");
    });
    showError(e);
  });
}

/** Palette and pane menu: the focused session's newest image. */
export function openLastImage() {
  const s = S.focused ? sessions.get(S.focused) : undefined;
  const newest = s?.images?.[0];
  if (newest) openImage(newest);
}

function cell(path: string): HTMLElement {
  const b = button("strip-cell", "", () => openImage(path));
  b.dataset.path = path;
  b.title = path;
  b.tabIndex = -1;
  b.setAttribute("aria-label", path.split("/").pop() ?? path);
  void thumb(path).then((url) => {
    if (url === null) return b.classList.add("gone");
    if (!url) return;
    const img = h("img");
    img.src = url;
    img.alt = "";
    b.appendChild(img);
  });
  return b;
}

/** Shows, updates or removes the strip under the pane's terminal. Runs on every head sync. */
export function syncStrip(id: string, paneCell: HTMLElement) {
  const list = sessions.get(id)?.images ?? [];
  let strip = paneCell.querySelector<HTMLElement>(":scope > .image-strip");
  if (!list.length) {
    strip?.remove();
    return;
  }
  const sig = list.join("\n");
  if (strip?.dataset.sig === sig) return;
  if (!strip) {
    strip = h("div", "image-strip");
    strip.setAttribute("role", "toolbar");
    strip.setAttribute("aria-label", "Images sent in this session");
    // A wheel turn scrolls the cells sideways.
    const cells = h("div", "strip-cells");
    cells.addEventListener("wheel", (e) => {
      cells.scrollLeft += e.deltaY || e.deltaX;
      e.preventDefault();
    }, { passive: false });
    const clear = button("strip-clear", "", () => {
      invoke("clear_session_images", { session: id }).catch(showError);
    });
    clear.tabIndex = -1;
    clear.setAttribute("aria-label", "Clear the strip");
    clear.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
    strip.append(cells, clear);
    paneCell.appendChild(strip);
  }
  strip.dataset.sig = sig;
  const cells = strip.querySelector<HTMLElement>(".strip-cells")!;
  // Keep the cells that stay, so their thumbnails do not flash.
  const have = new Map([...cells.children].map((c) => [(c as HTMLElement).dataset.path!, c as HTMLElement]));
  cells.replaceChildren(...list.map((p) => have.get(p) ?? cell(p)));
  cells.scrollLeft = 0;
}

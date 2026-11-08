/**
 * The upload line of a drop or a paste to a remote Host. It sits in the
 * pane's bottom left corner: "Uploading 3 of 6 files (4.2 MB of 11.0 MB)…",
 * then "Uploaded 6 files (11.0 MB)." or the failure. A drop that ends
 * within SHOW_MS shows nothing, so a fast link does not flash the pane. The
 * Rust drop path sends the counts (`skiff:upload`) from the Host's replies:
 * nothing more crosses the link. The line redraws at most every REDRAW_MS.
 */
import { listen } from "@tauri-apps/api/event";

import { size } from "../appearance/skiffFolderDialog";
import { tn } from "../i18n";

/** How far one upload is: files and bytes the Host took. */
export interface Upload {
  id: string;
  done: number;
  count: number;
  sent: number;
  size: number;
}

/** A drop that ends sooner shows no line. */
export const SHOW_MS = 400;
/** The line redraws at most this often. */
export const REDRAW_MS = 100;
/** The result line stays this long. */
export const DONE_MS = 2000;
export const FAILED_MS = 5000;

/** The line while files go: the file that goes now, of all. */
export const sendingLine = (u: Upload) =>
  tn("upload.sending", u.count, { current: Math.min(u.done + 1, u.count), sent: size(u.sent), size: size(u.size) });
export const doneLine = (u: Upload) => tn("upload.done", u.count, { size: size(u.size) });
export const failedLine = (u: Upload) => tn("upload.failed", u.count, { done: u.done });

/** The pane's cell, where the line goes. */
const cellOf = (pane: string) => [...document.querySelectorAll<HTMLElement>(".cell")].find((c) => c.dataset.session === pane);

const lines = new Map<string, UploadLine>();
let seq = 0;

/** One upload's line in one pane. */
export class UploadLine {
  readonly id = `upload-${++seq}`;
  private at: Upload;
  private el: HTMLElement | null = null;
  /** SHOW_MS went by. */
  private shown = false;
  /** The drop path sent a count: the files are read and going. */
  private got = false;
  private ended = false;
  private redraw: number | undefined;
  private lastDraw = 0;
  private wait: number;

  constructor(private pane: string, count: number) {
    this.at = { id: this.id, done: 0, count, sent: 0, size: 0 };
    lines.set(this.id, this);
    this.wait = window.setTimeout(() => {
      this.shown = true;
      this.draw();
    }, SHOW_MS);
  }

  /** A count from the drop path. Redraws at most every REDRAW_MS. */
  update(u: Upload) {
    if (this.ended) return;
    this.at = u;
    this.got = true;
    if (!this.shown || this.redraw !== undefined) return;
    const due = this.lastDraw + REDRAW_MS - Date.now();
    if (due <= 0) return this.draw();
    this.redraw = window.setTimeout(() => {
      this.redraw = undefined;
      this.draw();
    }, due);
  }

  /** The drop is over. A line that showed says how it ended. */
  end(ok: boolean) {
    if (this.ended) return;
    this.ended = true;
    window.clearTimeout(this.wait);
    window.clearTimeout(this.redraw);
    lines.delete(this.id);
    if (!this.shown || !this.got) return;
    if (ok) this.at = { ...this.at, done: this.at.count, sent: this.at.size };
    this.paint(ok ? doneLine(this.at) : failedLine(this.at), !ok);
    window.setTimeout(() => this.el?.remove(), ok ? DONE_MS : FAILED_MS);
  }

  private draw() {
    if (this.ended || !this.got) return;
    this.lastDraw = Date.now();
    this.paint(sendingLine(this.at), false);
  }

  private paint(text: string, failed: boolean) {
    const cell = cellOf(this.pane);
    if (!cell) return this.el?.remove();
    if (!this.el) {
      // A newer drop on the same pane takes the corner.
      cell.querySelector(".upload-line")?.remove();
      this.el = document.createElement("div");
      this.el.className = "upload-line";
      this.el.setAttribute("role", "status");
    }
    this.el.classList.toggle("failed", failed);
    this.el.textContent = text;
    if (this.el.parentElement !== cell) cell.append(this.el);
  }
}

void listen<Upload>("skiff:upload", ({ payload }) => lines.get(payload.id)?.update(payload));

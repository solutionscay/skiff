/**
 * Files dragged from another app onto a pane. While they are over a pane,
 * its body shows the drop look and one label. On drop, the backend copies
 * each image to the cache and Skiff types the paths as a paste. The drop
 * focuses the pane; the current row stays. Anywhere else, a drop does nothing.
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { isRemote, hostName } from "../app/hosts";
import { S, sessions } from "../app/state";
import { showError } from "../ui/alerts";
import { modalOpen } from "../ui/dom";
import { isSlot } from "../workspace/layoutSlots";
import { focusPane } from "./paneActions";
import { previewOnScreen } from "./peek";
import { type Typed, typeFiles } from "./terminalClipboard";
import { panes } from "./terminalState";
import { view } from "./terminal";

const overlay = document.createElement("div");
overlay.className = "drop-preview file-drop";
overlay.hidden = true;
const label = document.createElement("span");
label.className = "drag-ghost";
overlay.appendChild(label);
document.body.appendChild(overlay);

/** Paths of the drag in progress. `over` events carry only the position. */
let dragged: string[] = [];

/** The live session pane under a window position, in CSS pixels. */
function paneAt(x: number, y: number): string | null {
  if (modalOpen() || previewOnScreen()) return null;
  for (const [id, r] of view.cellRects()) {
    if (x < r.left || x >= r.right || y < r.top || y >= r.bottom) continue;
    const s = sessions.get(id);
    if (isSlot(id) || !s || s.state === "done" || !panes.has(id)) return null;
    return id;
  }
  return null;
}

function show(id: string | null) {
  const body = id ? view.body(id) : null;
  if (!body || !dragged.length) {
    overlay.hidden = true;
    return;
  }
  const r = body.getBoundingClientRect();
  Object.assign(overlay.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
  label.textContent = dragged.length === 1 ? "Type the path" : `Type ${dragged.length} paths`;
  overlay.hidden = false;
}

async function drop(id: string, paths: string[]) {
  if (id !== S.focused) focusPane(id);
  try {
    const t = await invoke<Typed>("drop_files", { session: id, paths, host: isRemote() ? hostName() : null });
    typeFiles(id, t);
  } catch (e) {
    showError(e);
  }
}

void getCurrentWebview().onDragDropEvent(({ payload: p }) => {
  if (p.type === "leave") {
    dragged = [];
    return show(null);
  }
  if (p.type === "enter") dragged = p.paths;
  const ratio = window.devicePixelRatio || 1;
  const id = paneAt(p.position.x / ratio, p.position.y / ratio);
  if (p.type !== "drop") return show(id);
  const paths = p.paths.length ? p.paths : dragged;
  dragged = [];
  show(null);
  if (id && paths.length) void drop(id, paths);
});

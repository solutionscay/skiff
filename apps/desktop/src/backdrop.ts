/** The project's background image: one picture behind every pane of the terminal area. */
import { invoke } from "@tauri-apps/api/core";
import { host } from "./terminalHost";
import { place, S, sessions } from "./state";

const OPACITY_KEY = "skiff.paneOpacity";
const OPACITY_MIN = 0.4;
const OPACITY_DEFAULT = 0.85;

let opacity = (() => {
  try {
    const v = Number(localStorage.getItem(OPACITY_KEY));
    return v >= OPACITY_MIN && v <= 1 ? v : OPACITY_DEFAULT;
  } catch {
    return OPACITY_DEFAULT;
  }
})();

/** How opaque the panes are over a background image. The image never shows at 1. */
export const paneOpacity = () => opacity;
export const PANE_OPACITY_MIN = OPACITY_MIN;

export function setPaneOpacity(v: number) {
  opacity = Math.min(1, Math.max(OPACITY_MIN, v));
  try {
    localStorage.setItem(OPACITY_KEY, String(opacity));
  } catch {
    /* the setting lasts for this run */
  }
  host.style.setProperty("--pane-alpha", String(opacity));
}

/** Blob URLs by file path. A path that failed to load stays out, so it is tried again. */
const urls = new Map<string, string>();
const loading = new Set<string>();
let shown: string | null = null;

async function load(path: string) {
  if (urls.has(path) || loading.has(path)) return;
  loading.add(path);
  try {
    const bytes = await invoke<ArrayBuffer>("read_image", { path });
    urls.set(path, URL.createObjectURL(new Blob([bytes])));
  } catch (e) {
    console.error(e);
  } finally {
    loading.delete(path);
  }
  if (path === wanted()) apply();
}

/** The focused session's project, else the selected one. */
function wanted(): string | null {
  const s = S.focused ? sessions.get(S.focused) : undefined;
  const p = (s && place(s)?.project) ?? S.projects.find((x) => x.name === S.selectedProject);
  return p?.background ?? null;
}

function apply() {
  const path = wanted();
  const url = path ? urls.get(path) ?? null : null;
  host.classList.toggle("has-bg", !!url);
  host.style.setProperty("--pane-alpha", url ? String(opacity) : "1");
  if (url !== shown) {
    shown = url;
    host.style.setProperty("--bg-image", url ? `url("${url}")` : "none");
  }
}

/** Call on every render. Cheap when nothing changed. */
export function applyBackdrop() {
  const path = wanted();
  if (path && !urls.has(path)) void load(path);
  apply();
}

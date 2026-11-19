/**
 * The clipboard, in the order that works on each platform:
 *
 * 1. GTK on the main thread (Linux). The webview's own write needs a user
 *    gesture, and the copy key is a native menu accelerator, so it reaches
 *    the page as a menu event with no gesture. GTK writes to the display the
 *    window is on, Wayland or X11.
 * 2. The webview (`navigator.clipboard`): macOS, and any Linux call that
 *    comes from a real gesture.
 * 3. The Tauri plugin. On Linux it reaches for X11 and fails on Wayland.
 */
import { invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

/** For the clipboard trace: the path that ran, and the errors of the paths tried before it. */
export interface ClipTry {
  path?: "gtk" | "webview" | "plugin";
  failed: string[];
}

const note = (tr: ClipTry | undefined, path: ClipTry["path"], e: unknown) => tr?.failed.push(`${path}: ${String(e)}`);

export async function copyText(text: string, tr?: ClipTry): Promise<void> {
  try {
    await invoke("clipboard_write", { text });
    if (tr) tr.path = "gtk";
    return;
  } catch (e) {
    /* not Linux */
    note(tr, "gtk", e);
  }
  try {
    await navigator.clipboard.writeText(text);
    if (tr) tr.path = "webview";
  } catch (e) {
    note(tr, "webview", e);
    if (tr) tr.path = "plugin";
    await writeText(text);
  }
}

export async function pasteText(tr?: ClipTry): Promise<string> {
  try {
    const text = await invoke<string>("clipboard_read");
    if (tr) tr.path = "gtk";
    return text;
  } catch (e) {
    /* not Linux */
    note(tr, "gtk", e);
  }
  try {
    const text = await readText();
    if (tr) tr.path = "plugin";
    return text;
  } catch (e) {
    note(tr, "plugin", e);
    if (tr) tr.path = "webview";
    return navigator.clipboard.readText();
  }
}

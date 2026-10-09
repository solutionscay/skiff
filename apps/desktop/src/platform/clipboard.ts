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

export async function copyText(text: string): Promise<void> {
  try {
    return await invoke("clipboard_write", { text });
  } catch {
    /* not Linux */
  }
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    await writeText(text);
  }
}

export async function pasteText(): Promise<string> {
  try {
    return await invoke<string>("clipboard_read");
  } catch {
    /* not Linux */
  }
  try {
    return await readText();
  } catch {
    return navigator.clipboard.readText();
  }
}

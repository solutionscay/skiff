/**
 * The clipboard through the webview first: WebKitGTK goes through GTK, which
 * works on Wayland and X11 alike. The Tauri plugin reaches for X11 and fails on
 * a Wayland session ("X11 server connection timed out"), so it is the fallback.
 */
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    await writeText(text);
  }
}

export async function pasteText(): Promise<string> {
  try {
    return await readText();
  } catch {
    return navigator.clipboard.readText();
  }
}

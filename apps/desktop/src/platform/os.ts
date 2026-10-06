import { invoke } from "@tauri-apps/api/core";

/** Opens a file or folder in the OS default app. */
export const openPath = (path: string) => invoke<void>("open_path", { path });
/** Opens a web address in the default browser. */
export const openUrl = (url: string) => invoke<void>("open_url", { url });

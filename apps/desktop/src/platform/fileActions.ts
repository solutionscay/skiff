import { invoke } from "@tauri-apps/api/core";
import { showError } from "../ui/alerts";
export { openPath as open } from "../terminal/peek";
export const reveal = (path: string) => void invoke("reveal_file", { path }).catch(showError);
export const copy = (path: string) => void navigator.clipboard.writeText(path).catch(showError);

import { invoke } from "@tauri-apps/api/core";
import { showError } from "../ui/alerts";
import { openPath } from "./os";
export { openPath as open } from "../terminal/peek";
export const openFolder = (path: string) => void openPath(path).catch(showError);
export const reveal = (path: string) => void invoke("reveal_file", { path }).catch(showError);
export const copy = (path: string) => void navigator.clipboard.writeText(path).catch(showError);

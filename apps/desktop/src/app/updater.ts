/**
 * App updates: a quiet check after the launch, Help › Check for updates…,
 * and the top-bar badge once an update is downloaded. Only the person's
 * click restarts the app. The new app hands skiffd over at its launch, so
 * sessions keep running.
 */
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "../platform/os";
import { showError, showNotice } from "../ui/alerts";
import { $, button, h } from "../ui/dom";
import { leave } from "../workspace/profile";
import { failLine, releasePage } from "./updateNotice";

/** The quiet check waits for the launch to settle. */
const QUIET_MS = 10_000;

let update: Update | null = null;
let state: "idle" | "checking" | "downloading" | "ready" = "idle";

/** Called once the launch is done. Reports nothing unless an update is ready. */
export function startUpdates() {
  setTimeout(() => void find(false), QUIET_MS);
}

/** Help › Check for updates… and the palette entry. */
export function checkForUpdates() {
  if (state === "ready") return openPop();
  if (state === "idle") void find(true);
}

/**
 * Checks, then downloads. `asked`: the person chose Check for updates…, so
 * the result and any fault show. The quiet check only logs a fault.
 */
async function find(asked: boolean) {
  const off = await invoke<string | null>("update_off").catch((e) => String(e));
  if (off) return asked ? showNotice(off) : undefined;
  state = "checking";
  let found: Update | null;
  try {
    found = await check();
  } catch (e) {
    state = "idle";
    return asked ? showError(failLine("check", e)) : console.error(e);
  }
  if (!found) {
    state = "idle";
    return asked ? showNotice(`Skiff ${await getVersion()} is the latest version.`) : undefined;
  }
  state = "downloading";
  badge();
  try {
    // The updater checks the signature here, before anything is installed.
    await found.download();
  } catch (e) {
    state = "idle";
    badge();
    void found.close();
    return asked ? showError(failLine("download", e), retry(found.version)) : console.error(e);
  }
  update = found;
  state = "ready";
  badge();
  if (asked) openPop();
}

/** The release page, for an install that cannot run here. */
const retry = (version: string) => ({ label: "Release page", run: () => openUrl(releasePage(version)) });

/**
 * Restart to update: the workspace save guard first, then the install, then
 * the restart. Cancel at the guard installs nothing. A failed install, such
 * as a cancelled password prompt for a .deb or .rpm, keeps this app running.
 */
function restart() {
  const u = update;
  if (!u) return;
  void leave("Skiff restarts to finish the update. Sessions keep running.", async () => {
    try {
      await u.install();
    } catch (e) {
      showError(failLine("install", e), retry(u.version));
      return false;
    }
    try {
      await invoke("relaunch_app");
      return true;
    } catch (e) {
      showError(e);
      return false;
    }
  });
}

function badge() {
  document.getElementById("update-badge")?.remove();
  document.getElementById("update-pop")?.remove();
  if (state !== "downloading" && state !== "ready") return;
  const b = button("tb-cell update-badge", state === "ready" ? "Update ready" : "Downloading update", () => {
    if (state !== "ready") return;
    document.getElementById("update-pop") ? closePop() : openPop();
  });
  b.id = "update-badge";
  b.disabled = state !== "ready";
  b.setAttribute("aria-haspopup", "dialog");
  $("open-settings").before(b);
}

const outside = (e: MouseEvent) => {
  const pop = document.getElementById("update-pop");
  const b = document.getElementById("update-badge");
  if (pop && !pop.contains(e.target as Node) && !b?.contains(e.target as Node)) closePop();
};

/** A panel under the badge: the version, what a restart changes, and the choice. */
function openPop() {
  const b = document.getElementById("update-badge");
  if (!b || !update || document.getElementById("update-pop")) return;
  const back = document.activeElement as HTMLElement | null;
  const pop = h("div", "");
  pop.id = "update-pop";
  pop.setAttribute("role", "dialog");
  const title = `Update to Skiff ${update.version}`;
  pop.setAttribute("aria-label", title);
  const later = button("confirm-cancel", "Not now", () => closePop(back));
  const go = button("confirm-act", "Restart to update", () => {
    closePop(back);
    restart();
  });
  const foot = h("div", "confirm-foot");
  foot.append(later, go);
  pop.append(h("div", "confirm-title", title), h("div", "confirm-body", "Skiff restarts. Sessions keep running."), foot);
  pop.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    closePop(back);
  });
  const r = b.getBoundingClientRect();
  pop.style.top = `${r.bottom + 4}px`;
  pop.style.right = `${Math.max(8, window.innerWidth - r.right)}px`;
  document.body.appendChild(pop);
  b.setAttribute("aria-expanded", "true");
  window.addEventListener("mousedown", outside, true);
  later.focus();
}

/** Focus goes back to what had it before the panel opened. */
function closePop(back?: HTMLElement | null) {
  document.getElementById("update-pop")?.remove();
  document.getElementById("update-badge")?.setAttribute("aria-expanded", "false");
  window.removeEventListener("mousedown", outside, true);
  if (back?.isConnected) back.focus();
}

/** A daemon that differs from the app: the top-bar badge, and the restart it offers. */
import { invoke } from "@tauri-apps/api/core";
import type { DaemonWarning, SessionInfo } from "../platform/types";
import { showError } from "../ui/alerts";
import { dialogFrame } from "../ui/dialogParts";
import { $, button, h } from "../ui/dom";
import { branchName, taskTitle } from "../workspace/model";
import { sessions } from "./state";
import { place } from "./stateQueries";

/** Badge text and panel title per warning. */
const WARNINGS: Record<DaemonWarning["kind"], [string, string]> = {
  outdated: ["Update pending", "Restart skiffd to finish the update"],
  protocol: ["skiffd incompatible", "skiffd does not match this app"],
  newer: ["skiffd newer", "skiffd is newer than this app"],
  hung: ["skiffd not responding", "skiffd is not responding"],
};

let shown: DaemonWarning | null = null;
/** Set by "Restart when idle": the restart waits until no agent works. */
let whenIdle = false;

const live = () => [...sessions.values()].filter((s) => s.state !== "done");
const working = () => live().filter((s) => s.state === "working");
const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

async function restartNow() {
  whenIdle = false;
  try {
    await invoke("restart_daemon");
    location.reload();
  } catch (e) {
    showError(e);
    daemonBadge(shown);
  }
}

/** Called on every state change. Restarts once the last agent stops working. */
export function restartIfIdle() {
  if (whenIdle && !working().length) void restartNow();
}

/**
 * A cell in the top bar, beside Settings. It opens a panel that says what a
 * restart costs, so the restart stays the user's call. A daemon the app
 * cannot work with opens the panel at once.
 */
export function daemonBadge(warning: DaemonWarning | null) {
  shown = warning;
  document.getElementById("daemon-badge")?.remove();
  document.getElementById("daemon-pop")?.remove();
  if (!warning) return;
  const badge = button("tb-cell daemon-badge", "", () => (pop.isConnected ? close() : open()));
  badge.id = "daemon-badge";
  const [label, title] = WARNINGS[warning.kind];
  badge.append(h("span", "dot " + (warning.kind === "newer" ? "idle" : "waiting")), whenIdle ? "Restarts when idle" : label);
  badge.setAttribute("aria-haspopup", "dialog");
  $("open-settings").before(badge);

  const pop = h("div", "");
  pop.id = "daemon-pop";
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", title);
  const later = button("confirm-cancel", whenIdle ? "Keep waiting" : "Not now", () => close());
  const restart = whenIdle
    ? button("confirm-act", "Cancel restart", () => {
        whenIdle = false;
        daemonBadge(shown);
      })
    : button("confirm-act", "Restart skiffd…", () => {
        close();
        // A hung daemon lists nothing: there is nothing to choose from.
        if (warning.kind === "hung" || !live().length) void restartNow();
        else restartDialog();
      });
  const foot = h("div", "confirm-foot");
  foot.append(later, restart);
  const body = whenIdle
    ? `skiffd restarts when no agent is working. ${plural(working().length, "agent is", "agents are")} working now.`
    : warning.message;
  pop.append(h("div", "confirm-title", title), h("div", "confirm-body", body), foot);

  const outside = (e: MouseEvent) => {
    if (!pop.contains(e.target as Node) && !badge.contains(e.target as Node)) close();
  };
  function open() {
    const r = badge.getBoundingClientRect();
    pop.style.top = `${r.bottom + 4}px`;
    pop.style.right = `${Math.max(8, window.innerWidth - r.right)}px`;
    document.body.appendChild(pop);
    badge.setAttribute("aria-expanded", "true");
    window.addEventListener("mousedown", outside, true);
    later.focus();
  }
  function close() {
    pop.remove();
    badge.setAttribute("aria-expanded", "false");
    window.removeEventListener("mousedown", outside, true);
  }
  pop.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    close();
    badge.focus();
  });
  if (!whenIdle && (warning.kind === "protocol" || warning.kind === "hung")) requestAnimationFrame(open);
}

/** The sessions a restart stops, by project and worktree, with what each one does. */
function sessionList(list: SessionInfo[]): HTMLElement {
  const byPlace = new Map<string, SessionInfo[]>();
  for (const s of list) {
    const at = place(s);
    const key = at ? `${at.project.name} / ${branchName(at.worktree)}` : "Other";
    byPlace.set(key, [...(byPlace.get(key) ?? []), s]);
  }
  const box = h("div", "restart-list");
  for (const [key, group] of byPlace) {
    box.append(h("div", "restart-place", key));
    for (const s of group) {
      const row = h("div", "restart-row");
      row.append(h("span", "dot " + s.state), h("span", "restart-name", taskTitle(s)), h("span", "restart-state", s.state));
      box.append(row);
    }
  }
  return box;
}

/** Restart now, when idle, or not at all. */
function restartDialog() {
  const back = document.activeElement as HTMLElement | null;
  const list = live();
  const busy = working().length;
  const waiting = list.filter((s) => s.state === "waiting").length;
  const { overlay, panel } = dialogFrame("confirm-panel", "alertdialog", "Restart skiffd");
  const lines = [`${plural(list.length, "session stops", "sessions stop")}. Each pane comes back as a shell in its folder.`];
  if (busy) lines.push(`${plural(busy, "agent is", "agents are")} working.`);
  if (waiting) lines.push(`${plural(waiting, "agent waits", "agents wait")} for input.`);
  const cancel = button("confirm-cancel", "Cancel", () => done());
  const idle = button("confirm-cancel", "When idle", () => {
    whenIdle = true;
    done();
    restartIfIdle();
    daemonBadge(shown);
  });
  const now = button("confirm-act", "Restart now", () => {
    done();
    void restartNow();
  });
  idle.title = "Restart when no agent is working";
  const foot = h("div", "confirm-foot");
  foot.append(cancel, ...(busy ? [idle] : []), now);
  panel.append(h("div", "confirm-title", "Restart skiffd"), h("div", "confirm-body", lines.join(" ")), sessionList(list), foot);
  overlay.appendChild(panel);
  document.body.appendChild(overlay);

  function done() {
    overlay.remove();
    back?.focus?.();
  }
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) done();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key !== "Escape") return;
    e.preventDefault();
    done();
  });
  cancel.focus();
}

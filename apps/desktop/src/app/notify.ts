import { namesFor } from "./flatNames";
/** Desktop notifications from the app: a session waits, and its pane does not
 *  have the keys. They go through the system as Skiff, with its icon. The
 *  rule is in `waits.ts`. The app starts `skiff watch` with `--no-notify`, so
 *  the watcher does not send its own.
 *
 *  On macOS a bundled app sends through the system notification center
 *  (`notify_show`): one per session, with a subtitle, removed when the pane is
 *  seen. Elsewhere, or when that fails, the notification plugin sends. */

import { foreground, hostFields, invokeAt, pageRoute, withinHost, type HostRoute } from "../platform/invoke";
import { listen } from "@tauri-apps/api/event";
import { onAction, removeActive, sendNotification } from "@tauri-apps/plugin-notification";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { t } from "../i18n";
import type { SessionInfo } from "../platform/types";
import { isMac } from "./keys";
import { hasKeys } from "./seen";
import { skiffAgentName } from "./skiffAgent";
import { sessions } from "./state";
import { place } from "./stateQueries";
import { Waits } from "./waits";
import { reported } from "../workspace/programStatus";
import { basename, samePath, taskTitle } from "../workspace/model";
import { liveRoute, openAttention } from "./attention";

let waits = new Waits();

/** The notification shown for each session: `"center"` for the system
 *  notification center, or its id in the plugin. */
let shown = new Map<string, "center" | number>();
let nextId = 1;
hostFields(() => ({waits:new Waits(),shown:new Map<string,"center" | number>()}), () => ({waits,shown}), f => { ({waits,shown} = f); });

/** Waits this long before it sends, so the report that comes with the wait
 *  (its question) is in, and a pane that took the keys meanwhile is left out. */
const SETTLE_MS = 250;

/** The pane's name, as the tree shows it. */
const nameOf = (s: SessionInfo) => namesFor("session")(s.id,s.coordinator ? skiffAgentName() : taskTitle(s));

/** The agent's question, when it reported one. */
function question(s: SessionInfo): string | null {
  const p = reported(s);
  return p?.state === "blocked" && p.msg?.trim() ? p.msg.trim() : null;
}

/** A session's `state` event. */
export function stateMoved(id: string, state: string) {
  if (!waits.moved(id, state, Date.now(), foreground() && hasKeys(id))) return;
  const route = pageRoute();
  setTimeout(() => { if (route && liveRoute(route)) withinHost(route, () => send(route,id)); }, SETTLE_MS);
}

/** `project · dir`, or the one of them there is. The folder is left out at the
 *  root of the project's main worktree. */
function subtitle(s: SessionInfo): string {
  const at = place(s);
  const dir = at && at.worktree.is_main && samePath(s.cwd, at.worktree.path) ? "" : basename(s.cwd);
  if (!at) return dir;
  return dir ? t("notify.subtitle", { project: at.project.name, dir }) : at.project.name;
}

function send(route: HostRoute, id: string) {
  const s = sessions.get(id);
  if (!s || s.state !== "waiting" || (foreground() && hasKeys(id))) return;
  const body = question(s) ?? t("notify.waiting");
  if (!isMac) return sendPlugin(id, s, body);
  shown.set(id, "center");
  const owner = shown;
  const request = notificationId(id);
  invokeAt(route, "notify_show", { session: request, thread: notificationKey(id), title: nameOf(s), subtitle: subtitle(s), body })
    .then(() => {
      // The first one waits for the permission answer: the pane may have been seen meanwhile.
      if (owner.get(id) !== "center") invokeAt(route, "notify_remove", { session: request }).catch(console.error);
    })
    .catch((e) => {
      console.warn("notify: the notification center did not take it, the plugin sends:", e);
      if (owner.get(id) !== "center" || !liveRoute(route)) return;
      owner.delete(id);
      withinHost(route, () => {
        const now = sessions.get(id);
        if (now?.state === "waiting" && !(foreground() && hasKeys(id))) sendPlugin(id, now, body);
      });
    });
}

function sendPlugin(id: string, s: SessionInfo, body: string) {
  const project = place(s)?.project.name;
  const name = nameOf(s);
  const n = nextId++;
  shown.set(id, n);
  // The plugin shows title and body only on the desktop: the project goes into the title.
  sendNotification({
    id: n,
    title: project ? t("notify.title", { name, project }) : name,
    body,
    // On the desktop the plugin ignores the thread. The session id is kept for a click.
    group: notificationId(id),
    extra: { session: notificationKey(id) },
    ...(isMac ? { sound: "NSUserNotificationDefaultSoundName" } : {}),
  });
}

/** The pane has the keys: its notification goes. The plugin cannot remove a
 *  shown notification on macOS or Windows; there this does nothing. */
export function paneSeen(id: string) {
  const n = shown.get(id);
  if (n === undefined) return;
  shown.delete(id);
  if (n === "center") {
    const route = pageRoute();
    if (route) invokeAt(route,"notify_remove", { session: notificationId(id) }).catch(console.error);
  }
  else removeActive([{ id: n }]).catch(() => {});
}

/** The session is gone. */
export function sessionGone(id: string) {
  waits.forget(id);
  paneSeen(id);
}

/** A click on a notification brings the window forward and opens the pane. */
export function watchNotificationClicks() {
  onAction((a) => opened(a.notification?.extra?.session)).catch(console.error);
  if (isMac) listen<string>("skiff:notify-click", (e) => opened(e.payload)).catch(console.error);
}

function notificationId(id: string) {
  return JSON.stringify([pageRoute()?.host, id]);
}

function notificationKey(id: string) {
  return JSON.stringify([pageRoute(), id]);
}

function opened(key: unknown) {
  if (typeof key !== "string") return;
  try {
    const [route, session] = JSON.parse(key);
    if (!route || typeof session !== "string" || !openAttention(route, session)) return;
  } catch { return; }
  const w = getCurrentWindow();
  void w
    .unminimize()
    .then(() => w.show())
    .then(() => w.setFocus())
    .catch(console.error);
}

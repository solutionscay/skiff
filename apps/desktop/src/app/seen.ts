/** What the user has seen: the pane with the keys, while the window is in front. */
import { invoke } from "@tauri-apps/api/core";

import { S, sessions } from "./state";

/** True when `id` is the focused pane of the active window. Its result is in front of the user. */
export const hasKeys = (id: string) => S.focused === id && document.hasFocus();

/** Drops the session's unread flag and failed mark, here and in the daemon. */
export function markSeen(id: string) {
  const s = sessions.get(id);
  if (!s || (!s.unread && s.agent_exit == null)) return;
  s.unread = false;
  s.agent_exit = null;
  invoke("session_seen", { session: id }).catch(console.error);
}

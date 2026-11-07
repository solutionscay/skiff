/** Starts a session and places it, unless the person moved while it started. */
import { placeInSlot } from "../workspace/canvas";
import { agentCallsign } from "../appearance/agentNames";
import type { SessionInfo, SplitDir } from "../platform/types";
import { render } from "./render";
import { born, S, sessions, upsert } from "./state";
import { previewToken } from "../terminal/peek";
import { panes } from "../terminal/terminalState";
import { inBackground, showSingle, splitWith } from "../workspace/view";
import { hostInvoke } from "./hostCaps";

/** A new session is the single view, unless it came from a pane's Split menu. */
export async function newSession(
  cwd: string | null,
  agent: string | null,
  label: string,
  role: "agent" | "shell",
  where?: { target: string; dir: SplitDir } | { slot: string },
  /** A fixed name, such as the Skiff agent's, in place of a call sign. */
  name?: string,
) {
  // Projects list agents as command lines; the first word is the program.
  const words = (agent ?? "").trim().split(/\s+/).filter(Boolean);
  // The user owns the request until they move: a click or key elsewhere, or a new preview.
  const mark = () => `${previewToken()} ${S.roveKey} ${S.focused} ${S.activeGroup}`;
  const before = mark();
  const visible = S.focused ? panes.get(S.focused) : undefined;
  // Agent IDs are useful in the launch menu, but call signs make a new crew
  // easier (and more fun) to tell apart once it is running.
  const sessionLabel = name ?? (role === "agent" ? agentCallsign([...sessions.values()].map((s) => s.label)) : label);
  const info = await hostInvoke<SessionInfo>("session_lifecycle", "create_session", {
    spec: {
      label: sessionLabel,
      name: role === "agent" ? sessionLabel : null,
      role,
      cwd,
      command: words[0] ?? null,
      args: words.slice(1),
      cols: visible?.term.cols ?? 120,
      rows: visible?.term.rows ?? 40,
    },
  });
  for (const b of born) b.add(info.id);
  upsert(info);
  // The user may have moved to a preview while it started: it shows under the preview.
  const place = () => {
    if (where && "slot" in where) placeInSlot(where.slot, info.id);
    else if (where) splitWith(where.target, where.dir, info.id);
    else showSingle(info.id);
  };
  // Not moved: it shows and takes the keys. Moved: it waits in the list, and a
  // split or empty pane it was asked for still gets it, without the keys.
  if (mark() === before) place();
  else if (where) inBackground(place, true);
  else render();
  return info.id;
}

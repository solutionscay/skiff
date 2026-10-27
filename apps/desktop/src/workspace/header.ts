import { slotsOf } from "./layoutSlots";

import { sessionsOf } from "./layout";

import { glyph } from "./layoutIcon";
import { branchName, taskTitle } from "./model";

import { $, h, setText } from "../ui/dom";

import { S, sessions } from "../app/state";
import { accent, activeGroupObj, currentProject, place } from "../app/stateQueries";

/** Banner: which pane gets the next key, and the group it belongs to. */
export function renderHeader() {
  const s = S.focused ? sessions.get(S.focused) : undefined;
  const at = s ? place(s) : null;
  const main = $<HTMLElement>("main");
  main.style.setProperty("--pc", accent(at?.project ?? currentProject()));

  const label = $("focus-label");
  const keysTo = $("keys-to");
  const g = activeGroupObj();
  const groupLabel = $("group-label");
  // Renders run many times a second: the label is rebuilt only when it changes.
  const sig = g ? JSON.stringify([g.name, g.layout]) : "";
  if (groupLabel.dataset.sig !== sig) {
    groupLabel.dataset.sig = sig;
    groupLabel.replaceChildren();
    if (g) {
      const n = sessionsOf(g.layout).length;
      const empty = slotsOf(g.layout).length;
      groupLabel.append(glyph(g.layout), h("span", "", g.name), h("span", "dim", empty ? `${empty} of ${n} empty` : `${n} panes`));
    }
  }
  if (!s) {
    setText(keysTo, g && slotsOf(g.layout).length ? "pick an agent for each empty pane" : "no session to type into");
    setText(label, "");
    return;
  }
  setText(keysTo, "keys go to");
  setText(label, at
    ? `${at.project.name} / ${branchName(at.worktree)} / ${taskTitle(s)}`
    : `${s.cwd} / ${taskTitle(s)}`);
}

import { namesFor } from "../app/flatNames";
import { slotsOf } from "./layoutSlots";

import { sessionsOf } from "./layout";

import { glyph } from "./layoutIcon";
import { branchName, taskTitle } from "./model";

import { $, h, setText } from "../ui/dom";

import { S, sessions } from "../app/state";
import { accent, activeGroupObj, currentProject, place } from "../app/stateQueries";
import { t, tn } from "../i18n";

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
  const groupName = g ? namesFor("group")(g.id,g.name) : "";
  const sig = g ? JSON.stringify([groupName, g.layout]) : "";
  if (groupLabel.dataset.sig !== sig) {
    groupLabel.dataset.sig = sig;
    groupLabel.replaceChildren();
    if (g) {
      const n = sessionsOf(g.layout).length;
      const empty = slotsOf(g.layout).length;
      groupLabel.append(glyph(g.layout), h("span", "", groupName), h("span", "dim", empty ? t("topbar.emptyOf", { empty, n }) : tn("topbar.panes", n)));
    }
  }
  if (!s) {
    setText(keysTo, t(g && slotsOf(g.layout).length ? "topbar.pickAgents" : "topbar.noSession"));
    setText(label, "");
    return;
  }
  setText(keysTo, t("topbar.keysGoTo"));
  const title = namesFor("session")(s.id,taskTitle(s));
  setText(label, at
    ? `${namesFor("project")(at.project.path,at.project.name)} / ${namesFor("worktree")(at.worktree.path,branchName(at.worktree))} / ${title}`
    : `${s.cwd} / ${title}`);
}

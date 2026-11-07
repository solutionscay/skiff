/** A session the daemon no longer has leaves the view: its pane, its groups, and the keys. */
import { filledOf, isSlot } from "../workspace/layoutSlots";
import { removePane, sessionsOf } from "../workspace/layout";
import { deleteGroup, syncTemplateName } from "./groups";
import { render } from "./render";
import { gone, S, sessions } from "./state";
import { activeGroupObj, shownIds } from "./stateQueries";
import { heirOf } from "./skiffAgent";
import { panes } from "../terminal/terminalState";
import { focusPane, inBackground, paneNear, revealSession, rowNear, unfocus } from "../workspace/view";

/** The daemon pruned the session from its groups; groups_changed brings them. Here the view drops it at once. */
export function dropSession(id: string) {
  inBackground(() => dropPane(id));
}

function dropPane(id: string) {
  gone.add(id);
  if (!sessions.has(id)) return;
  // The keys go to a neighbor, picked while the pane and its row are still there:
  // the pane beside it in a group, else the next session row in the list.
  // A retiring project Skipper gives them to the fleet Skipper that took it over.
  const heir = heirOf(id, S.focused === id);
  const near = S.focused !== id || heir ? null : shownIds().length > 1 ? paneNear(id) : S.single === id ? rowNear(id) : null;
  sessions.delete(id);
  panes.get(id)?.term.dispose();
  panes.get(id)?.el.remove();
  panes.delete(id);
  S.selection = S.selection.filter((x) => x !== id);
  if (S.focused === id) S.focused = null;
  for (const g of [...S.groups]) {
    if (!sessionsOf(g.layout).includes(id)) continue;
    let next = removePane(g.layout, id);
    if (filledOf(next).length === 0) {
      // The last session ended: the group has nothing left to show, so it goes.
      deleteGroup(g);
      continue;
    }
    if (next) {
      g.layout = next;
      syncTemplateName(g);
    }
    if (g.focus === id) g.focus = filledOf(next)[0] ?? null;
  }
  if (S.single === id) S.single = null;
  const ids = shownIds();
  if (S.focused && !ids.includes(S.focused)) S.focused = null;
  if (heir && sessions.has(heir)) return revealSession(heir);
  if (near && ids.includes(near)) return focusPane(near);
  if (!ids.length) return near && sessions.has(near) ? revealSession(near) : unfocus();
  if (!S.focused) {
    const next = activeGroupObj()?.focus ?? ids.find((x) => !isSlot(x));
    if (next) return focusPane(next);
  }
  render();
}

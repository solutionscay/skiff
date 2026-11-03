import { ctxMenu } from "../../ui/contextMenu";
import { isSkiffAgent, skipperWord } from "../../app/skiffAgent";
import { keyLabel } from "../../app/keys";
import { t, tn } from "../../i18n";
import { type MenuEntry } from "../../ui/menu";
import { isRemote } from "../../app/hosts";
import { isProjectSkipper, scopeName } from "../model";
import { setAvatarDialog } from "../skipperFace";
import type { SessionInfo } from "../../platform/types";
import { startSessionRename } from "../rename";
import { clearSelection, selectionPlan, splitSelection } from "../selection";
import { MAX_PANES, S, sessions } from "../../app/state";
import { groupOf } from "../../app/stateQueries";
import { removeFromGroup } from "../view";
import { hostCan } from "../../app/hostCaps";
import { endEntry, endSelectedEntry, sessionLabel, themeEntry } from "./end";
import { groupCloseEntries } from "./group";
import { skiffAgentMenu } from "./pane";

export function rowMenu(s: SessionInfo, x: number, y: number) {
  if (isSkiffAgent(s.id)) return skiffAgentMenu(s, x, y);
  const ids = S.selection.filter((id) => sessions.has(id));
  if (ids.length > 1 && ids.includes(s.id)) return selectionMenu(ids, x, y);
  ctxMenu.open(x, y, sessionLabel(s), [
    ...(groupOf(s.id)
      ? [{ icon: "code-ungroup", label: t("cmd.removeFromGroup"), hint: keyLabel("close-pane"), disabled: !hostCan("session_groups"), run: () => removeFromGroup(s.id) }]
      : []),
    // A project Skipper's name is derived from the Skiff agent's name and its project: nothing to type.
    ...(isProjectSkipper(s) ? [] : [{ icon: "tools-pencil", label: t("menu.rename"), hint: keyLabel("rename"), disabled: !hostCan("session_lifecycle"), run: () => startSessionRename(s.id) }]),
    themeEntry(s),
    ...(isProjectSkipper(s) && s.skipper ? [avatarEntry(s.skipper, t("skipper.avatarTitle", { name: t(s.skipper.startsWith("fleet:") ? "skipper.avatarFleet" : "skipper.avatarProject", { scope: scopeName(s.skipper), skipper: skipperWord() }) }))] : []),
    endEntry(s),
  ]);
}

/**
 * The menu of a row in a multi-selection acts on every selected session. Rename and
 * the theme act on one session, so they are left out. No hints: the keys act on one row.
 */
function selectionMenu(ids: string[], x: number, y: number) {
  const plan = selectionPlan();
  const grouped = ids.filter((id) => groupOf(id));
  ctxMenu.open(x, y, tn("menu.selected", ids.length), [
    ...(plan
      ? [{
          icon: "code-group",
          label: plan.kind === "new" ? tn("menu.groupSelected", ids.length) : plan.label,
          disabled: plan.size > MAX_PANES,
          run: splitSelection,
        }]
      : []),
    ...(grouped.length
      ? [{
          icon: "code-ungroup",
          label: tn("menu.ungroupSelected", grouped.length),
          run: () => {
            clearSelection();
            for (const id of grouped) removeFromGroup(id);
          },
        }]
      : []),
    endSelectedEntry(ids),
  ]);
}

const pick = (m: MenuEntry) => void ("run" in m && m.run?.());

/**
 * Delete or Backspace on a list row. With one way to remove it, that runs
 * (End or removal asks first). With more, a menu at the row asks which.
 */
export function deleteKeyMenu(el: HTMLElement) {
  // A key opened it: show focus rings, so the first item reads as the one picked.
  document.body.classList.add("kbd");
  const r = el.getBoundingClientRect();
  const [x, y] = [r.left + 24, r.bottom];
  const g = S.groups.find((x) => x.id === el.dataset.group);
  if (g && !el.dataset.session) return ctxMenu.open(x, y, t("menu.group", { name: g.name }), groupCloseEntries(g));
  const s = sessions.get(el.dataset.session ?? "");
  if (!s) return;
  const ids = S.selection.filter((id) => sessions.has(id));
  if (ids.length > 1 && ids.includes(s.id)) return pick(endSelectedEntry(ids));
  if (!groupOf(s.id)) return pick(endEntry(s));
  ctxMenu.open(x, y, sessionLabel(s), [
    { icon: "code-ungroup", label: t("cmd.removeFromGroup"), hint: keyLabel("close-pane"), disabled: !hostCan("session_groups"), run: () => removeFromGroup(s.id) },
    endEntry(s),
  ]);
}

/** A Skipper's face: `scope` "fleet:<name>" or a project. Kept in its folder on this Mac. The Admiral wears the mascot's head. */
function avatarEntry(scope: string, title: string): MenuEntry {
  return { icon: "playback-image", label: t("menu.avatar"), disabled: isRemote(), run: () => setAvatarDialog(scope, title, () => {}) };
}

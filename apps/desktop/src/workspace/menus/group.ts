import { ctxMenu } from "../../ui/contextMenu";
import { launchIcon } from "../../appearance/agentIcon";
import { confirmAction } from "../../ui/confirm";
import { skipperEndLines } from "../skipperEnd";
import { keyLabel } from "../../app/keys";
import { t, tn, upper } from "../../i18n";
import { type MenuEntry } from "../../ui/menu";
import { basename, branchName, locate } from "../model";
import { cwdOf } from "../canvas";
import { filledOf, slotsOf } from "../layoutSlots";
import type { Group } from "../../platform/types";
import { newSession } from "../../app/daemon";
import { showError } from "../../ui/alerts";
import { deleteGroup } from "../../app/groups";
import { sessionsOf } from "../layout";
import { startRename } from "../rename";
import { render } from "../../app/render";
import { clearSelection } from "../selection";
import { MAX_PANES, S, sessions } from "../../app/state";
import { launchChoices, launchDigit } from "../../app/stateQueries";
import { groupThemeMenu } from "../../appearance/themes";
import { refocusTerminal, showGroup, unsplit } from "../view";
import { resumeAllEntry } from "../../terminal/resume";
import { hostCan, hostInvoke } from "../../app/hostCaps";

export function groupMenu(g: Group, x: number, y: number) {
  const resume = resumeAllEntry(sessionsOf(g.layout));
  ctxMenu.open(x, y, t("menu.group", { name: g.name }), [
    ...(resume ? [resume] : []),
    ...groupStartEntries(g),
    { head: upper(t("menu.headGroup")) },
    { icon: "tools-pencil", label: t("menu.rename"), hint: keyLabel("rename"), disabled: !hostCan("session_groups"), run: () => startRename(g) },
    { icon: "tools-sparkles", label: t("menu.groupTheme"), hint: keyLabel("theme"), disabled: !hostCan("session_lifecycle"), run: () => void groupThemeMenu(g) },
    ...groupCloseEntries(g),
  ]);
}

/** Shell, then agents: a new session in the group's first empty pane, else a new pane on the right. */
function groupStartEntries(g: Group): MenuEntry[] {
  const cwd = cwdOf(g);
  const at = cwd ? locate(S.projects, cwd) : null;
  const where = at ? branchName(at.worktree) : cwd ? basename(cwd) : t("menu.home");
  const empty = slotsOf(g.layout)[0];
  const filled = filledOf(g.layout).filter((id) => sessions.has(id));
  const full = !empty && filled.length >= MAX_PANES;
  const entries: MenuEntry[] = [{ head: upper(t("menu.newIn", { where })) }];
  launchChoices().forEach((a, i) => {
    entries.push({
      glyph: launchIcon(a),
      label: a?.id ?? t("menu.shell"),
      hint: launchDigit(i),
      disabled: full || !hostCan("session_lifecycle"),
      run: () => {
        if (g.id !== S.activeGroup) showGroup(g.id);
        const target = g.focus && filled.includes(g.focus) ? g.focus : filled[0];
        const place = empty ? { slot: empty } : { target, dir: "row" as const };
        void newSession(cwd, a?.command ?? null, a?.id ?? "", a ? "agent" : "shell", place).catch(showError);
      },
    });
  });
  return entries;
}

/** Drops the group. Its sessions keep running and go back under their worktrees. */
export function ungroup(g: Group) {
  if (g.id === S.activeGroup) unsplit(g);
  else {
    deleteGroup(g);
    render();
  }
}

/** Kill all sessions in the group. It asks first when one runs. The group goes with its last session. */
export async function killGroup(id: string) {
  // Group reloads replace objects while a menu or confirmation is open.
  const group = S.groups.find((x) => x.id === id);
  if (!group) return;
  const ids = sessionsOf(group.layout).filter((s) => sessions.has(s));
  const running = ids.filter((s) => sessions.get(s)?.state !== "done");
  if (running.length) {
    const ok = await confirmAction({
      // Not the group's name: an unnamed group is called "1 over 2", which reads as noise here.
      title: t("killGroup.title"),
      body: tn("killGroup.body", running.length),
      action: t("killGroup.action"),
      content: skipperEndLines(running),
    });
    if (!ok) return refocusTerminal();
  }
  clearSelection();
  const results = await Promise.allSettled(ids.filter((id) => sessions.has(id)).map((id) => hostInvoke("session_lifecycle", "kill_session", { session: id })));
  for (const result of results) {
    if (result.status === "rejected") {
      showError(result.reason);
    }
  }
  render();
}

/** The same group actions serve context menus, the menu bar, and the palette. */
export function groupCloseEntries(g: Group | null): Exclude<MenuEntry, { head: string } | { sep: true }>[] {
  const current = () => S.groups.find((x) => x.id === g?.id);
  const idsOf = (group: Group) => sessionsOf(group.layout).filter((id) => sessions.has(id));
  const hasSessions = !!g && idsOf(g).length > 0;
  const drop = () => {
    // Group reloads replace objects while a menu or confirmation is open.
    const group = current();
    if (group) ungroup(group);
  };
  const entries: Exclude<MenuEntry, { head: string } | { sep: true }>[] = [
    { icon: "code-ungroup", label: t("menu.ungroup"), hint: keyLabel("close-pane"), disabled: !g || !hostCan("session_groups"), run: drop },
  ];
  if (hasSessions) entries.push(
    { icon: "indicators-square-stop", label: t("menu.killGroup"), hint: keyLabel("end-session"), danger: true, disabled: !hostCan("session_lifecycle"), run: () => void killGroup(g!.id) },
  );
  return entries;
}

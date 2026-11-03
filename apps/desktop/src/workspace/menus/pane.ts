import { ctxMenu } from "../../ui/contextMenu";
import { imageCount, openImages } from "../../terminal/images";
import { isSkiffAgent, openWatchLog, skiffAgentFolder, skiffAgentName, skiffWatch, startSkiffWatch } from "../../app/skiffAgent";
import { launchIcon } from "../../appearance/agentIcon";
import { keyLabel } from "../../app/keys";
import { t, upper } from "../../i18n";
import { toggleFocusMode, toggleMaximize } from "../../app/modes";
import { type MenuEntry } from "../../ui/menu";
import { isRemote } from "../../app/hosts";
import { basename, branchName, isProjectSkipper } from "../model";
import type { SessionInfo, SplitDir } from "../../platform/types";
import { openFolder } from "../../platform/fileActions";
import { revealEntry } from "../fileMenu";
import { newSession } from "../../app/daemon";
import { showError } from "../../ui/alerts";
import { startSessionRename } from "../rename";
import { S, sessions } from "../../app/state";
import { launchChoices, launchDigit, place, shownIds, splitFull } from "../../app/stateQueries";
import { focusPane, removeFromGroup } from "../view";
import { resumeOffer, resumePane } from "../../terminal/resume";
import { ledgerFolded, ledgerShown, toggleLedger } from "../ledger";
import { hostCan } from "../../app/hostCaps";
import { endEntry, sessionLabel, themeEntry } from "./end";

export function paneMenu(id: string, x: number, y: number) {
  const s = sessions.get(id);
  if (!s) return;
  if (id !== S.focused) focusPane(id);
  if (isSkiffAgent(id)) return skiffAgentMenu(s, x, y);
  const full = splitFull();
  const entries: MenuEntry[] = [
    { icon: "layouts-panel-left:flip", label: t("cmd.splitRight"), hint: keyLabel("split-right"), disabled: full || !hostCan("session_lifecycle"), run: () => splitMenu(id, "row", x, y) },
    { icon: "layouts-panel-bottom", label: t("cmd.splitDown"), hint: keyLabel("split-down"), disabled: full || !hostCan("session_lifecycle"), run: () => splitMenu(id, "col", x, y) },
    ...(isProjectSkipper(s) ? [] : [{ icon: "tools-pencil", label: t("menu.rename"), hint: keyLabel("rename"), disabled: !hostCan("session_lifecycle"), run: () => startSessionRename(id) }]),
  ];
  const offer = resumeOffer(s);
  if (offer) entries.unshift({ icon: "schedule-refresh-cw", label: t("menu.resume", { agent: offer.agent }), run: () => resumePane(id, { x, y }) });
  // These act on the pane you right-clicked, which the line above just focused.
  if (shownIds().length > 1) {
    entries.push({ icon: S.maximized ? "view-restore" : "view-maximize", label: t(S.maximized ? "cmd.restore" : "cmd.maximize"), hint: keyLabel("maximize"), run: toggleMaximize });
  }
  entries.push({ icon: "view-focus", label: t(S.focusMode ? "cmd.leaveFocusMode" : "cmd.focusMode"), hint: keyLabel("focus-mode"), run: toggleFocusMode });
  entries.push(themeEntry(s));
  entries.push({ icon: "playback-image", label: t("cmd.images"), hint: keyLabel("images"), disabled: !imageCount(), run: openImages });
  if (shownIds().length > 1) {
    entries.push({ icon: "code-ungroup", label: t("cmd.removeFromGroup"), hint: keyLabel("close-pane"), disabled: !hostCan("session_groups"), run: () => removeFromGroup(id) });
  }
  entries.push(endEntry(s));
  ctxMenu.open(x, y, sessionLabel(s), entries);
}

/**
 * The Skiff agent's row and pane menu: no splits, groups or rename. Its name is in Settings.
 * With no session, the row shows only its folder entries.
 */
export function skiffAgentMenu(s: SessionInfo | null, x: number, y: number) {
  const folder = skiffAgentFolder();
  // A remote Host's folder stays there. Its log is served as a Host preview.
  const local = !isRemote();
  const entries: MenuEntry[] = folder && local
    ? [revealEntry(() => openFolder(folder))]
    : [];
  if (s) {
    // Only while the row's icon is red: no watcher runs, or its last line failed.
    const watch = skiffWatch(s).state;
    // A remote Host's skiff-host starts it there.
    if (watch === "missing" && (local || hostCan("skipper_run"))) entries.push({ icon: "schedule-refresh-cw", label: t("menu.startWatcher"), run: startSkiffWatch });
    if ((watch === "missing" || watch === "failed") && hostCan("skipper_watch_log")) entries.push({ icon: "indicators-square-arrow-out-up-right", label: t("menu.openWatchLog"), run: openWatchLog });
    entries.push(
      { icon: "view-focus", label: t(S.focusMode ? "cmd.leaveFocusMode" : "cmd.focusMode"), hint: keyLabel("focus-mode"), run: toggleFocusMode },
    );
    if (ledgerShown()) entries.push({ icon: "layouts-panel-left:flip", label: t(ledgerFolded() ? "cmd.showLedger" : "cmd.hideLedger"), hint: keyLabel("ledger"), run: toggleLedger });
    entries.push(
      themeEntry(s),
      endEntry(s),
    );
  }
  if (entries.length) ctxMenu.open(x, y, s ? sessionLabel(s) : skiffAgentName(), entries);
}

/** Second step of Split: a new shell or agent in this pane's worktree. */
export function splitMenu(id: string, dir: SplitDir, x: number, y: number) {
  const s = sessions.get(id);
  if (!s) return;
  const at = place(s);
  const cwd = at ? at.worktree.path : s.cwd;
  const where = at ? branchName(at.worktree) : basename(s.cwd);
  const entries: MenuEntry[] = [{ head: upper(t("menu.newIn", { where })) }];
  launchChoices().forEach((a, i) => {
    entries.push({
      glyph: launchIcon(a),
      label: a?.id ?? t("menu.shell"),
      hint: launchDigit(i),
      run: () => void newSession(cwd, a?.command ?? null, a?.id ?? "", a ? "agent" : "shell", { target: id, dir }).catch(showError),
    });
  });
  ctxMenu.open(x, y, t(dir === "row" ? "menu.addPaneRightTitle" : "menu.addPaneBelowTitle"), entries);
}

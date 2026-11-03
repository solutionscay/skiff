import { confirmAction } from "../../ui/confirm";
import { keyLabel } from "../../app/keys";
import { tn, t } from "../../i18n";
import { agentName, taskTitle } from "../model";
import type { SessionInfo } from "../../platform/types";
import type { MenuEntry } from "../../ui/menu";
import { showError } from "../../ui/alerts";
import { sessions } from "../../app/state";
import { clearSelection } from "../selection";
import { render } from "../../app/render";
import { sessionThemeMenu } from "../../appearance/themes";
import { refocusTerminal } from "../view";
import { hostCan, hostInvoke } from "../../app/hostCaps";

export function sessionLabel(s: SessionInfo) {
  return `${agentName(s)} · ${taskTitle(s)}`;
}

export function themeEntry(s: SessionInfo): MenuEntry {
  return { icon: "tools-sparkles", label: t("cmd.sessionTheme"), hint: keyLabel("theme"), disabled: !hostCan("session_lifecycle"), run: () => void sessionThemeMenu(s) };
}

/** Right-click, End session: stops the process and drops it from the list. */
export function endEntry(s: SessionInfo): MenuEntry {
  if (s.state === "done") {
    return {
      icon: "tools-trash-2",
      label: t("menu.removeFromList"),
      hint: keyLabel("end-session"),
      danger: true,
      disabled: !hostCan("session_lifecycle"),
      run: async () => {
        const ok = await confirmAction({
          title: t("removeSession.title", { name: taskTitle(s) }),
          body: t("removeSession.body"),
          action: t("removeSession.action"),
        });
        if (ok) void endSession(s.id);
        else refocusTerminal();
      },
    };
  }
  return {
    icon: "indicators-square-stop",
    label: t("cmd.endSession"),
    hint: keyLabel("end-session"),
    danger: true,
    disabled: !hostCan("session_lifecycle"),
    // A running agent loses its work: ask once more.
    run: async () => {
      const ok = await confirmAction({
        title: t("endSession.title", { name: taskTitle(s) }),
        body: t("endSession.body", { agent: agentName(s) }),
        action: t("endSession.action"),
      });
      if (ok) void endSession(s.id);
      else refocusTerminal();
    },
  };
}

/** Right-click on a multi-selection: ends every selected session, with one confirm. */
export function endSelectedEntry(ids: string[]): MenuEntry {
  return endManyEntry(ids, tn("menu.endSelected", ids.length), tn("endSessions.title", ids.length));
}

/** Ends several sessions after one confirm. Sessions that already exited go without asking. */
function endManyEntry(ids: string[], label: string, title: string): MenuEntry {
  const running = ids.filter((id) => sessions.get(id)?.state !== "done");
  const end = () => {
    clearSelection();
    render();
    for (const id of ids) void endSession(id);
  };
  if (!running.length) {
    return { icon: "tools-trash-2", label: tn("menu.removeSelected", ids.length), danger: true, disabled: !hostCan("session_lifecycle"), run: end };
  }
  return {
    icon: "indicators-square-stop",
    label,
    danger: true,
    disabled: !hostCan("session_lifecycle"),
    run: async () => {
      const ok = await confirmAction({
        title,
        body: tn("endSessions.body", running.length),
        action: t("endSessions.action"),
      });
      if (ok) end();
      else refocusTerminal();
    },
  };
}

export async function endSession(id: string) {
  try {
    await hostInvoke("session_lifecycle", "kill_session", { session: id });
  } catch (e) {
    showError(e);
  }
}

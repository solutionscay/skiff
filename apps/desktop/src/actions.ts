import { type Action } from "./keys";

import { invoke } from "@tauri-apps/api/core";

import { $ } from "./dom";
import { showError } from "./alerts";
import { currentRow, cycleRegion, fromProject, stepList, stepRail, toProject } from "./keyboard";

import { newWorktree, projectMenu, projectRun, splitMenu } from "./menus";
import { ctxMenu } from "./contextMenu";
import { addProject, launchMenu, settings } from "./panels";

import { renameListItem, startRename, startSessionRename } from "./rename";

import { FONT_DEFAULT, S } from "./state";
import { currentProject, currentWorktree, shownIds, splitFull } from "./stateQueries";

import { paneCenter } from "./terminal";

import { openFind } from "./terminalSearch";
import { copySelection, pasteClipboard } from "./terminalClipboard";
import { setFontSize } from "./terminalFont";

import { modeGate, modeKeyBlocked, toggleFocusMode, toggleMaximize } from "./modes";
import { closePane, focusNextWaiting, goBack, moveFocus } from "./view";
import { switcher } from "./commandUi";
import { showShortcuts } from "./infoDialogs";
function openNewSession() {
  // Its key works from inside a menu, such as the worktree menu that shows it: that menu goes.
  ctxMenu.close(false);
  // The highlighted row's worktree, as its + would; else the focused one.
  const row = S.atRail ? undefined : currentRow();
  const rowPlus = row?.closest("#sidebar-scroll .wt")?.querySelector<HTMLElement>(".wt-plus");
  if (rowPlus) return rowPlus.click();
  const at = currentWorktree();
  if (!at) return;
  const plus = [...document.querySelectorAll<HTMLElement>("#sidebar-scroll .wt-plus")].find((b) => b.dataset.wt === at.w.path);
  launchMenu.open(plus ?? $("rail"), at.p, at.w);
}

export function runAction(a: Action) {
  const shown = S.focused && shownIds().includes(S.focused);
  // The native menu runs its accelerators through here, not through the key handler.
  if (modeKeyBlocked(a) || modeGate(a)) return;
  // The open palette holds every other action; its own key closes it.
  if (switcher.isOpen() && a !== "palette") return;
  switch (a) {
    case "palette": return switcher.toggle();
    case "new-session": return openNewSession();
    case "new-worktree": {
      const p = currentProject();
      return p ? newWorktree(p) : undefined;
    }
    case "add-project": return void addProject.open();
    case "project-menu": {
      // The highlighted row's menu, as a right-click opens it: a worktree, a
      // group, a session, or a Changes or Files row. The
      // highlight, not DOM focus: a picked group can be current while focus
      // rests elsewhere. On the rail, or with no row, the project menu.
      const row = S.atRail || document.activeElement?.closest("#rail") ? undefined : currentRow();
      if (!row) return openProjectMenu();
      const b = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: b.left + 24, clientY: b.bottom }));
      return;
    }
    case "project-folder":
    case "project-copy-path":
    case "project-color":
    case "project-theme":
    case "project-changes":
    case "project-files": {
      const p = currentProject();
      return p ? projectRun(p)[a]() : undefined;
    }
    case "split-right":
    case "split-down": {
      if (!S.focused || !shown || splitFull()) return;
      const [x, y] = paneCenter();
      return splitMenu(S.focused, a === "split-right" ? "row" : "col", x, y);
    }
    case "close-pane": return shown ? closePane(S.focused!) : undefined;
    case "next-waiting": return focusNextWaiting();
    case "back": return goBack();
    case "rename": {
      // In the list, the row with the keys: arrowing to a row does not open it at once.
      if (renameListItem(document.activeElement as HTMLElement | null)) return;
      const pg = S.groups.find((x) => x.id === S.groupPicked);
      return pg ? startRename(pg) : S.focused ? startSessionRename(S.focused) : undefined;
    }
    case "copy": return void copySelection();
    case "paste": return void pasteClipboard();
    case "find": return openFind();
    case "font-bigger": return setFontSize(S.fontSize + 1);
    case "font-smaller": return setFontSize(S.fontSize - 1);
    case "font-reset": return setFontSize(FONT_DEFAULT);
    case "settings": return void settings.open();
    case "quit": return void invoke("quit_app").catch(showError);
    case "focus-left": return moveFocus("left");
    case "focus-right": return moveFocus("right");
    case "focus-up": return moveFocus("up");
    case "focus-down": return moveFocus("down");
    case "region-next": return cycleRegion(1);
    case "region-prev": return cycleRegion(-1);
    case "session-next": return S.atRail ? stepRail(1) : stepList(1);
    case "session-prev": return S.atRail ? stepRail(-1) : stepList(-1);
    case "list-project": return toProject();
    case "list-back": return fromProject();
    case "shortcuts": return showShortcuts();
    case "focus-mode": return toggleFocusMode();
    case "maximize": return toggleMaximize();
  }
}

/** The selected project's menu, at its rail icon. */
export function openProjectMenu() {
  const p = currentProject();
  const chip = document.querySelector<HTMLElement>("#rail .rail-chip.active");
  if (!p || !chip) return;
  const r = chip.getBoundingClientRect();
  projectMenu(p, r.right, r.top);
}

/**
 * Workspaces (#154): a `.skiffw` file names some projects and fleets. While
 * one is open, the tree shows only those (a fleet brings the projects in it
 * now), and the Ledger shows only their work and the work of no project. A
 * workspace changes what shows, never a project, a fleet, a session or the
 * ledger. With none open, the tree shows All projects.
 *
 * The file name is the workspace's name. New puts the file in Skiff's own
 * folder. Each change is saved at once: membership, folds and the last pick.
 * There is no unsaved state and no prompt. A change to the file from outside
 * is read back on the next disk check; a file that is gone shows a mark, and
 * changes wait until it is saved again.
 */

import { render } from "../app/renderRequest";
import { S } from "../app/state";
import { createMembership } from "./profile/membership";
import { createWorkspaceState, trimPath, type Profile } from "./profile/state";
import { workspaceDialog } from "./profile/dialogs";
import { createTransitions } from "./profile/transitions";
import { createStatus } from "./profile/status";
export type { Profile } from "./profile/state";

const state = createWorkspaceState();
const membership = createMembership(state, { announce, changed, showView: (focus) => transitions.showView(focus) });
export const workspaceOpen = state.workspaceOpen;
export const workspaceName = state.workspaceName;
export const shows = state.shows;
export const ledgerScope = state.ledgerScope;
export const ledgerAllWork = state.ledgerAllWork;
export const setLedgerAllWork = state.setLedgerAllWork;
export const noteSelected = state.noteSelected;
export const { includeAdded, includeNew, keepInView, renameWorkspaceFleet, projectsPlus, projectWorkspaceEntries, fleetWorkspaceEntries } = membership;

/** Says a change to screen readers without moving the keys. */
function announce(text: string) {
  const live = document.getElementById("ws-live");
  if (!live) return;
  live.textContent = "";
  requestAnimationFrame(() => (live.textContent = text));
}

/** The profile changed in memory: show it and save it. */
function changed() {
  render();
  void transitions.save();
}

state.setHooks({ announce, changed });

// ── Files ──────────────────────────────────────────────────────────────────

/** Where the keys go when what had them is gone: the sidebar's current row. */
const home = () => document.querySelector<HTMLElement>('#sidebar-scroll [tabindex="0"]');

const transitions = createTransitions(state, { announce, home });
export const { openWorkspace, closeWorkspace } = transitions;

function dialog(isNew: boolean) {
  workspaceDialog(state, isNew, {
    home, changed, showView: () => transitions.showView(), S, trimPath,
    create: async (name: string, profile: Profile) => {
      const why = await transitions.create(name, profile);
      if (!why) void status.refreshOwn();
      return why;
    },
    renameTo: async (name: string) => {
      const why = await transitions.renameTo(name);
      if (!why) void status.refreshOwn();
      return why;
    },
  });
}

export const newWorkspace = () => dialog(true);
export const editWorkspace = () => dialog(false);

// ── Menus and the status bar ───────────────────────────────────────────────

const status = createStatus(state, transitions, { home, newWorkspace, editWorkspace });
export const { recentWorkspaceEntries, workspaceNotice, renderWorkspaceStatus, startWorkspaces } = status;

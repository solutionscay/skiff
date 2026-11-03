/**
 * Workspaces (#154): a `.skiffw` file names some projects and fleets. While
 * one is open, the tree shows only those (a fleet brings the projects in it
 * now), and the Ledger shows only their work and the work of no project. A
 * workspace changes what shows, never a project, a fleet, a session or the
 * ledger. With none open, the tree shows All projects.
 *
 * One document at a time: its path (null: Untitled), the profile in memory,
 * the profile last read or saved (the baseline), and the file's revision on
 * disk. Edited: the profile differs from the baseline. Name, inclusions,
 * repairs, and the user's folds and project picks are edits. Automatic opens
 * and picks are not.
 *
 * Every way out of an edited workspace (New, Open, a recent, Close, a file
 * from the OS, Quit, the window's close) asks Save / Don't Save / Cancel.
 * Saves are atomic and check the revision first. An outside change to the
 * file shows a notice and never reloads by itself.
 */

import { render } from "../app/renderRequest";
import { S } from "../app/state";
import { t } from "../i18n";
import { createMembership } from "./profile/membership";
import { createWorkspaceState, trimPath } from "./profile/state";
import { editWorkspaceDialog } from "./profile/dialogs";
import { createTransitions } from "./profile/transitions";
import { createStatus } from "./profile/status";
export type { Profile } from "./profile/state";

const state = createWorkspaceState();
const membership = createMembership(state, { announce, changed, showView: (focus) => transitions.showView(focus) });
export const workspaceOpen = state.workspaceOpen;
export const workspaceName = state.workspaceName;
export const workspaceEdited = state.workspaceEdited;
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

/** The profile changed in memory. */
function changed() {
  const now = workspaceEdited();
  if (now !== state.wasEdited) announce(t(now ? "workspace.editedSay" : "workspace.cleanSay", { name: workspaceName() }));
  state.wasEdited = now;
  render();
}

state.setHooks({ announce, changed });

// ── Files ──────────────────────────────────────────────────────────────────

const transitions = createTransitions(state, { announce, changed, home: () => home(), editWorkspace });
const { showView } = transitions;
export const { leave, openWorkspace, newWorkspace, closeWorkspace, saveWorkspace, saveWorkspaceAs, quitApp } = transitions;

export function editWorkspace() {
  editWorkspaceDialog(state, { home, changed, showView, S, trimPath });
}

// ── Menus and the status bar ───────────────────────────────────────────────

/** Where the keys go when what had them is gone: the sidebar's current row. */
const home = () => document.querySelector<HTMLElement>('#sidebar-scroll [tabindex="0"]');

const status = createStatus(state, transitions, { home, editWorkspace });
export const { recentWorkspacesMenu, workspaceNotice, reloadWorkspace, keepWorkspace, renderWorkspaceStatus, startWorkspaceQuitGuard, startWorkspaces } = status;

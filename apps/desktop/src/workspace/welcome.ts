import { launchIcon } from "../appearance/agentIcon";

import { branchName, isFolder } from "./model";

import { newSession } from "../app/daemon";

import { button, h } from "../ui/dom";
import { branchIcon } from "../ui/icons";
import { host } from "../terminal/terminalHost";

import { showError } from "../ui/alerts";
import { newWorktree } from "./menus";

import { addProject } from "../app/panels";
import { openClosedProject } from "./projectClose";

import { S } from "../app/state";
import { checkShownHost, hostName, isRemote, updateShownHost } from "../app/hosts";
import { daemonWarning } from "../app/daemonRestart";
import { hostCan } from "../app/hostCaps";
import { openRemoteNewSession } from "./remoteLaunch";
import { accent, currentProject, launchChoices, shownIds } from "../app/stateQueries";

let welcomeSig = "";
let welcomeBox: HTMLElement | null = null;

/** First run: add a project. Just added, or nothing focused: start an agent. */
export function renderWelcome() {
  const p = currentProject();
  const w = p?.worktrees.find((x) => x.is_main) ?? p?.worktrees[0];
  // Same content: keep the element, and the focus and hover on its buttons.
  const gap = daemonWarning();
  const sig = JSON.stringify([gap?.kind, gap?.message, S.projects.length, S.closedProjects.map((x) => x.name), !!S.projectsError, p?.name, p?.color, w?.path, S.justAdded === p?.name, !!S.focused, shownIds().length, launchChoices().map((a) => a?.id), hostCan("session_lifecycle")]);
  if (sig === welcomeSig && (!welcomeBox || welcomeBox.isConnected)) return;
  welcomeSig = sig;
  host.querySelector(".empty")?.remove();
  welcomeBox?.remove();
  welcomeBox = null;
  const box = h("div", "welcome");
  if (isRemote()) {
    // A remote Host: its sessions, and new ones when its Skiff offers them.
    if (S.focused || shownIds().length) return;
    const can = hostCan("session_lifecycle");
    const text = can
      ? `Pick a session in the list, or start one in a folder on ${hostName()}. Its agents are the ones installed there.`
      : `Pick a session in the list to see and type in it.`;
    box.append(h("div", "welcome-title", `Sessions on ${hostName()}`), h("div", "welcome-text", text));
    if (can) {
      const start = button("welcome-primary", "New session…", () => {
        const b = start.getBoundingClientRect();
        openRemoteNewSession(b.left, b.bottom);
      });
      box.append(start);
    } else {
      box.append(...versionGap());
    }
  } else if (S.projects.length === 0 && S.closedProjects.length && !S.projectsError) {
    // Every project is closed: open one again, or add another.
    const actions = h("div", "welcome-actions");
    for (const c of S.closedProjects) {
      const b = button("", c.name, () => void openClosedProject(c));
      b.style.color = accent(c);
      actions.appendChild(b);
    }
    const add = button("", "add project…", () => void addProject.open());
    actions.appendChild(add);
    box.append(
      h("div", "welcome-title", "No open project"),
      h("div", "welcome-text", "Open a closed project again, with its settings, or add another."),
      actions,
    );
  } else if (S.projects.length === 0 && !S.projectsError) {
    const add = button("welcome-primary", "Add project", () => void addProject.open());
    const hint = h("div", "welcome-hint", "or edit ");
    hint.appendChild(h("span", "mono", "~/.config/skiff/projects.toml"));
    box.append(
      h("div", "welcome-title", "Add your first project"),
      h("div", "welcome-text", "Pick a folder. Skiff runs agents in it. In a git repository, it also lists the worktrees."),
      add,
      hint,
    );
  } else if (p && w && !shownIds().length && (S.justAdded === p.name || !S.focused)) {
    const title = h("div", "welcome-title");
    const name = h("span", "", p.name);
    name.style.color = accent(p);
    title.append(name, S.justAdded === p.name ? " is ready" : "");
    const text = h("div", "welcome-text", "Start an agent in ");
    text.append(h("span", "mono", branchName(w)), isFolder(p) ? "." : ", or make a worktree for a task first.");
    const actions = h("div", "welcome-actions");
    for (const a of launchChoices()) {
      const b = button("", "", () => {
        S.justAdded = null;
        newSession(w.path, a?.command ?? null, a?.id ?? "", a ? "agent" : "shell").catch(showError);
      });
      b.append(launchIcon(a), a ? a.id : "shell");
      actions.appendChild(b);
    }
    if (!isFolder(p)) {
      const wt = button("", "", () => newWorktree(p));
      wt.append(branchIcon(), "worktree…");
      actions.appendChild(wt);
    }
    box.append(title, text, actions);
  } else if (!S.focused && !shownIds().length) {
    box.append(h("div", "welcome-text", "No session. Press + on a worktree to start one."));
  } else {
    return;
  }
  host.appendChild(box);
  welcomeBox = box;
}

/** Why a remote Host offers no new sessions, and the one step that fixes it
 *  from here. A Skiff that Skiff installed: Update Skiff…, the same update
 *  as in Settings › Hosts. One from its own install, or a newer one: the
 *  message only, as the badge says it. Neither: the check list. */
function versionGap(): HTMLElement[] {
  const w = daemonWarning();
  const name = hostName();
  if (w?.kind === "update") {
    return [
      h("div", "welcome-hint", `${name} runs an older Skiff. Update it to start and end sessions here.`),
      button("welcome-primary", "Update Skiff…", () => updateShownHost()),
    ];
  }
  if (w?.kind === "remote") return [h("div", "welcome-hint", w.message)];
  return [
    h("div", "welcome-hint", `${name} does not offer new sessions. Its Skiff is older than this app, or its skiff-host does not run.`),
    button("welcome-primary", `Check ${name}…`, () => checkShownHost()),
  ];
}

import { launchIcon } from "../appearance/agentIcon";

import { branchName, isFolder } from "./model";

import { newSession } from "../app/daemon";

import { button, h } from "../ui/dom";
import { branchIcon } from "../ui/icons";
import { host } from "../terminal/terminalHost";

import { showError } from "../ui/alerts";
import { newWorktree } from "./menus";

import { addProject } from "../app/panels";
import { openSkiffAgent, SKIFF_AGENT_KEY, skiffAgentChoice, skiffAgentFolder, skiffAgentName } from "../app/skiffAgent";
import { keepRow } from "./selection";
import { openClosedProject } from "./projectClose";

import { S } from "../app/state";
import { accent, currentProject, launchChoices, shownIds } from "../app/stateQueries";

let welcomeSig = "";
let welcomeBox: HTMLElement | null = null;

/** The Skiff agent's start button, while it is on. It runs in its own folder, so it needs no project. */
function skiffAgentButton(): HTMLButtonElement | null {
  if (!skiffAgentFolder()) return null;
  const b = button("", "", () => {
    keepRow(SKIFF_AGENT_KEY);
    openSkiffAgent();
  });
  b.append(launchIcon(skiffAgentChoice()), skiffAgentName());
  return b;
}

/** Add project and the Skiff agent's button side by side. */
function first(add: HTMLElement, agent: HTMLElement): HTMLElement {
  const row = h("div", "welcome-first");
  const actions = h("div", "welcome-actions");
  actions.appendChild(agent);
  row.append(add, actions);
  return row;
}

/** First run: add a project, or start the Skiff agent. Just added, or nothing focused: start an agent. */
export function renderWelcome() {
  const p = currentProject();
  const w = p?.worktrees.find((x) => x.is_main) ?? p?.worktrees[0];
  // A pane on screen with no project is the Skiff agent's: the welcome does not cover it.
  const none = S.projects.length === 0 && !S.projectsError && !shownIds().length;
  const agent = skiffAgentFolder() ? [skiffAgentName(), skiffAgentChoice()?.id ?? null] : null;
  // Same content: keep the element, and the focus and hover on its buttons.
  const sig = JSON.stringify([agent, S.projects.length, S.closedProjects.map((x) => x.name), !!S.projectsError, p?.name, p?.color, w?.path, S.justAdded === p?.name, !!S.focused, shownIds().length, launchChoices().map((a) => a?.id)]);
  if (sig === welcomeSig && (!welcomeBox || welcomeBox.isConnected)) return;
  welcomeSig = sig;
  host.querySelector(".empty")?.remove();
  welcomeBox?.remove();
  welcomeBox = null;
  const box = h("div", "welcome");
  if (none && S.closedProjects.length) {
    // Every project is closed: open one again, or add another.
    const actions = h("div", "welcome-actions");
    for (const c of S.closedProjects) {
      const b = button("", c.name, () => void openClosedProject(c));
      b.style.color = accent(c);
      actions.appendChild(b);
    }
    const add = button("", "add project…", () => void addProject.open());
    actions.appendChild(add);
    const sa = skiffAgentButton();
    if (sa) actions.appendChild(sa);
    box.append(
      h("div", "welcome-title", "No open project"),
      h("div", "welcome-text", "Open a closed project again, with its settings, or add another."),
      actions,
    );
  } else if (none) {
    const add = button("welcome-primary", "Add project", () => void addProject.open());
    const hint = h("div", "welcome-hint", "or edit ");
    hint.appendChild(h("span", "mono", "~/.config/skiff/projects.toml"));
    const sa = skiffAgentButton();
    box.append(
      h("div", "welcome-title", "Add your first project"),
      h("div", "welcome-text", "Pick a folder. Skiff runs agents in it. In a git repository, it also lists the worktrees."),
      sa ? first(add, sa) : add,
      hint,
    );
  } else if (S.projects.length === 0 && !S.projectsError) {
    // The Skiff agent's pane shows.
    return;
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

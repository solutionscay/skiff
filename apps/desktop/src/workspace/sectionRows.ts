/** Section heads, the fleet shown block, Not-in-workspace and Other. */
import type { SessionInfo } from "../platform/types";
import { button, h } from "../ui/dom";
import { chevron, icon, plusIcon } from "../ui/icons";
import { fleetMenu } from "./menus";
import { DEFAULT_ACCENT, S } from "../app/state";
import { accent, place } from "../app/stateQueries";
import { addProject } from "../app/panels";
import { dragTree } from "./treeDrag";
import { fleetKey, isOpen, rollup, setProjectsHidden, toggleRow, type Section } from "./tree";
import { render } from "../app/renderRequest";
import { projectsPlus, workspaceOpen } from "./profile";
import { hostConnected, hostName, isRemote } from "../app/hosts";
import { sentences, t, tn } from "../i18n";
import { placedRows } from "./sessionRows";
import { errorRow } from "./projectRows";

/**
 * What the list says about the projects themselves, from the selected
 * Host's data plane: they load, they are out of reach and why, or there are
 * none. Null when the projects show and say it all.
 */
export function projectsNote(): HTMLElement | null {
  const remote = isRemote();
  const host = remote ? hostName() : "";
  // A remote Host says what it waits for: the connect, then its projects.
  if (S.dataState === "loading" || (remote && !hostConnected())) {
    if (S.projects.length) return null;
    return h("div", "note-row", remote ? t("sidebar.waitingFor", { host }) : t("sidebar.loadingProjects"));
  }
  if (S.dataState === "unavailable") {
    // Each cause says what is wrong and what Skiff does about it.
    const why = S.dataNote ?? (remote ? t("sidebar.cannotShowOn", { host }) : t("sidebar.cannotShow"));
    const text = S.projects.length ? sentences([why, t("sidebar.lastRead")]) : why;
    // A remote Host without its service is a setup, not a fault: no red.
    return remote ? h("div", "note-row", text) : errorRow(text);
  }
  return S.projects.length ? null : h("div", "note-row", remote ? t("sidebar.noProjectsOn", { host }) : t("sidebar.noProjects"));
}

/** The head over sessions in projects the open workspace leaves out. */
export function outsideHead(): HTMLElement {
  const head = h("div", "tree-head");
  head.append(h("span", "tree-head-name", t("sidebar.outside")));
  return head;
}

/** One row for each session the open workspace would hide. Each names its project or fleet. */
export function outsideBlock(outside: { session: SessionInfo; where: string }[], block: HTMLElement = h("div", "sessions-block")): HTMLElement {
  block.style.height = `calc(${Math.max(40, outside.length * 20)} * var(--u))`;
  return placedRows(block, outside.map(({ session, where }) => {
    const p = S.projects.find((x) => x.name === where);
    return [session, p ? accent(p) : DEFAULT_ACCENT, where];
  }));
}

/** The head over sessions whose folder is in no project. */
export function otherHead(): HTMLElement {
  const head = h("div", "tree-head");
  head.append(h("span", "tree-head-name", t("sidebar.other")));
  return head;
}

/**
 * The sessions still under a fleet that hides its projects: each one that
 * waits, and the one on screen. Each row names its project.
 */
export function shownBlock(shown: SessionInfo[], block: HTMLElement = h("div", "sessions-block fleet-shown")): HTMLElement {
  block.style.height = `calc(${shown.length * 20} * var(--u))`;
  return placedRows(block, shown.map((s) => {
    const p = place(s)!.project;
    return [s, accent(p), p.name];
  }));
}

/**
 * A section head: "Projects" with its +, or a fleet that folds and opens.
 * A fleet that hides its projects counts its hidden sessions (`hidden`); a
 * click on the count shows the projects. `alone`: nothing under the head.
 */
export function sectionHead(sec: Section, hidden: number | null = null, alone = false): HTMLElement {
  if (!sec.fleet) {
    const head = h("div", "tree-head");
    head.append(h("span", "tree-head-name", t("sidebar.projects")));
    // In a workspace, the + puts projects and fleets in it, or adds a project to Skiff.
    const add = button("tree-head-add", "", () => projectsPlus(add, () => void addProject.open()));
    add.title = workspaceOpen() ? t("sidebar.addToWorkspace") : t("sidebar.addProject");
    add.setAttribute("aria-label", add.title);
    if (workspaceOpen()) add.setAttribute("aria-haspopup", "menu");
    add.appendChild(plusIcon());
    head.appendChild(add);
    return head;
  }
  const name = sec.fleet;
  const key = fleetKey(name);
  const open = !alone && isOpen(key);
  const head = h("div", "tree-head fleet-head" + (alone ? " alone" : ""));
  const pick = button("fleet-pick", "", () => {
    if (!alone) toggleRow(key, render);
  });
  pick.dataset.key = key;
  if (!alone) {
    pick.dataset.fold = key;
    pick.setAttribute("aria-expanded", String(open));
  }
  // The chevron sits at the row's end: the indent stays for the hierarchy.
  pick.append(h("span", "tree-head-name", name));
  if (!alone) pick.append(chevron(open));
  const add = button("tree-head-add", "", () => void addProject.open(name));
  add.title = t("sidebar.addProjectTo", { name });
  add.setAttribute("aria-label", add.title);
  add.appendChild(plusIcon());
  // The ⋯ opens the fleet's menu, as on a project row: the same items as a right-click.
  const more = button("tree-head-add tree-head-more", "", () => {
    const r = more.getBoundingClientRect();
    fleetMenu({ name, projects: sec.projects }, r.right, r.top);
  });
  more.setAttribute("aria-label", t("sidebar.menu", { name }));
  more.setAttribute("aria-haspopup", "menu");
  more.appendChild(icon('<circle cx="5" cy="12" r="1"></circle><circle cx="12" cy="12" r="1"></circle><circle cx="19" cy="12" r="1"></circle>'));
  head.append(pick);
  if (hidden !== null) {
    const count = button("fleet-hidden", `· ${hidden}`, () => {
      setProjectsHidden(name, false);
      render();
    });
    count.setAttribute("aria-label", tn("sidebar.showHidden", hidden));
    head.append(count);
  }
  head.append(more, add);
  const roll = open || alone ? null : rollup(sec.projects, name);
  if (roll) pick.insertBefore(roll, pick.querySelector(".chev"));
  head.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    fleetMenu({ name, projects: sec.projects }, e.clientX, e.clientY);
  });
  head.addEventListener("mousedown", (e) => dragTree(e, head, sec.projects, name));
  return head;
}

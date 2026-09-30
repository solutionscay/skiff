import { invoke } from "@tauri-apps/api/core";

import { isUnread } from "./model";
import type { Project } from "./types";

import { $, button, h } from "./dom";
import { icon } from "./icons";

import { projectIcon } from "./projectIcon";
import { showError } from "./alerts";
import { projectMenu } from "./menus";

import { addProject } from "./panels";

import { render } from "./renderRequest";

import { FLAG } from "./stateIcon";
import { OTHER, S, sessions } from "./state";
import { accent, place } from "./stateQueries";

import { selectProject } from "./view";
import { worktreeLines } from "./sidebarLines";
export function renderRail() {
  const rail = $<HTMLElement>("rail");
  const hasOther = worktreeLines(null).length > 0;
  // The last outside session ended: Other goes away, and the first project shows.
  if (!hasOther && S.selectedProject === OTHER) S.selectedProject = S.projects[0]?.name ?? null;
  const waitingIn = (p: Project) => [...sessions.values()].filter((s) => s.state === "waiting" && place(s)?.project === p).length;
  // Renders run while agents print. A rebuilt chip loses its hover and hides
  // its icon until trimmed again, so the rail changes only when what it shows does.
  const unreadIn = (p: Project) => [...sessions.values()].filter((s) => isUnread(s) && place(s)?.project === p).length;
  const sig = JSON.stringify([S.selectedProject, hasOther, S.projects.map((p) => [p.name, p.short, p.icon, accent(p), waitingIn(p), unreadIn(p)])]);
  if (rail.dataset.sig === sig) return;
  rail.dataset.sig = sig;
  rail.replaceChildren();
  S.projects.forEach((p, i) => {
    const waiting = waitingIn(p);
    const unread = unreadIn(p);
    const item = h("div", "rail-item");
    const b = button("rail-chip" + (p.name === S.selectedProject ? " active" : ""), p.icon ? "" : p.short, () => selectProject(p.name));
    if (p.icon) {
      b.classList.add("has-icon");
      b.appendChild(projectIcon(p, 24));
    }
    b.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      projectMenu(p, e.clientX, e.clientY);
    });
    b.addEventListener("mousedown", (e) => dragProject(e, item, i));
    b.dataset.project = p.name;
    b.style.setProperty("--pc", accent(p));
    b.title = i < 9 ? `${p.name} (${navigator.userAgent.includes("Macintosh") ? "⌘" : "Ctrl"}+Shift+${i + 1})` : p.name;
    b.setAttribute("aria-label", p.name + (waiting ? `, ${waiting} waiting` : unread ? `, ${unread} unread` : ""));
    if (p.name === S.selectedProject) b.setAttribute("aria-current", "true");
    item.appendChild(b);
    if (waiting) {
      const bell = h("span", "rail-waiting");
      bell.appendChild(icon('<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"></path><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"></path>'));
      item.appendChild(bell);
    } else if (unread) {
      // The bell wins: a question outranks a result.
      const flag = h("span", "rail-unread");
      flag.appendChild(icon(FLAG));
      item.appendChild(flag);
    }
    rail.appendChild(item);
  });
  if (hasOther) {
    const item = h("div", "rail-item");
    const on = S.selectedProject === OTHER;
    const b = button("rail-chip rail-other" + (on ? " active" : ""), "…", () => {
      S.selectedProject = OTHER;
      render();
    });
    b.title = "Other: sessions outside every project";
    b.setAttribute("aria-label", "Other sessions");
    if (on) b.setAttribute("aria-current", "true");
    item.appendChild(b);
    rail.appendChild(item);
  }
  const add = button("rail-chip rail-add", "+", () => void addProject.open());
  add.title = "Add project";
  add.setAttribute("aria-label", "Add project");
  rail.appendChild(add);
}

/** Drag a rail chip to reorder projects. A press that does not move stays a click. */
function dragProject(down: MouseEvent, item: HTMLElement, from: number) {
  if (down.button !== 0 || S.projects.length < 2) return;
  const sy = down.clientY;
  const items = [...$<HTMLElement>("rail").querySelectorAll<HTMLElement>(".rail-item")].slice(0, S.projects.length);
  let to = from;
  let active = false;
  const mark = () => {
    items.forEach((el, i) => {
      el.classList.toggle("drop-before", active && i === to && to < from);
      el.classList.toggle("drop-after", active && i === to && to > from);
    });
  };
  const move = (m: MouseEvent) => {
    if (!active) {
      if (Math.abs(m.clientY - sy) < 5) return;
      active = true;
      item.classList.add("dragging");
      document.body.classList.add("dragging-rail");
    }
    to = items.findIndex((el) => m.clientY < el.getBoundingClientRect().bottom);
    if (to < 0) to = items.length - 1;
    mark();
  };
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    if (!active) return;
    // The click that follows the release would select the project.
    // A release off the chip fires no click, so the guard must not outlive this event.
    const swallow = (c: Event) => c.stopPropagation();
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true), 0);
    document.body.classList.remove("dragging-rail");
    item.classList.remove("dragging");
    active = false;
    mark();
    if (to === from) return;
    const [p] = S.projects.splice(from, 1);
    S.projects.splice(to, 0, p);
    render();
    invoke("reorder_projects", { order: S.projects.map((x) => x.name) }).catch(showError);
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

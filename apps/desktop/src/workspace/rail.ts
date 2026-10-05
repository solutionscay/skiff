import { invoke } from "@tauri-apps/api/core";

import { isUnread } from "./model";
import type { Project } from "../platform/types";

import { $, button, h } from "../ui/dom";

import { projectIcon } from "../appearance/projectIcon";
import { showError } from "../ui/alerts";
import { projectMenu } from "./menus";

import { addProject } from "../app/panels";

import { render } from "../app/renderRequest";

import { OTHER, S, sessions } from "../app/state";
import { accent, place } from "../app/stateQueries";

import { selectProject } from "./view";
import { worktreeLines } from "./sidebarLines";
/** A circle around the chip's icon. Working: 12 dashes, and a turn of one dash loops without a seam. */
function railRing(state: "working" | "waiting" | "unread"): Element {
  const t = document.createElement("template");
  t.innerHTML = `<svg class="rail-ring ${state}" viewBox="0 0 36 36" aria-hidden="true"><circle cx="18" cy="18" r="16" pathLength="24"></circle></svg>`;
  return t.content.firstChild as Element;
}

export function renderRail() {
  const rail = $<HTMLElement>("rail");
  const hasOther = worktreeLines(null).length > 0;
  // The last outside session ended: Other goes away, and the first project shows.
  if (!hasOther && S.selectedProject === OTHER) S.selectedProject = S.projects[0]?.name ?? null;
  const waitingIn = (p: Project) => [...sessions.values()].filter((s) => s.state === "waiting" && place(s)?.project === p).length;
  // Renders run while agents print. A rebuilt chip loses its hover and hides
  // its icon until trimmed again, so the rail changes only when what it shows does.
  const unreadIn = (p: Project) => [...sessions.values()].filter((s) => isUnread(s) && place(s)?.project === p).length;
  const workingIn = (p: Project) => [...sessions.values()].some((s) => s.state === "working" && place(s)?.project === p);
  const sig = JSON.stringify([S.selectedProject, hasOther, S.projects.map((p) => [p.name, p.short, p.icon, accent(p), waitingIn(p), unreadIn(p), workingIn(p)])]);
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
    // One ring tells the project's state. It turns, dashed and blue, while an agent works.
    // It stops as a solid circle: amber when an agent waits, green when output is unread.
    // Waiting wins, then working, then unread.
    const ring = waiting ? "waiting" : workingIn(p) ? "working" : unread ? "unread" : null;
    if (ring) item.appendChild(railRing(ring));
    if (waiting || unread) b.classList.add("attention");
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

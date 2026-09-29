/** The rail, the session list, the banner, the welcome screen. */
import { invoke } from "@tauri-apps/api/core";
import { filledOf, slotsOf } from "./canvas";
import { agentIcon, launchIcon } from "./agentIcon";
import { glyph, sessionsOf } from "./layout";
import { agentName, branchName, byStart, taskTitle, locate } from "./model";
import type { Group, Project, SessionInfo, Worktree } from "./types";
import { newSession } from "./daemon";
import { $, branchIcon, button, chevron, h, host, icon, plusIcon, projectIcon, showError } from "./dom";
import { groupMenu, newWorktree, projectMenu, removeWorktree, rowMenu } from "./menus";
import { addProject, launchMenu } from "./panels";
import { leaveRename, renameGroup, renameRow, startRename } from "./rename";
import { render } from "./render";
import { clearSelection, selectRange, toggleSelect, toggleSelectGroup } from "./selection";
import { accent, activeGroupObj, collapsed, currentProject, DEFAULT_ACCENT, OTHER, enabledAgents, groupedIds, place, removeErrors, removing, S, selectedWorktree, sessions, worktreeSessions, shownIds } from "./state";
import { groupTheme } from "./themes";
import { dragSessions, revealSession, selectProject, selectWorktree, showGroup } from "./view";

export function renderRail() {
  const rail = $<HTMLElement>("rail");
  rail.replaceChildren();
  S.projects.forEach((p, i) => {
    const waiting = [...sessions.values()].filter((s) => s.state === "waiting" && place(s)?.project === p).length;
    const item = h("div", "rail-item");
    const b = button("rail-chip" + (p.name === S.selectedProject ? " active" : ""), p.icon ? "" : p.short, () => selectProject(p.name));
    if (p.icon) b.appendChild(projectIcon(p, 20));
    b.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      projectMenu(p, e.clientX, e.clientY);
    });
    b.addEventListener("mousedown", (e) => dragProject(e, item, i));
    b.style.setProperty("--pc", accent(p));
    b.title = i < 9 ? `${p.name} (Ctrl ${i + 1})` : p.name;
    b.setAttribute("aria-label", p.name + (waiting ? `, ${waiting} waiting` : ""));
    if (p.name === S.selectedProject) b.setAttribute("aria-current", "true");
    item.appendChild(b);
    if (waiting) {
      const bell = h("span", "rail-waiting");
      bell.appendChild(icon('<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"></path><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"></path>'));
      item.appendChild(bell);
    }
    rail.appendChild(item);
  });
  const hasOther = worktreeLines(null).length > 0;
  // The last outside session ended: Other goes away, and the first project shows.
  if (!hasOther && S.selectedProject === OTHER) S.selectedProject = S.projects[0]?.name ?? null;
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

/** One line in a session list: a session, or a group's header. */
type Line = { session: SessionInfo; branch?: boolean; group?: undefined; in?: Group } | { group: Group };

/** 20px lines with a consistent 20px gap below the last session. */
function sessionBlock(lines: Line[], color: string, head: HTMLElement | null, open: boolean): HTMLElement {
  const block = h("div", "sessions-block");
  const n = (head ? 1 : 0) + (open ? lines.length : 0);
  block.style.height = `calc(${Math.max(40, n * 20 + (open && lines.length ? 20 : 0))} * var(--u))`;
  if (head) block.appendChild(head);
  if (!open) return block;
  for (const l of lines) {
    if (l.group) block.appendChild(groupRow(l.group));
    else block.appendChild(sessionRow(l.session, color, l));
  }
  return block;
}

function sessionRow(s: SessionInfo, color: string, o: { branch?: boolean; in?: Group }): HTMLElement {
  if (S.renamingSession === s.id) return renameRow(s, color);
  // State shows only when it matters: waiting stands out, working pulses, done fades.
  const cls = ["session-row", `st-${s.state}`];
  if (s.id === S.focused && !(o.in && o.in.id === S.groupPicked)) cls.push("focused");
  if (o.in) cls.push("nested");
  if (S.selection.includes(s.id)) cls.push("selected");
  const row = h("button", cls.join(" "));
  row.type = "button";
  row.dataset.session = s.id;
  row.addEventListener("mousedown", (e) => {
    dragSessions(e, S.selection.length > 1 && S.selection.includes(s.id) ? [...S.selection] : [s.id]);
  });
  row.addEventListener("click", (e) => {
    if (e.shiftKey) selectRange(s.id);
    else if (e.ctrlKey || e.metaKey) toggleSelect(s.id);
    else {
      // A plain click opens and ends any selection. It never picks.
      clearSelection();
      revealSession(s.id);
    }
  });
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    rowMenu(s, e.clientX, e.clientY);
  });
  row.style.setProperty("--pc", color);
  if (o.in) row.style.setProperty("--gc", groupColor(o.in));
  row.setAttribute("aria-label", `${agentName(s)}, ${taskTitle(s)}, ${s.state}`);
  row.title = `${agentName(s)}: ${taskTitle(s)}`;
  row.append(agentIcon(s), h("span", "title", taskTitle(s)));
  if (o.branch) {
    const at = place(s);
    row.appendChild(h("span", "where mono", at ? branchName(at.worktree) : "other"));
  }
  row.appendChild(stateIcon(s));
  return row;
}

/**
 * A worktree's lines: its groups (a group lives in the worktree of its first
 * session) with their members, then its sessions in no group.
 */
function worktreeLines(w: Worktree | null): Line[] {
  // A canvas with no session yet lives where it was made.
  const home = (g: Group) => {
    const first = sessions.get(filledOf(g.layout)[0]);
    if (first) return place(first)?.worktree ?? null;
    return g.cwd ? locate(S.projects, g.cwd)?.worktree ?? null : null;
  };
  const lines: Line[] = [];
  for (const g of S.groups) {
    if (home(g) !== w) continue;
    lines.push({ group: g });
    for (const id of sessionsOf(g.layout)) {
      const m = sessions.get(id);
      if (m) lines.push({ session: m, in: g, branch: (place(m)?.worktree ?? null) !== w });
    }
  }
  const grouped = groupedIds();
  const loose = w
    ? worktreeSessions(w)
    : [...sessions.values()].filter((x) => !place(x)).sort(byStart);
  for (const m of loose) if (!grouped.has(m.id)) lines.push({ session: m });
  return lines;
}

function errorRow(text: string): HTMLElement {
  const row = h("div", "error-row", text);
  row.setAttribute("role", "alert");
  return row;
}

export function renderSidebar() {
  const side = $<HTMLElement>("sidebar-scroll");
  // Keep focus and caret position in an inline control across re-renders.
  const active = document.activeElement as HTMLElement | null;
  const activeKey = active?.dataset?.key;
  const caret = active instanceof HTMLInputElement ? active.selectionStart : null;

  side.replaceChildren();
  if (S.selectedProject === OTHER) return renderOther(side, activeKey, caret);
  const p = currentProject();
  const color = accent(p);

  // One 40px row: the name, and a + for the project menu in the column of
  // every worktree's +. The rail shows the icon; the path is in the tooltip.
  const head = h("div", "project-row");
  head.style.setProperty("--pc", color);
  const title = h("div", "project-title");
  if (p) {
    title.append(h("span", "project-name", p.name), h("span", "project-meta", p.error ? "error" : `${p.worktrees.length} ${p.worktrees.length === 1 ? "worktree" : "worktrees"}`));
    title.title = p.path;
  } else {
    title.append(h("span", "project-name dim", S.projects.length ? "No project selected" : "No projects"));
    title.title = S.projectsError ?? "Add one with + in the rail";
  }
  head.appendChild(title);
  if (p) {
    const add = button("wt-plus", "", () => {
      const r = add.getBoundingClientRect();
      projectMenu(p, r.right, r.top);
    });
    add.title = "New worktree, and more";
    add.setAttribute("aria-label", `${p.name} actions`);
    add.setAttribute("aria-haspopup", "menu");
    add.appendChild(plusIcon());
    head.appendChild(add);
  }
  side.appendChild(head);

  if (p) {
    if (p.error) side.appendChild(errorRow(p.error));
    else if (p.worktrees.length === 0) side.appendChild(h("div", "note-row", "No worktrees"));
    const sel = selectedWorktree.get(p.name);
    for (const w of p.worktrees) side.appendChild(worktreeBlock(p, w, w.path === sel, color));
  }

  restoreFocus(side, activeKey, caret);
}

/** The "Other" view: sessions and groups whose folder is in no project. */
function renderOther(side: HTMLElement, activeKey: string | undefined, caret: number | null) {
  const head = h("div", "project-row");
  head.style.setProperty("--pc", DEFAULT_ACCENT);
  const title = h("div", "project-title");
  title.append(h("span", "project-name", "Other"), h("span", "project-meta", "outside every project"));
  head.appendChild(title);
  side.appendChild(head);
  const lines = worktreeLines(null);
  if (lines.length) side.appendChild(sessionBlock(lines, DEFAULT_ACCENT, null, true));
  else side.appendChild(h("div", "note-row", "No sessions"));
  restoreFocus(side, activeKey, caret);
}

function restoreFocus(side: HTMLElement, activeKey: string | undefined, caret: number | null) {
  if (activeKey) {
    const again = side.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-key="${activeKey}"]`);
    if (again && !again.disabled) {
      again.focus();
      if (caret !== null && again instanceof HTMLInputElement) again.setSelectionRange(caret, caret);
    }
  }
}

/** One color per group, by creation order, so a group keeps its color. */
const GROUP_COLORS = ["#9ec1ff", "#f28fd0", "#7ee0cb", "#e0c07e", "#ff9e7a", "#c3e88d"];

const groupColor = (g: Group) => GROUP_COLORS[Math.max(0, S.groups.indexOf(g)) % GROUP_COLORS.length];

function groupRow(g: Group): HTMLElement {
  const ids = sessionsOf(g.layout);
  const row = h("div", "group-row" + (g.id === S.activeGroup ? " active" : ""));
  const theme = groupTheme(g);
  // A themed group takes its color from the theme: the cursor color, else ANSI blue.
  row.style.setProperty("--gc", theme ? theme.cursor ?? theme.palette[4] ?? groupColor(g) : groupColor(g));
  const lead = sessions.get(g.focus ?? ids[0]);
  row.style.setProperty("--pc", accent(lead ? place(lead)?.project : null));
  if (S.renaming?.id === g.id) {
    const r = S.renaming;
    const input = h("input", "form-input");
    input.type = "text";
    input.value = r.name;
    input.spellcheck = false;
    input.dataset.key = "group-rename";
    input.setAttribute("aria-label", "Group name");
    input.addEventListener("input", () => (r.name = input.value));
    input.addEventListener("blur", () => {
      if (S.renaming === r && input.isConnected) void renameGroup(g.id, r.name);
    });
    input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") void renameGroup(g.id, r.name, true);
      else if (e.key === "Escape") {
        S.renaming = null;
        render();
        leaveRename();
      } else return;
      e.preventDefault();
    });
    row.append(glyph(g.layout), input);
    return row;
  }
  const pick = h("button", "group-pick");
  pick.dataset.group = g.id;
  pick.type = "button";
  pick.title = "Double-click to rename";
  pick.append(glyph(g.layout), h("span", "group-name", g.name));
  if (theme) {
    const strip = h("span", "theme-chip");
    strip.title = `Terminal theme: ${theme.name}`;
    strip.style.background = theme.background;
    strip.style.setProperty("--fg", theme.foreground);
    strip.style.setProperty("--cur", theme.cursor ?? theme.palette[4] ?? theme.foreground);
    strip.append(h("span", "chip-text"), h("span", "chip-cursor"));
    pick.append(strip);
  }
  pick.append(h("span", "group-count", String(ids.length)));
  pick.addEventListener("click", (e) => {
    if (e.ctrlKey || e.metaKey) return toggleSelectGroup(g);
    // detail counts clicks across the re-render the first click causes.
    if (e.detail >= 2) startRename(g);
    else if (g.id !== S.activeGroup || g.id !== S.groupPicked) showGroup(g.id, true);
  });
  pick.addEventListener("dblclick", () => startRename(g));
  pick.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    groupMenu(g, e.clientX, e.clientY);
  });
  row.appendChild(pick);
  return row;
}

function worktreeBlock(p: Project, w: Worktree, selected: boolean, color: string): HTMLElement {
  const block = h("div", "wt" + (selected ? " selected" : ""));
  block.style.setProperty("--pc", color);

  const row = h("div", "wt-row");
  const pick = button("wt-pick", "", () => selectWorktree(p, w));
  pick.dataset.wt = w.path;
  pick.append(branchIcon(), h("span", "branch", branchName(w)));
  if (w.is_main) pick.appendChild(h("span", "tag", "primary"));
  if (w.locked) pick.appendChild(h("span", "tag", "locked"));
  if (w.prunable) pick.appendChild(h("span", "tag", "prunable"));
  if (removing.has(w.path)) pick.appendChild(h("span", "tag", "removing…"));
  pick.title = w.path;
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    worktreeMenu(p, w, e.clientX, e.clientY);
  });
  row.appendChild(pick);
  const plus = button("wt-plus", "", () => launchMenu.open(plus, p, w));
  plus.dataset.wt = w.path;
  plus.setAttribute("aria-label", `Start a session in ${branchName(w)}`);
  plus.setAttribute("aria-haspopup", "menu");
  plus.title = "Start a session";
  plus.appendChild(plusIcon());
  row.appendChild(plus);
  block.appendChild(row);

  const list = worktreeSessions(w);
  const open = !collapsed.has(w.path);
  const waiting = list.filter((s) => s.state === "waiting").length;
  const count = button("wt-count", "", () => {
    if (open) collapsed.add(w.path);
    else collapsed.delete(w.path);
    render();
  });
  count.setAttribute("aria-expanded", String(open));
  count.append(h("span", "", `${list.length} ${list.length === 1 ? "session" : "sessions"}`));
  if (waiting) count.append(h("span", "waiting-note", `· ${waiting} waiting`));
  count.append(h("span", "spacer"), chevron(open));
  block.appendChild(sessionBlock(worktreeLines(w), color, count, open));

  const err = removeErrors.get(w.path);
  if (err) block.appendChild(errorRow(err));
  return block;
}

/** Banner: which pane gets the next key, and the group it belongs to. */
export function renderHeader() {
  const s = S.focused ? sessions.get(S.focused) : undefined;
  const at = s ? place(s) : null;
  const main = $<HTMLElement>("main");
  main.style.setProperty("--pc", accent(at?.project ?? currentProject()));

  const label = $("focus-label");
  const keysTo = $("keys-to");
  const g = activeGroupObj();
  const groupLabel = $("group-label");
  groupLabel.replaceChildren();
  if (g) {
    const n = sessionsOf(g.layout).length;
    const empty = slotsOf(g.layout).length;
    groupLabel.append(glyph(g.layout), h("span", "", g.name), h("span", "dim", empty ? `${empty} of ${n} empty` : `${n} panes`));
  }
  if (!s) {
    keysTo.textContent = g && slotsOf(g.layout).length ? "pick an agent for each empty pane" : "no session to type into";
    label.textContent = "";
    return;
  }
  keysTo.textContent = "keys go to";
  label.textContent = at
    ? `${at.project.name} / ${branchName(at.worktree)} / ${taskTitle(s)}`
    : `${s.cwd} / ${taskTitle(s)}`;
}

export function renderCounts() {
  const all = [...sessions.values()];
  const waiting = all.filter((s) => s.state === "waiting").length;
  const working = all.filter((s) => s.state === "working").length;
  $("counts").textContent = `${all.length} sessions · ${waiting} waiting · ${working} working`;
  const nw = $<HTMLButtonElement>("next-waiting");
  nw.hidden = waiting === 0;
  $("next-waiting-label").textContent = `Next waiting (${waiting})`;

  renderWelcome();
}

let welcomeSig = "";
let welcomeBox: HTMLElement | null = null;

/** First run: add a project. Just added, or nothing focused: start an agent. */
function renderWelcome() {
  const p = currentProject();
  const w = p?.worktrees.find((x) => x.is_main) ?? p?.worktrees[0];
  // Same content: keep the element, and the focus and hover on its buttons.
  const sig = JSON.stringify([S.projects.length, !!S.projectsError, p?.name, p?.color, w?.path, S.justAdded === p?.name, !!S.focused, shownIds().length, enabledAgents().map((a) => a.id)]);
  if (sig === welcomeSig && (!welcomeBox || welcomeBox.isConnected)) return;
  welcomeSig = sig;
  host.querySelector(".empty")?.remove();
  welcomeBox?.remove();
  welcomeBox = null;
  const box = h("div", "welcome");
  if (S.projects.length === 0 && !S.projectsError) {
    const add = button("welcome-primary", "Add project", () => void addProject.open());
    const hint = h("div", "welcome-hint", "or edit ");
    hint.appendChild(h("span", "mono", "~/.config/skiff/projects.toml"));
    box.append(
      h("div", "welcome-title", "Add your first project"),
      h("div", "welcome-text", "Pick a folder in a git repository. Skiff lists its worktrees and runs agents in them."),
      add,
      hint,
    );
  } else if (p && w && !shownIds().length && (S.justAdded === p.name || !S.focused)) {
    const title = h("div", "welcome-title");
    const name = h("span", "", p.name);
    name.style.color = accent(p);
    title.append(name, S.justAdded === p.name ? " is ready" : "");
    const text = h("div", "welcome-text", "Start an agent in ");
    text.append(h("span", "mono", branchName(w)), ", or make a worktree for a task first.");
    const actions = h("div", "welcome-actions");
    for (const a of [...enabledAgents(), null]) {
      const b = button("", "", () => {
        S.justAdded = null;
        newSession(w.path, a?.command ?? null, a?.id ?? "", a ? "agent" : "shell").catch(showError);
      });
      b.append(launchIcon(a), a ? a.id : "shell");
      actions.appendChild(b);
    }
    const wt = button("", "", () => newWorktree(p));
    wt.append(branchIcon(), "worktree…");
    actions.appendChild(wt);
    box.append(title, text, actions);
  } else if (!S.focused && !shownIds().length) {
    box.append(h("div", "welcome-text", "No session. Press + on a worktree to start one."));
  } else {
    return;
  }
  host.appendChild(box);
  welcomeBox = box;
}

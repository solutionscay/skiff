import { lineIndex, worktreeLines, type LineIndex } from "./sidebarLines";
/** The session tree and its focus, caret, and hover restoration. */
import { filledOf } from "./layoutSlots";
import { agentIcon, agentKind, launchIcon } from "../appearance/agentIcon";
import { ink } from "../appearance/appTheme";
import { appliedTheme, ownTheme, signatureColor } from "../appearance/themes";
import { agentName, branchName, scopeName, taskTitle } from "./model";
import { faceSig, skipperFace } from "./skipperFace";
import type { Group, Project, SessionInfo, Worktree } from "../platform/types";
import { changeCounts, changesBlock, changesSig, hasChanges, showsChanges } from "./changes";
import { filesBlock, filesSig, showsFiles } from "./files";
import { $, button, h } from "../ui/dom";
import { branchIcon, chevron, icon, plusIcon } from "../ui/icons";
import { fleetMenu, projectMenu, rowMenu, skiffAgentMenu, worktreeMenu } from "./menus";
import { launchMenu } from "../app/panels";
import { itemKey, listItems } from "../app/keyboard";
import { clearSelection, keepRow } from "./selection";
import { stateIcon } from "../appearance/stateIcon";
import { collapsed, DEFAULT_ACCENT, removeErrors, removing, S, selectedWorktree, sessions } from "../app/state";
import { accent, awayPlaces, currentWorktree, place } from "../app/stateQueries";
import { groupHome, revealSession, selectWorktree } from "./view";
import { addProject } from "../app/panels";
import { animateFold, growOpened } from "../ui/fold";
import { dragTree } from "./treeDrag";
import { bareWorktree, fleetKey, folded, hiddenSplit, isOpen, openToFocused, outsideSessions, projectBadge, projectKey, projectsHidden, rollCounts, rollup, setProjectsHidden, toggleRow, treeDrag, treeSections, type Section } from "./tree";
import { render } from "../app/renderRequest";
import { projectsPlus, renderWorkspaceStatus, shows, workspaceOpen } from "./profile";
import { ledgerCounts, ledgerNeeds, ledgerWorkTitle } from "./ledger";
import { fleetScope, fleetSkipperOf, projectSkipperOf, projectWatch, openSkiffAgent, SKIFF_AGENT_KEY, skiffAgentChoice, skiffAgentFolder, skiffAgentName, skiffAgentSession, skiffWatch, skipperWord, syncSkiffAgent } from "../app/skiffAgent";
import { hostConnected, hostName, isRemote } from "../app/hosts";
import { step, tally } from "../diagnostics/stalls";
import { sentences, t, tn } from "../i18n";
import { renderSkippers, renderViewToggle, skippersOn } from "./skippersView";
import { anchorOf, keepAnchor, restoreFocus, type Caret } from "./sidebarFocus";
import { applyLook, groupColor, look, sessionBlock, sessionRow } from "./sessionRows";

/**
 * The row's state mark. An idle row with an unread mark shows its title in
 * bold. A class does this, not `:has(.unread)`: WebKit and Chromium restyle
 * every row title on any change under a `:has()` rule.
 */
function unreadMark(row: HTMLElement, s: SessionInfo): HTMLElement {
  const mark = stateIcon(s);
  if (s.state === "idle" && mark.classList.contains("unread")) row.classList.add("unread-row");
  return mark;
}

/**
 * What the list says about the projects themselves, from the selected
 * Host's data plane: they load, they are out of reach and why, or there are
 * none. Null when the projects show and say it all.
 */
function projectsNote(): HTMLElement | null {
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

function errorRow(text: string): HTMLElement {
  const row = h("div", "error-row", text);
  row.setAttribute("role", "alert");
  return row;
}

let lastProjects: Project[] | null = null;
let lastGroups: Group[] | null = null;
let listGen = 0;
let lastSig = "";

/** What each kept block draws, by its key: the agent row, a section head, a project, Other. */
let blockSigs = new Map<string, string>();
/** The blocks the last render drew, by the same keys, and each section's rows. */
let blocks = new Map<string, HTMLElement>();
const sectionKids = new Map<string, HTMLElement>();

const AGENT_BLOCK = "agent";
const OTHER_BLOCK = "other";
const OUTSIDE_BLOCK = "outside";
const projectBlockKey = (name: string) => `project:${name}`;

/**
 * Everything the tree draws, as one string, and the part of it each block
 * draws. Renders run while agents work. A rebuilt row loses its hover,
 * restarts its icons and costs a style pass, so a block is rebuilt only when
 * what it shows changes. Expanding one project rebuilds that project.
 */
function sidebarSig(index: LineIndex): { all: string; parts: Map<string, string>; looks: Map<string, string[]> } {
  // Rows close over project, worktree and group objects. A new list brings new
  // objects, and some callers compare them by identity: the rows are rebuilt.
  if (S.projects !== lastProjects || S.groups !== lastGroups) {
    lastProjects = S.projects;
    lastGroups = S.groups;
    listGen++;
  }
  // The session rows each project draws: its own, its visitors' and its groups' members.
  const under = new Map<string, unknown[]>();
  const add = (name: string | null, row: unknown) => {
    const k = name === null ? OTHER_BLOCK : projectBlockKey(name);
    (under.get(k) ?? under.set(k, []).get(k)!).push(row);
  };
  // A state change alone rebuilds no block: applyLook writes it into the kept rows.
  // A folded row counts the states under it, so its block keeps them.
  const looksUnder = new Map<string, string[]>();
  const addLook = (name: string | null, l: string) => {
    const k = name === null ? OTHER_BLOCK : projectBlockKey(name);
    (looksUnder.get(k) ?? looksUnder.set(k, []).get(k)!).push(l);
  };
  const rowOf = new Map<string, unknown>();
  const rows = [...sessions.values()].map((s) => {
    const t = ownTheme(s);
    const at = place(s);
    const away = awayPlaces(s);
    const row = [
      s.id, s.started_at,
      taskTitle(s), agentName(s), agentKind(s), t ? ink(signatureColor(t)) : null,
      at?.worktree.path ?? null, away.map((a) => a.worktree.path),
    ];
    rowOf.set(s.id, row);
    add(at?.project.name ?? null, row);
    addLook(at?.project.name ?? null, look(s));
    for (const a of away) add(a.project.name, row);
    return [row, look(s)];
  });
  const groups = S.groups.map((g) => {
    const lead = sessions.get(g.focus ?? filledOf(g.layout)[0]);
    const row = [g.id, g.name, g.layout, g.cwd, groupColor(g), accent(lead ? place(lead)?.project : null)];
    const home = groupHome(g);
    const owner = home ? S.projects.find((p) => p.worktrees.includes(home))?.name ?? null : null;
    add(owner, [row, filledOf(g.layout).map((id) => rowOf.get(id) ?? null)]);
    return row;
  });
  const projects = S.projects.map((p) => [
    p.name, p.short, p.icon, p.fleet ?? null, p.error, p.default_branch, accent(p), selectedWorktree.get(p.name),
    p.worktrees.map((w) => [
      w.path, w.branch, w.is_main, w.locked, w.prunable, collapsed.has(w.path), removing.has(w.path), removeErrors.get(w.path),
      showsChanges(p) ? changesSig(w) : null, showsFiles(p) ? filesSig(w) : null,
    ]),
  ]);
  // The agent icons are inked against the applied app theme: a new theme re-inks them.
  // The watcher state changes with no session event when the grace ends.
  const sa = skiffAgentSession();
  const agent = skiffAgentFolder() ? [skiffAgentName(), sa?.id ?? null, skiffAgentChoice()?.id ?? null, sa ? skiffWatch(sa) : null, sa?.watch?.follows ?? null, ledgerNeeds(), sa ? [rowOf.get(sa.id), look(sa)] : null, faceSig(null)] : null;
  const base = [listGen, appliedTheme(), sa?.id ?? null];
  const parts = new Map<string, string>();
  parts.set(AGENT_BLOCK, JSON.stringify([base, agent, sa ? S.selection.includes(sa.id) : false]));
  parts.set(OTHER_BLOCK, JSON.stringify([base, under.get(OTHER_BLOCK) ?? null, S.selection, S.renamingSession, S.renaming?.id]));
  S.projects.forEach((p, i) => {
    const k = projectBlockKey(p.name);
    const open = isOpen(projectKey(p.name));
    // An open project's Skipper row is rebuilt alone (syncSkipperRow).
    const skipper = open ? !!projectSkipperOf(p.name) : skipperSig(p.name);
    const bare = bareWorktree(p);
    const lines = bare ? worktreeLines(bare, index) : [];
    const bareEmpty = bare ? !lines.length && !skipper && !showsFiles(p) && !(showsChanges(p) && hasChanges(bare)) : false;
    parts.set(k, JSON.stringify([base, projects[i].slice(0, -1), p.worktrees.map((w) => w.path), bare?.path, bareEmpty, open, p.name === S.selectedProject, skipper, open ? null : looksUnder.get(k) ?? null]));
    for (const w of p.worktrees) {
      const wtLines = worktreeLines(w, index);
      const ids = wtLines.flatMap((l) => l.group ? [l.group.id] : "session" in l ? [l.session.id] : [l.pointer.id]);
      parts.set(`worktree:${p.name}:${w.path}`, JSON.stringify([base, (projects[i][projects[i].length - 1] as unknown[][])[p.worktrees.indexOf(w)], bare === w, w.path, w.path === selectedWorktree.get(p.name), wtLines.map((l) => l.group ? [groups[S.groups.indexOf(l.group)], filledOf(l.group.layout).map((id) => rowOf.get(id))] : rowOf.get(("session" in l ? l.session : l.pointer).id)), S.selection.filter((id) => ids.includes(id)), ids.includes(S.renamingSession ?? "") ? S.renamingSession : null, ids.includes(S.renaming?.id ?? "") ? S.renaming?.id : null]));
    }
  });
  const all = JSON.stringify([
    listGen, appliedTheme(), S.selectedProject, folded.keys(), S.projects.map(shows), hiddenSig(), outsideSig(),
    S.selection, S.renamingSession, S.renaming?.id ?? null, rows, groups, projects, agent,
    S.dataState, S.dataNote,
  ]);
  return { all, parts, looks: looksUnder };
}

/** The sessions under Not in this workspace, and what their rows draw. */
function outsideSig() {
  return outsideSessions().map(({ session: x, where }) => [x.id, where, taskTitle(x), agentName(x), agentKind(x)]);
}

/** What each fleet that hides its projects shows: its head's count and the rows still under it. */
function hiddenSig() {
  return treeSections().flatMap((sec) => {
    if (!sec.fleet || !projectsHidden(sec.fleet)) return [];
    const { shown, hidden } = hiddenSplit(sec.projects);
    return [[sec.fleet, hidden, shown.map((x) => x.id)]];
  });
}

/**
 * Puts `nodes` in `parent` in order. Kept nodes stay where they are: a node
 * taken out and put back loses its style and restarts its animations.
 * `before` runs first when anything moves.
 */
function setChildren(parent: HTMLElement, nodes: HTMLElement[], before: () => void) {
  if (parent.children.length === nodes.length && nodes.every((n, i) => parent.children[i] === n)) return;
  before();
  const want = new Set<Node>(nodes);
  for (const c of [...parent.children]) if (!want.has(c)) c.remove();
  nodes.forEach((n, i) => {
    if (parent.children[i] !== n) parent.insertBefore(n, parent.children[i] ?? null);
  });
}

export function renderSidebar() {
  const side = $<HTMLElement>("sidebar-scroll");
  if (treeDrag.active) return;
  renderWorkspaceStatus();
  syncSkiffAgent();
  renderViewToggle();
  if (skippersOn()) {
    // The tree draws whole again when it comes back.
    lastSig = "";
    const active = document.activeElement as HTMLElement | null;
    const key = active?.closest("#sidebar-scroll") ? active.dataset.session ?? active.dataset.key : undefined;
    renderSkippers(side);
    restoreFocus(side, key, null, []);
    return;
  }
  openToFocused();
  const index = lineIndex();
  const sig = step("render.sidebar.sig", () => sidebarSig(index));
  if (lastSig === sig.all) return tally("sidebar.same");
  lastSig = sig.all;
  tally("sidebar.drawn");
  // Keep focus and the selected range in an inline control across re-renders,
  // so a render while a rename is open does not undo its select-all.
  const active = document.activeElement as HTMLElement | null;
  // The sidebar is rebuilt whenever sessions update. Keep focus on any list
  // row as well as inline rename fields, otherwise a selected session loses
  // its keyboard shortcuts after the next render.
  const activeKey = active?.dataset?.key ?? active?.dataset?.session ?? active?.dataset?.wt ?? active?.dataset?.group;
  const caret: Caret | null = active instanceof HTMLInputElement && active.selectionStart !== null
    ? [active.selectionStart, active.selectionEnd ?? active.selectionStart, active.selectionDirection ?? "none"]
    : null;
  // Read before the first row comes or goes: the rows, where the view is, and the
  // rows after the focused one, then before it, nearest first. If the focused row
  // goes (its session ended), the cursor moves to the next one left. A state
  // change alone moves no row: no read, no layout forced, no focus to put back.
  let items: HTMLElement[] | undefined;
  let near: string[] = [];
  let anchor: ReturnType<typeof anchorOf> = null;
  const takeAnchor = () => {
    if (items) return;
    items = listItems();
    const at = active ? items.indexOf(active) : -1;
    near = at < 0 ? [] : [...items.slice(at + 1), ...items.slice(0, at).reverse()].map(itemKey);
    anchor = anchorOf(side, items);
  };

  // The block drawn last time when what it shows is the same, else a new one.
  const next = new Map<string, HTMLElement>();
  const sigs = new Map<string, string>();
  const keep = (key: string, sig: string, build: () => HTMLElement) => {
    const old = blocks.get(key);
    const el = old && blockSigs.get(key) === sig ? old : (tally("sidebar.block"), takeAnchor(), build());
    next.set(key, el);
    sigs.set(key, sig);
    return el;
  };
  const nodes: HTMLElement[] = [];
  const kidsNow = new Set<string>();
  if (skiffAgentFolder()) nodes.push(keep(AGENT_BLOCK, sig.parts.get(AGENT_BLOCK)!, skiffAgentRow));
  for (const sec of treeSections()) {
    // A fleet head holds its projects for drag and menu. Folded, it counts
    // what they wait on, work on and left unread.
    const fleetSkipper = sec.fleet ? fleetSkipperOf(sec.fleet) : null;
    // A fleet that hides its projects keeps its Skipper and the sessions that need a look.
    const split = sec.fleet && projectsHidden(sec.fleet) ? hiddenSplit(sec.projects) : null;
    // With none of them, the head is alone: no chevron and no fold.
    const alone = !!split && !fleetSkipper && !split.shown.length;
    const open = !sec.fleet || (!alone && isOpen(fleetKey(sec.fleet)));
    const headSig = JSON.stringify([
      listGen, sec.fleet, open, workspaceOpen(), sec.projects.map((p) => p.name), split?.hidden ?? null, alone,
      open ? null : sec.projects.map((p) => [sig.parts.get(projectBlockKey(p.name)), sig.looks.get(projectBlockKey(p.name)) ?? null]),
      open || !sec.fleet ? null : [fleetSkipperOf(sec.fleet)?.state ?? null, fleetSkipperOf(sec.fleet)?.unread ?? false],
    ]);
    nodes.push(keep(`head:${sec.fleet ?? ""}`, headSig, () => sectionHead(sec, split?.hidden ?? null, alone)));
    if (!open) continue;
    const sk = sec.fleet ?? "";
    kidsNow.add(sk);
    let kids = sectionKids.get(sk);
    if (!kids) {
      kids = h("div", "tree-kids");
      if (sec.fleet) kids.dataset.kids = fleetKey(sec.fleet);
      sectionKids.set(sk, kids);
    }
    // A fleet's Skipper is the first row under its head.
    const skipperRow = sec.fleet && fleetSkipper
      ? [keep(`skipper:${sec.fleet}`, JSON.stringify([listGen, skipperSig(fleetScope(sec.fleet))]), () => projectSkipperRow(fleetScope(sec.fleet!), fleetSkipper))]
      : [];
    const shownSig = split ? JSON.stringify([listGen, appliedTheme(), S.selection, S.renamingSession, split.shown.map((x) => [x.id, x.state, x.unread, x.exit_code, x.agent_exit, taskTitle(x), agentName(x), agentKind(x), place(x)?.project.name])]) : "";
    setChildren(kids, [...skipperRow, ...(split
      ? split.shown.length ? [keep(`shown:${sec.fleet}`, shownSig, () => shownBlock(split.shown))] : []
      : sec.projects.map((p) => {
        const k = projectBlockKey(p.name);
        const block = keep(k, sig.parts.get(k)!, () => projectBlock(p, index));
        const kids = [...block.children].find((el) => el.classList.contains("tree-kids")) as HTMLElement | undefined;
        if (kids && !p.error && p.worktrees.length) {
          const bare = bareWorktree(p);
          const prefix = [...kids.children].filter((el) => !(el as HTMLElement).dataset.worktreeBlock) as HTMLElement[];
          const worktrees = p.worktrees.map((w) => {
            const wk = `worktree:${p.name}:${w.path}`;
            return keep(wk, sig.parts.get(wk)!, () => {
              const body = bare === w ? bareBlock(p, w, index) : worktreeBlock(p, w, w.path === selectedWorktree.get(p.name), accent(p), index);
              body.dataset.worktreeBlock = w.path;
              return body;
            });
          });
          setChildren(kids, [...prefix, ...worktrees], takeAnchor);
        }
        return syncSkipperRow(block, p, takeAnchor);
      }))], takeAnchor);
    nodes.push(kids);
  }
  // A folded fleet's rows start over when it opens: the fold left them shrunk.
  for (const k of [...sectionKids.keys()]) if (!kidsNow.has(k)) sectionKids.delete(k);
  const note = projectsNote();
  if (note) nodes.push(note);
  else if (S.projects.length && !S.projects.some(shows)) nodes.push(h("div", "note-row", t("sidebar.noProjectsHere")));
  // Sessions the open workspace would hide: never out of sight (#229).
  const outside = outsideSessions();
  if (outside.length) {
    nodes.push(keep(`${OUTSIDE_BLOCK}-head`, "", outsideHead));
    const outsideKey = JSON.stringify([listGen, appliedTheme(), S.selection, S.renamingSession, outsideSig()]);
    nodes.push(keep(OUTSIDE_BLOCK, outsideKey, () => outsideBlock(outside)));
  }
  // Sessions whose folder is in no project.
  if (worktreeLines(null, index).length) {
    nodes.push(keep(`${OTHER_BLOCK}-head`, "", otherHead));
    nodes.push(keep(OTHER_BLOCK, sig.parts.get(OTHER_BLOCK)!, () => sessionBlock(worktreeLines(null, index), DEFAULT_ACCENT)));
  }
  setChildren(side, nodes, takeAnchor);
  // Rows in kept blocks take the new state of their session.
  for (const row of side.querySelectorAll<HTMLElement>(".session-row[data-look]")) {
    const s = sessions.get(row.dataset.session!);
    if (s && row.dataset.look !== look(s)) {
      tally("sidebar.row");
      applyLook(row, s);
    }
  }
  blocks = next;
  blockSigs = sigs;
  if (anchor) keepAnchor(side, anchor);
  growOpened();
  if (items) restoreFocus(side, activeKey, caret, near);
}

/** The head over sessions in projects the open workspace leaves out. */
function outsideHead(): HTMLElement {
  const head = h("div", "tree-head");
  head.append(h("span", "tree-head-name", t("sidebar.outside")));
  return head;
}

/** One row for each session the open workspace would hide. Each names its project or fleet. */
function outsideBlock(outside: { session: SessionInfo; where: string }[]): HTMLElement {
  const block = h("div", "sessions-block");
  block.style.height = `calc(${Math.max(40, outside.length * 20)} * var(--u))`;
  for (const { session, where } of outside) {
    const p = S.projects.find((x) => x.name === where);
    block.appendChild(sessionRow(session, p ? accent(p) : DEFAULT_ACCENT, { where }));
  }
  return block;
}

/** The head over sessions whose folder is in no project. */
function otherHead(): HTMLElement {
  const head = h("div", "tree-head");
  head.append(h("span", "tree-head-name", t("sidebar.other")));
  return head;
}

/**
 * The Skiff agent: one fixed row above the projects, with its name from
 * Settings, its agent's glyph and title, and its state mark. Its session is
 * left out of the tree below. With no session, a click or Enter starts one.
 */
function skiffAgentRow(): HTMLElement {
  const s = skiffAgentSession();
  const name = skiffAgentName();
  const row = h("button", "session-row skiff-agent-row" + (s ? ` st-${s.state}` : "") + (s && S.selection.includes(s.id) ? " selected" : ""));
  row.type = "button";
  row.style.setProperty("--pc", DEFAULT_ACCENT);
  if (!s) {
    const a = skiffAgentChoice();
    row.dataset.key = SKIFF_AGENT_KEY;
    row.setAttribute("aria-label", t("sidebar.agentOff", { name }));
    row.append(skipperFace(null, 24), launchIcon(a), h("span", "skiff-agent-name", name), h("span", "title", a ? t("sidebar.agentStart", { agent: a.id }) : t("sidebar.noAgentOn")));
    row.addEventListener("click", () => {
      // The row is current before the start: newSession shows the pane only if
      // the current row has not moved while the agent starts.
      keepRow(SKIFF_AGENT_KEY);
      openSkiffAgent();
    });
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      skiffAgentMenu(null, e.clientX, e.clientY);
    });
    return row;
  }
  row.dataset.session = s.id;
  // The agent icon shows the watcher: teal while it runs, and slowly turning
  // while the agent is idle and the watcher follows a pane with an open ledger
  // task, so the turn means "watching work". An older watcher does not say
  // what it follows: still. Red while none runs or the last line failed.
  const watch = skiffWatch(s);
  if (watch.state === "runs") row.classList.add("watch-on");
  if (watch.state === "runs" && s.state === "idle" && (s.watch?.follows?.length ?? 0) > 0) {
    row.classList.add("watch-turn");
    // The row is rebuilt often. Start the turn at the elapsed phase, not at its first frame.
    row.style.setProperty("--turn-delay", `${-(performance.now() % 8000)}ms`);
  }
  if (watch.state === "missing" || watch.state === "failed") row.classList.add("watch-error");
  if (watch.text) row.title = watch.text;
  const vars = { name, title: s.title ?? agentName(s), state: t(`state.${s.state}`), watch: watch.text };
  row.setAttribute("aria-label", watch.text ? t("sidebar.agentLabelWatch", vars) : t("sidebar.agentLabel", vars));
  // Its face, 24 px and centered on the row, then the agent icon with the watcher, and the skiff under way while it works.
  row.append(skipperFace(null, 24), agentIcon(s), ...underWay(s), h("span", "skiff-agent-name", name), h("span", "title", s.title?.replace(/^[^\p{L}\p{N}]+/u, "") ?? ""));
  // One amber count, only while ledger tasks need the person.
  const needs = ledgerNeeds();
  if (needs) {
    const c = h("span", "ledger-count", String(needs));
    c.title = tn("ledger.tasksNeedYou", needs);
    row.append(c);
  }
  row.append(unreadMark(row, s));
  row.addEventListener("mousedown", (e) => {
    if (e.button === 0) row.focus();
  });
  row.addEventListener("click", () => {
    clearSelection();
    openSkiffAgent();
  });
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    rowMenu(s, e.clientX, e.clientY);
  });
  return row;
}

/** A Skipper row's skiff under way, the tree's working mark, while it works. */
const underWay = (s: SessionInfo) => (s.state === "working" ? [stateIcon(s)] : []);

/** What a project's or a fleet's Skipper row shows, for the block's signature. */
function skipperSig(scope: string) {
  const s = projectSkipperOf(scope);
  if (!s) return null;
  return [s.id, s.state, S.selection.includes(s.id), ledgerCounts(scope), ledgerWorkTitle(scope), s.watch ?? null, s.title ?? null, s.unread ?? false, faceSig(scope)];
}

/**
 * An open project's Skipper row changes with its state, its watcher and its
 * Ledger counts. A kept block rebuilds that row alone.
 */
function syncSkipperRow(block: HTMLElement, p: Project, before: () => void): HTMLElement {
  const s = projectSkipperOf(p.name);
  const old = block.querySelector<HTMLElement>(":scope > .tree-kids > .project-skipper-row");
  if (!s || !old) return block;
  const sig = JSON.stringify(skipperSig(p.name));
  if (block.dataset.skipper === sig) return block;
  block.dataset.skipper = sig;
  tally("sidebar.row");
  before();
  old.replaceWith(projectSkipperRow(p.name, s));
  return block;
}

/**
 * A project or fleet Skipper: the first row of its project block, or the
 * first row under its fleet head. Its agent icon, "Skipper", the title of
 * its work, and the amber and blue counts of the tasks it owns. Its session
 * is left out of the tree below.
 */
function projectSkipperRow(scope: string, s: SessionInfo): HTMLElement {
  const name = skipperWord();
  const row = h("button", "session-row project-skipper-row" + ` st-${s.state}` + (S.selection.includes(s.id) ? " selected" : ""));
  row.type = "button";
  row.dataset.session = s.id;
  // Its own watcher, as on the Skiff agent's row: teal while it runs, turning
  // while the Skipper is idle and the watcher follows a pane with an open
  // task of this project, red when its last line failed.
  const watch = projectWatch(s);
  if (watch.state === "runs") row.classList.add("watch-on");
  if (watch.state === "runs" && s.state === "idle" && (s.watch?.follows?.length ?? 0) > 0) {
    row.classList.add("watch-turn");
    row.style.setProperty("--turn-delay", `${-(performance.now() % 8000)}ms`);
  }
  if (watch.state === "failed") row.classList.add("watch-error");
  if (watch.text) row.title = watch.text;
  const title = ledgerWorkTitle(scope) ?? s.title?.replace(/^[^\p{L}\p{N}]+/u, "") ?? "";
  const { need, working } = ledgerCounts(scope);
  const vars = { scope: scopeName(scope), skipper: name, title: title || t(`state.${s.state}`), watch: watch.text };
  row.setAttribute("aria-label", watch.text ? t("sidebar.skipperLabelWatch", vars) : t("sidebar.skipperLabel", vars));
  row.append(skipperFace(scope, 20), agentIcon(s), ...underWay(s), h("span", "skiff-agent-name", name), h("span", "title", title));
  // Its tasks an agent is on: the blue count. The bell is for a waiting session only.
  const roll = rollCounts(0, working);
  if (roll) row.append(roll);
  // One amber count, only while its ledger tasks need the person, as on the Skiff agent's row.
  if (need) {
    const c = h("span", "ledger-count", String(need));
    c.title = tn("ledger.tasksNeedYou", need);
    row.append(c);
  }
  row.append(unreadMark(row, s));
  row.addEventListener("mousedown", (e) => {
    if (e.button === 0) row.focus();
  });
  row.addEventListener("click", () => {
    clearSelection();
    revealSession(s.id);
    requestAnimationFrame(() => keepRow(s.id, null));
  });
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    rowMenu(s, e.clientX, e.clientY);
  });
  return row;
}

/**
 * The sessions still under a fleet that hides its projects: each one that
 * waits, and the one on screen. Each row names its project.
 */
function shownBlock(shown: SessionInfo[]): HTMLElement {
  const block = h("div", "sessions-block fleet-shown");
  block.style.height = `calc(${shown.length * 20} * var(--u))`;
  for (const s of shown) {
    const p = place(s)!.project;
    block.appendChild(sessionRow(s, accent(p), { where: p.name }));
  }
  return block;
}

/**
 * A section head: "Projects" with its +, or a fleet that folds and opens.
 * A fleet that hides its projects counts its hidden sessions (`hidden`); a
 * click on the count shows the projects. `alone`: nothing under the head.
 */
function sectionHead(sec: Section, hidden: number | null = null, alone = false): HTMLElement {
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

/** A project: one 40px row, then its worktrees, groups and sessions while it is open. */
function projectBlock(p: Project, index: LineIndex): HTMLElement {
  const color = accent(p);
  const key = projectKey(p.name);
  const open = isOpen(key);
  const bare = bareWorktree(p);
  // Nothing under the project: no chevron, no fold, no hover.
  // changesBlock reads the worktree's changes the first time, so a project folded since the start still learns of them.
  const changes = bare && showsChanges(p) ? changesBlock(bare) : null;
  const skipper = projectSkipperOf(p.name);
  const empty = !!bare && !p.error && !worktreeLines(bare, index).length && !changes && !showsFiles(p) && !skipper;
  const block = h("div", "tree-project" + (p.name === S.selectedProject ? " selected" : ""));
  block.style.setProperty("--pc", color);

  const head = h("div", "project-row" + (empty ? " empty" : ""));
  head.dataset.project = p.name;
  head.addEventListener("mousedown", (e) => dragTree(e, block, [p], null));
  const pick = h("button", "project-pick");
  pick.type = "button";
  pick.dataset.key = pick.dataset.fold = key;
  pick.tabIndex = -1;
  if (!empty) pick.setAttribute("aria-expanded", String(open));
  const fold = h("span", "tree-fold");
  if (!empty) fold.appendChild(chevron(open));
  pick.append(projectBadge(p), h("span", "project-name", p.name));
  if (p.error) pick.appendChild(h("span", "project-meta", t("sidebar.error")));
  const roll = open ? null : rollup([p]);
  if (roll) pick.appendChild(roll);
  if (!empty) pick.appendChild(fold);
  // A click opens or folds the project, as on a fleet head. Its sessions open from their rows.
  if (!empty) pick.addEventListener("click", () => toggleRow(key, render));
  head.appendChild(pick);
  // One button: the ⋯ menu starts sessions too. A bare worktree's launch menu hangs off it.
  const more = button("wt-plus", "", () => {
    const r = more.getBoundingClientRect();
    projectMenu(p, r.right, r.top);
  });
  if (bare) more.dataset.wt = bare.path;
  more.setAttribute("aria-label", t("sidebar.menu", { name: p.name }));
  more.setAttribute("aria-haspopup", "menu");
  more.appendChild(icon('<circle cx="5" cy="12" r="1"></circle><circle cx="12" cy="12" r="1"></circle><circle cx="19" cy="12" r="1"></circle>'));
  head.appendChild(more);
  head.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    projectMenu(p, e.clientX, e.clientY);
  });
  block.appendChild(head);
  if (!open || empty) return block;

  const kids = h("div", "tree-kids");
  kids.dataset.kids = key;
  block.appendChild(kids);
  if (skipper) {
    kids.appendChild(projectSkipperRow(p.name, skipper));
    block.dataset.skipper = JSON.stringify(skipperSig(p.name));
  }
  if (p.error) kids.appendChild(errorRow(p.error));
  else if (p.worktrees.length === 0) kids.appendChild(h("div", "note-row", t("sidebar.noWorktrees")));
  return block;
}

function bareBlock(p: Project, w: Worktree, index: LineIndex): HTMLElement {
  const body = h("div", "wt-body tree-bare");
  body.appendChild(sessionBlock(worktreeLines(w, index), accent(p)));
  if (showsChanges(p)) { const changes = changesBlock(w); if (changes) body.appendChild(changes); }
  if (showsFiles(p)) body.appendChild(filesBlock(w));
  return body;
}

function worktreeBlock(p: Project, w: Worktree, selected: boolean, color: string, index: LineIndex): HTMLElement {
  const block = h("div", "wt" + (selected ? " selected" : ""));
  block.style.setProperty("--pc", color);

  const lines = worktreeLines(w, index);
  // changesBlock reads the worktree's changes the first time, folded or not.
  const changes = showsChanges(p) ? changesBlock(w) : null;
  // Nothing under the worktree: no chevron and no fold. A click still selects it.
  const empty = !lines.length && !changes && !showsFiles(p);
  const open = !empty && !collapsed.has(w.path);

  const row = h("div", "wt-row");
  const pick = button("wt-pick", "", () =>
    empty
      ? selectWorktree(p, w)
      : animateFold(w.path, !open, () => {
          if (open) collapsed.add(w.path);
          else collapsed.delete(w.path);
          selectWorktree(p, w);
        }),
  );
  pick.dataset.wt = pick.dataset.fold = w.path;
  if (!empty) pick.setAttribute("aria-expanded", String(open));
  pick.append(branchIcon(), h("span", "branch", branchName(w)));
  // The main checkout shows its tag only while it is off its usual branch.
  const usual = p.default_branch ? w.branch === p.default_branch : ["main", "master"].includes(w.branch ?? "");
  if (w.is_main && !usual) pick.appendChild(h("span", "tag", t("sidebar.mainFolder")));
  if (w.locked) pick.appendChild(h("span", "tag", t("sidebar.locked")));
  if (w.prunable) pick.appendChild(h("span", "tag", t("sidebar.prunable")));
  if (removing.has(w.path)) pick.appendChild(h("span", "tag", t("sidebar.removing")));
  // The rows under an open worktree show the detail.
  const diff = showsChanges(p) && !open ? changeCounts(w) : null;
  if (diff) pick.appendChild(diff);
  if (!empty) pick.appendChild(chevron(open));
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    worktreeMenu(p, w, e.clientX, e.clientY);
  });
  row.appendChild(pick);
  const plus = button("wt-plus", "", () => launchMenu.open(plus, p, w));
  plus.dataset.wt = w.path;
  plus.setAttribute("aria-label", t("sidebar.startIn", { name: branchName(w) }));
  plus.setAttribute("aria-haspopup", "menu");
  plus.appendChild(plusIcon());
  row.appendChild(plus);
  block.appendChild(row);

  if (open) {
    // No count line: the rows carry their own marks, and a line that comes
    // and goes would move the whole tree.
    const body = h("div", "wt-body");
    body.dataset.kids = w.path;
    body.appendChild(sessionBlock(lines, color));
    if (changes) body.appendChild(changes);
    if (showsFiles(p)) body.appendChild(filesBlock(w));
    block.appendChild(body);
  }

  const err = removeErrors.get(w.path);
  if (err) block.appendChild(errorRow(err));
  return block;
}

// Right-click on the empty part of the list: the + menu for the current worktree.
$<HTMLElement>("sidebar-scroll").addEventListener("contextmenu", (e) => {
  if (e.defaultPrevented || (e.target as Element).closest("button, input, .wt-count")) return;
  const at = currentWorktree();
  if (!at) return;
  e.preventDefault();
  launchMenu.open({ x: e.clientX, y: e.clientY }, at.p, at.w);
});

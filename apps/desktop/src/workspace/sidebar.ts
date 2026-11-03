import { lineIndex, worktreeLines } from "./sidebarLines";
/** The session tree and its focus, caret, and hover restoration. */
import { agentKind } from "../appearance/agentIcon";
import { appliedTheme } from "../appearance/themes";
import { agentName, taskTitle } from "./model";
import { $, h } from "../ui/dom";
import { launchMenu } from "../app/panels";
import { itemKey, listItems } from "../app/keyboard";
import { DEFAULT_ACCENT, S, selectedWorktree, sessions } from "../app/state";
import { accent, currentWorktree, place } from "../app/stateQueries";
import { growOpened } from "../ui/fold";
import { bareWorktree, fleetKey, hiddenSplit, isOpen, openToFocused, outsideSessions, projectsHidden, treeDrag, treeSections } from "./tree";
import { renderWorkspaceStatus, shows, workspaceOpen } from "./profile";
import { fleetScope, fleetSkipperOf, skiffAgentFolder, syncSkiffAgent } from "../app/skiffAgent";
import { step, tally } from "../diagnostics/stalls";
import { t } from "../i18n";
import { renderSkippers, renderViewToggle, skippersOn } from "./skippersView";
import { anchorOf, keepAnchor, restoreFocus, type Caret } from "./sidebarFocus";
import { applyLook, look, sessionBlock, setChildren } from "./sessionRows";
import { projectSkipperRow, skiffAgentRow, skipperSig, syncSkipperRow } from "./skipperRows";
import { AGENT_BLOCK, OTHER_BLOCK, OUTSIDE_BLOCK, listGeneration, outsideSig, projectBlockKey, sidebarSig } from "./sidebarSig";
import { bareBlock, projectBlock, worktreeBlock } from "./projectRows";
import { otherHead, outsideBlock, outsideHead, projectsNote, sectionHead, shownBlock } from "./sectionRows";

let lastSig = "";

/** What each kept block draws, by its key: the agent row, a section head, a project, Other. */
let blockSigs = new Map<string, string>();
/** The blocks the last render drew, by the same keys, and each section's rows. */
let blocks = new Map<string, HTMLElement>();
const sectionKids = new Map<string, HTMLElement>();

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
  // A block drawn again gets the old one: its session rows that still fit stay (sessionBlock).
  const keep = (key: string, sig: string, build: (old?: HTMLElement) => HTMLElement) => {
    const old = blocks.get(key);
    const el = old && blockSigs.get(key) === sig ? old : (tally("sidebar.block"), takeAnchor(), build(old));
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
      listGeneration(), sec.fleet, open, workspaceOpen(), sec.projects.map((p) => p.name), split?.hidden ?? null, alone,
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
      ? [keep(`skipper:${sec.fleet}`, JSON.stringify([listGeneration(), skipperSig(fleetScope(sec.fleet))]), () => projectSkipperRow(fleetScope(sec.fleet!), fleetSkipper))]
      : [];
    const shownSig = split ? JSON.stringify([listGeneration(), appliedTheme(), S.selection, S.renamingSession, split.shown.map((x) => [x.id, x.state, x.unread, x.exit_code, x.agent_exit, taskTitle(x), agentName(x), agentKind(x), place(x)?.project.name])]) : "";
    setChildren(kids, [...skipperRow, ...(split
      ? split.shown.length ? [keep(`shown:${sec.fleet}`, shownSig, (old) => shownBlock(split.shown, old))] : []
      : sec.projects.map((p) => {
        const k = projectBlockKey(p.name);
        const block = keep(k, sig.parts.get(k)!, () => projectBlock(p, index));
        const kids = [...block.children].find((el) => el.classList.contains("tree-kids")) as HTMLElement | undefined;
        if (kids && !p.error && p.worktrees.length) {
          const bare = bareWorktree(p);
          const prefix = [...kids.children].filter((el) => !(el as HTMLElement).dataset.worktreeBlock) as HTMLElement[];
          const worktrees = p.worktrees.map((w) => {
            const wk = `worktree:${p.name}:${w.path}`;
            return keep(wk, sig.parts.get(wk)!, (old) => {
              const rows = old?.querySelector<HTMLElement>(":scope > .sessions-block, :scope > .wt-body > .sessions-block") ?? undefined;
              const body = bare === w ? bareBlock(p, w, index, rows) : worktreeBlock(p, w, w.path === selectedWorktree.get(p.name), accent(p), index, rows);
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
    const outsideKey = JSON.stringify([listGeneration(), appliedTheme(), S.selection, S.renamingSession, outsideSig()]);
    nodes.push(keep(OUTSIDE_BLOCK, outsideKey, (old) => outsideBlock(outside, old)));
  }
  // Sessions whose folder is in no project.
  if (worktreeLines(null, index).length) {
    nodes.push(keep(`${OTHER_BLOCK}-head`, "", otherHead));
    nodes.push(keep(OTHER_BLOCK, sig.parts.get(OTHER_BLOCK)!, (old) => sessionBlock(worktreeLines(null, index), DEFAULT_ACCENT, old)));
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

// Right-click on the empty part of the list: the + menu for the current worktree.
$<HTMLElement>("sidebar-scroll").addEventListener("contextmenu", (e) => {
  if (e.defaultPrevented || (e.target as Element).closest("button, input, .wt-count")) return;
  const at = currentWorktree();
  if (!at) return;
  e.preventDefault();
  launchMenu.open({ x: e.clientX, y: e.clientY }, at.p, at.w);
});

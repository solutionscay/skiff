/** Project, worktree and bare-checkout blocks. */
import { worktreeLines, type LineIndex } from "./sidebarLines";
import { branchName } from "./model";
import type { Project, Worktree } from "../platform/types";
import { changeCounts, changesBlock, showsChanges } from "./changes";
import { filesBlock, showsFiles } from "./files";
import { button, h } from "../ui/dom";
import { branchIcon, chevron, icon, plusIcon } from "../ui/icons";
import { projectMenu, worktreeMenu } from "./menus";
import { launchMenu } from "../app/panels";
import { collapsed, removeErrors, removing, S } from "../app/state";
import { accent } from "../app/stateQueries";
import { selectWorktree } from "./view";
import { animateFold } from "../ui/fold";
import { dragTree } from "./treeDrag";
import { bareWorktree, isOpen, projectBadge, projectKey, rollup, toggleRow } from "./tree";
import { render } from "../app/renderRequest";
import { projectSkipperOf } from "../app/skiffAgent";
import { t } from "../i18n";
import { sessionBlock } from "./sessionRows";
import { projectSkipperRow, skipperSig } from "./skipperRows";

export function errorRow(text: string): HTMLElement {
  const row = h("div", "error-row", text);
  row.setAttribute("role", "alert");
  return row;
}

/** A project: one 40px row, then its worktrees, groups and sessions while it is open. */
export function projectBlock(p: Project, index: LineIndex): HTMLElement {
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

/** `rows`: the session block drawn last time, whose rows stay. */
export function bareBlock(p: Project, w: Worktree, index: LineIndex, rows?: HTMLElement): HTMLElement {
  const body = h("div", "wt-body tree-bare");
  body.appendChild(sessionBlock(worktreeLines(w, index), accent(p), rows));
  if (showsChanges(p)) { const changes = changesBlock(w); if (changes) body.appendChild(changes); }
  if (showsFiles(p)) body.appendChild(filesBlock(w));
  return body;
}

export function worktreeBlock(p: Project, w: Worktree, selected: boolean, color: string, index: LineIndex, rows?: HTMLElement): HTMLElement {
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
    body.appendChild(sessionBlock(lines, color, rows));
    if (changes) body.appendChild(changes);
    if (showsFiles(p)) body.appendChild(filesBlock(w));
    block.appendChild(body);
  }

  const err = removeErrors.get(w.path);
  if (err) block.appendChild(errorRow(err));
  return block;
}

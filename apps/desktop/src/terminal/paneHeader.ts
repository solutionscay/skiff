import type { SessionInfo } from "../platform/types";
import { agentIcon, agentKind } from "../appearance/agentIcon";
import { slotHead } from "../workspace/canvas";
import { isSlot } from "../workspace/layoutSlots";

import { type Action, keyLabel } from "../app/keys";
import { toggleFocusMode, toggleMaximize } from "../app/modes";
import { rune, type RuneName } from "../ui/runes";
import { branchName, isProjectSkipper, scopeName, skipperBase, taskTitle } from "../workspace/model";
import { faceSig, skipperFace } from "../workspace/skipperFace";
import { stateIcon } from "../appearance/stateIcon";
import { progressOf, statusNote, statusSig } from "../workspace/programStatus";

import { button, h } from "../ui/dom";

import { icon } from "../ui/icons";

import { S, sessions } from "../app/state";
import { accent, awayPlaces, inShownGroup, place, shownIds } from "../app/stateQueries";

import { closePane, dragSessions, focusPane } from "./paneActions";
import { showChild, splitChild, splitChildren, splitSkipper, stepChild, toggleSplit } from "../workspace/skipperSplit";
import { resumeOffer, resumePane } from "./resume";
import { imagesButton, newestImage } from "./images";
import { insertShown } from "./insertSignal";
import { t } from "../i18n";
/** One pane header: the agent icon, the title, then the close button. */
export function paneHead(id: string, head: HTMLElement, cell: HTMLElement) {
  if (isSlot(id)) return slotHead(id, head);
  if (!head.dataset.drag) {
    // Drag a pane by its header to move it within the layout.
    head.dataset.drag = "1";
    // The buttons show on hover too. A class, not :hover: the head rebuilds its children
    // as an agent's title changes, and WebKit drops :hover until the pointer moves.
    head.addEventListener("mouseenter", () => head.classList.add("hover"));
    head.addEventListener("mouseleave", () => head.classList.remove("hover"));
    head.addEventListener("mousedown", (e) => {
      if (!(e.target as Element).closest("button")) dragSessions(e, [id]);
    });
    // A double-click on the header zooms the pane: maximize in a split, focus mode alone.
    head.addEventListener("dblclick", (e) => {
      if ((e.target as Element).closest("button")) return;
      if (id !== S.focused) focusPane(id);
      if (shownIds().length > 1) toggleMaximize();
      else toggleFocusMode();
    });
  }
  const s = sessions.get(id);
  const at = s ? place(s) : null;
  cell.style.setProperty("--pc", accent(at?.project));
  cell.classList.toggle("focused", id === S.focused);
  // A session picked in the list shows in its pane too, so a picked pair reads as one.
  cell.classList.toggle("selected", S.selection.includes(id));
  const multi = shownIds().length > 1;
  const grouped = inShownGroup(id);
  // The Skipper split: the Skipper's head has the split button, the child's head the cycle and ×.
  const split = splitHead(id);
  const away = s ? awayPlaces(s).map((a) => branchName(a.worktree)).join(", ") : "";
  // Rebuild only on change, so a click that spans a daemon event still lands.
  const sig = s ? JSON.stringify([s.state, s.exit_code, s.agent_exit, s.unread, S.maximized === id, S.focusMode, at?.project.name, taskTitle(s), agentKind(s), multi, grouped, away, resumeOffer(s), newestImage(), insertShown(id), faceSig(s.coordinator ? null : (s.skipper ?? null)), statusSig(s), split]) : "";
  if (head.dataset.sig === sig) return;
  head.dataset.sig = sig;
  head.replaceChildren();
  if (!s) return;
  head.classList.toggle("st-waiting", s.state === "waiting");
  head.classList.toggle("st-done", s.state === "done");
  // The state reads with the name it belongs to, not with the buttons.
  head.append(agentIcon(s), headTitle(s), stateIcon(s));
  // What the program reports: what it waits for and asks, how its turn ended.
  const note = statusNote(s);
  if (note) head.appendChild(h("span", "where status-note", note));
  // The program's own progress, along the header's lower edge.
  const progress = progressOf(s);
  if (progress !== null) {
    const bar = h("span", "rp-bar head-progress");
    bar.setAttribute("role", "progressbar");
    bar.setAttribute("aria-valuenow", String(progress));
    bar.setAttribute("aria-valuemin", "0");
    bar.setAttribute("aria-valuemax", "100");
    const fill = h("i");
    fill.style.width = `${progress}%`;
    bar.append(fill);
    head.appendChild(bar);
  }
  if (split?.kind === "child" && split.title) head.appendChild(h("span", "where split-task", split.title));
  if (away) head.appendChild(h("span", "where mono", t("pane.workingIn", { from: at ? branchName(at.worktree) : t("sidebar.otherWhere"), away })));
  // After a restart: the agent this pane ran, and a button to get back to it.
  const offer = resumeOffer(s);
  if (offer) head.appendChild(h("span", "where", offer.title ? t("pane.wasTitle", { agent: offer.agent, title: offer.title }) : t("pane.was", { agent: offer.agent })));
  head.appendChild(h("span", "head-gap"));
  if (offer) {
    const r = button("head-resume", t("pane.resume"), () => {
      // Measure first: focusing redraws the head, and a replaced button measures as 0,0.
      const at = r.getBoundingClientRect();
      if (id !== S.focused) focusPane(id);
      resumePane(id, { x: at.left, y: at.bottom, right: at.right });
    });
    r.title = t("pane.resumeTip", { agent: offer.agent });
    head.appendChild(r);
  }
  if (split?.kind === "child") head.append(...cycleButtons(split.at, split.n));
  if (split?.kind === "skipper") head.appendChild(splitButton(split.n, split.on));
  const images = imagesButton(id);
  if (images) head.appendChild(images);
  // Mouse controls for the two view modes. The pane is focused first: they act on the focused pane.
  const viewBtn = (name: RuneName, label: string, key: Action, run: () => void) => {
    const b = button("head-btn", "", () => {
      if (id !== S.focused) focusPane(id);
      run();
    });
    b.title = `${label} (${keyLabel(key)})`;
    b.setAttribute("aria-label", label);
    b.appendChild(rune(name, 12));
    head.appendChild(b);
  };
  if (multi) {
    const max = S.maximized === id;
    viewBtn(max ? "view-restore" : "view-maximize", max ? t("cmd.restore") : t("cmd.maximize"), "maximize", toggleMaximize);
  }
  viewBtn("view-focus", S.focusMode ? t("cmd.leaveFocusMode") : t("cmd.focusMode"), "focus-mode", toggleFocusMode);
  // A pane of a group has a × : it takes the session out and keeps it selected.
  // The child of a Skipper split has one too: it closes the split.
  if (grouped || split?.kind === "child") {
    const x = button(grouped ? "head-btn" : "head-btn head-close-split", "", () => closePane(id));
    const label = grouped ? t("pane.removeFromGroup") : t("pane.closeSplit");
    x.title = `${label} (${keyLabel("close-pane")})`;
    x.setAttribute("aria-label", label);
    x.appendChild(icon('<path d="M6 6l12 12M18 6L6 18"></path>'));
    head.appendChild(x);
  }
}

/** The title in the head. A project Skipper reads "Skipper · gesher", with
 *  its face before the project; the Skiff agent's face comes first. */
function headTitle(s: SessionInfo): HTMLElement {
  const title = h("span", "title", taskTitle(s));
  if (s.coordinator) {
    title.prepend(skipperFace(null, 16));
    return title;
  }
  if (!isProjectSkipper(s)) return title;
  title.replaceChildren(`${skipperBase(s)} · `, skipperFace(s.skipper ?? "", 16), scopeName(s.skipper ?? ""));
  return title;
}

/** What the Skipper split adds to a head: the split button on its Skipper, the cycle on its child. */
type SplitHead = { kind: "skipper"; n: number; on: boolean } | { kind: "child"; at: number; n: number; title: string };

function splitHead(id: string): SplitHead | null {
  const sk = splitSkipper();
  if (!sk) return null;
  const child = splitChild();
  if (id !== sk && id !== child) return null;
  const list = splitChildren();
  if (id === sk) return { kind: "skipper", n: list.length, on: !!child };
  const at = list.findIndex((c) => c.id === id);
  return { kind: "child", at, n: list.length, title: at < 0 ? "" : list[at].title };
}

/** The split button with the child count: "◫ 4". With no children it is dim and does nothing. */
function splitButton(n: number, on: boolean): HTMLElement {
  const b = button("head-btn head-split", "", toggleSplit);
  const label = on ? t("pane.closeSplit") : t("pane.openSplit");
  const key = on ? "" : keyLabel("child-next");
  b.title = key ? `${label} (${key})` : label;
  b.setAttribute("aria-label", label);
  b.setAttribute("aria-pressed", String(on));
  b.classList.toggle("on", on);
  b.disabled = !n;
  b.append(rune("layouts-columns", 12), h("span", "head-split-n", String(n)));
  return b;
}

/** ‹ 2 / 4 › on the child's head. A child that left the cycle shows "– / 4". The keys stay where they are. */
function cycleButtons(at: number, n: number): HTMLElement[] {
  const step = (dir: 1 | -1, action: Action, label: string, d: string) => {
    const b = button("head-btn head-cycle", "", () => {
      // A child that left the list: the arrows go to the first or last child.
      if (at < 0) {
        const list = splitChildren();
        const c = list[dir === 1 ? 0 : list.length - 1];
        if (c) showChild(c.id);
      } else stepChild(dir);
    });
    b.title = `${label} (${keyLabel(action)})`;
    b.setAttribute("aria-label", label);
    b.disabled = !n || (n === 1 && at === 0);
    b.appendChild(icon(`<path d="${d}"></path>`));
    return b;
  };
  return [
    step(-1, "child-prev", t("pane.prevChild"), "M15 6l-6 6 6 6"),
    h("span", "head-cycle-n", `${at < 0 ? "–" : at + 1} / ${n}`),
    step(1, "child-next", t("pane.nextChild"), "M9 6l6 6-6 6"),
  ];
}

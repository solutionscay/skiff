/** The Skiff agent row and the Project and Fleet Skipper rows. */
import { agentIcon, launchIcon } from "../appearance/agentIcon";
import { agentName, scopeName } from "./model";
import { faceSig, skipperFace } from "./skipperFace";
import type { Project, SessionInfo } from "../platform/types";
import { h } from "../ui/dom";
import { rowMenu, skiffAgentMenu } from "./menus";
import { clearSelection, keepRow } from "./selection";
import { stateIcon } from "../appearance/stateIcon";
import { DEFAULT_ACCENT, S } from "../app/state";
import { revealSession } from "./view";
import { rollCounts } from "./tree";
import { ledgerCounts, ledgerNeeds, ledgerWorkTitle } from "./ledger";
import { projectSkipperOf, projectWatch, openSkiffAgent, SKIFF_AGENT_KEY, skiffAgentChoice, skiffAgentName, skiffAgentSession, skiffWatch, skipperWord } from "../app/skiffAgent";
import { tally } from "../diagnostics/stalls";
import { t, tn } from "../i18n";

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
 * The Skiff agent: one fixed row above the projects, with its name from
 * Settings, its agent's glyph and title, and its state mark. Its session is
 * left out of the tree below. With no session, a click or Enter starts one.
 */
export function skiffAgentRow(): HTMLElement {
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
  // Its face, 24 px and centered on the row, then the agent icon with the watcher. The state mark is at the row's end.
  row.append(skipperFace(null, 24), agentIcon(s), h("span", "skiff-agent-name", name), h("span", "title", s.title?.replace(/^[^\p{L}\p{N}]+/u, "") ?? ""));
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

/** What a project's or a fleet's Skipper row shows, for the block's signature. */
export function skipperSig(scope: string) {
  const s = projectSkipperOf(scope);
  if (!s) return null;
  return [s.id, s.state, S.selection.includes(s.id), ledgerCounts(scope), ledgerWorkTitle(scope), s.watch ?? null, s.title ?? null, s.unread ?? false, faceSig(scope)];
}

/**
 * An open project's Skipper row changes with its state, its watcher and its
 * Ledger counts. A kept block rebuilds that row alone.
 */
export function syncSkipperRow(block: HTMLElement, p: Project, before: () => void): HTMLElement {
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
export function projectSkipperRow(scope: string, s: SessionInfo): HTMLElement {
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
  row.append(skipperFace(scope, 20), agentIcon(s), h("span", "skiff-agent-name", name), h("span", "title", title));
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

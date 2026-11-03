/** Mission Control cards, clusters and connectors. */
import type { SessionInfo } from "../../platform/types";
import { agentIcon } from "../../appearance/agentIcon";
import { stateIcon } from "../../appearance/stateIcon";
import { childrenNote, stateLabel, statusNote } from "../programStatus";
import { hostCan } from "../../app/hostCaps";
import { hostName } from "../../app/hosts";
import { accent, place } from "../../app/stateQueries";
import { h } from "../../ui/dom";
import { ledgerTitle } from "../ledger";
import * as G from "../missionLayout";
import { projectBadge } from "../tree";
import { t, tn } from "../../i18n";
import { sinceText } from "./model";
import * as T from "./terminal";
import { placeGraph, route } from "./grid";
import { drawConnectors, type Link } from "./connectors";
import { stateParts } from "./state";
import type { BoardState } from "../missionControl";

/** Markup to compare, less the busy icon's phase, which is new on each call. */
const strip = (html: string) => html.replace(/--(water|skiff|blink|face-turn)-delay:[^;"]*;?\s*/g, "");

/** Puts `parts` in `el` when they differ from what is there. The element stays, so its light does not flicker. */
export function refill(el: HTMLElement, parts: HTMLElement[]) {
  const next = parts.map((p) => strip(p.outerHTML)).join("");
  if (el.dataset.html === next) return;
  el.dataset.html = next;
  el.replaceChildren(...parts);
}


export function createRenderer(missionState: BoardState, dom: { items: HTMLElement; strings: SVGElement; find: HTMLElement; fault: HTMLElement }, hooks: {
  model: () => BoardState["m"]; boardSignature: (now: number) => string;
  isRunning: (s: SessionInfo) => boolean; dimmed: (s: SessionInfo) => boolean;
  stateWord: (s: SessionInfo) => string; cardName: (s: SessionInfo) => string;
  lookOf: (id: string) => 1 | -1; counts: () => void; drawTicks: () => void; drawPanel: () => void;
}) {
  const { items, strings, find, fault } = dom;
  const { model, boardSignature, isRunning, dimmed, stateWord, cardName, lookOf, counts, drawTicks, drawPanel } = hooks;
  /** A session card's meta line: the task and its uid, a shell's program, or the state. */
  function metaOf(s: SessionInfo): HTMLElement {
    const el = h("span", "m");
    // What the program reports: what it waits for, how its turn ended, its subtasks.
    const note = [statusNote(s), childrenNote(s)].filter(Boolean).join(" · ");
    const r = missionState.m.rows.get(s.id);
    if (r) {
      el.append(ledgerTitle(r), " ", h("span", "mono", r.uid));
      if (note) el.append(" · ", note);
      return el;
    }
    if (note) {
      el.textContent = note;
      return el;
    }
    if (s.role === "shell" && s.foreground_program && s.state === "working") el.textContent = s.foreground_program;
    else if (s.state === "done") el.textContent = s.exit_code ? t("mission.exitedWith", { code: s.exit_code }) : t("mission.exited");
    else if (s.state === "idle" && s.unread) el.textContent = t("mission.turnEnded");
    else if (s.state === "idle" && s.role === "shell") el.textContent = t("mission.atPrompt");
    else el.textContent = stateWord(s);
    return el;
  }

  /** A session card's inside: the head and the body. Admiral and the Skippers are windows (terminal.ts). */
  function cardParts(s: SessionInfo, now: number): HTMLElement[] {
    const head = h("span", "head");
    head.appendChild(h("span", "at", sinceText(s, now)));
    const body = h("span", "body");
    const tx = h("span", "tx");
    tx.append(h("span", "t", cardName(s)), metaOf(s));
    body.append(stateIcon(s), agentIcon(s), tx);
    return [head, body];
  }

  /** A cluster's head: the badge, the name and the counts. */
  function clusterHeadParts(key: string, fleet: boolean): HTMLElement[] {
    const list = fleet ? (missionState.m.g.fleets.find((f) => f.key === key)?.projects ?? []).flatMap((p) => missionState.m.members.get(p.key) ?? []) : (missionState.m.members.get(key) ?? []);
    const waits = list.filter((s) => s.state === "waiting").length;
    const works = list.filter((s) => s.state === "working").length;
    const roll = h("span", "mission-roll");
    if (waits) roll.appendChild(h("span", "w", tn("mission.wait", waits)));
    if (works) roll.appendChild(h("span", "g", tn("mission.working", works)));
    roll.appendChild(h("span", "", fleet ? tn("palette.sessions", list.length) : String(list.length)));
    if (fleet) return [h("span", "name", key.slice("fleet:".length)), roll];
    const p = missionState.m.projects.get(key) ?? null;
    return [p ? projectBadge(p) : h("span", "name", "…"), h("span", "name", p?.name ?? t("sidebar.other")), roll];
  }

  function setClass(el: HTMLElement, cls: string, on: boolean) {
    el.classList.toggle(cls, on);
  }

  /** Draws the graph as the model has it now: cards, clusters, plans and strings, keyed so what stays stays. */
  function draw() {
    const now = Date.now();
    missionState.drawnSig = boardSignature(now);
    missionState.m = model();
    missionState.placed = placeGraph(missionState.m.g);
    // The light: a card that left the board hands it to its parent, else the first node.
    if (missionState.lit && !missionState.placed.cards.has(missionState.lit) && !missionState.placed.clusters.has(missionState.lit)) missionState.lit = G.navParent(missionState.m.g, missionState.lit) ?? G.navFirst(missionState.m.g);
    if (!missionState.lit) missionState.lit = G.navFirst(missionState.m.g);

    missionState.numbered = G.skipperOrder(missionState.m.g).filter((id) => missionState.placed.cards.has(id)).slice(0, 9);
    const seen = new Set<string>();
    // Cards: sessions, Skippers and Admiral.
    const putCard = (id: string, r: G.Rect, parts: HTMLElement[] | G.Rung, cls: string, pc: string, dim: boolean) => {
      seen.add(id);
      let el = missionState.cards.get(id);
      if (!el) {
        el = h("div", "mission-card");
        el.dataset.id = id;
        missionState.cards.set(id, el);
        items.appendChild(el);
      }
      el.className = `mission-card ${cls}`;
      setClass(el, "lit", id === missionState.lit);
      setClass(el, "dim", dim);
      el.style.setProperty("--pc", pc);
      el.style.left = `${r.x}px`;
      el.style.top = `${r.y}px`;
      if (typeof parts !== "string") refill(el, parts);
      else Object.assign(el.style, { width: `${r.w}px`, height: `${r.h}px` });
    };
    for (const [id, r] of missionState.placed.cards) {
      const s = missionState.m.sess.get(id);
      if (!s) continue;
      const rung = missionState.placed.rungs.get(id);
      const cls = [rung ? `window ${rung === "admiral" ? "admiral" : "skipper"}` : "", s.state === "waiting" ? "waiting" : ""].join(" ");
      putCard(id, r, rung ?? cardParts(s, now), cls, rung ? "var(--accent)" : accent(place(s)?.project ?? null), dimmed(s));
      if (rung) {
        const w = T.windowOf(missionState.cards.get(id)!, rung, s);
        refill(w.head, T.windowHead(s, cardName(s), missionState.numbered.indexOf(id) + 1));
        refill(w.state, stateParts(s, now, missionState.m.idx));
      }
      const label2 = t("mission.tileLabel", { title: cardName(s), state: stateLabel(s) });
      if (missionState.cards.get(id)!.getAttribute("aria-label") !== label2) missionState.cards.get(id)!.setAttribute("aria-label", label2);
    }
    for (const [id, el] of missionState.cards) {
      if (seen.has(id)) continue;
      el.remove();
      missionState.cards.delete(id);
    }

    // Clusters: the frames.
    const seenClusters = new Set<string>();
    for (const [key, c] of missionState.placed.clusters) {
      seenClusters.add(key);
      let el = missionState.clusters.get(key);
      if (!el) {
        el = h("div", "mission-cluster");
        el.dataset.id = key;
        el.appendChild(h("div", "mission-cluster-head"));
        missionState.clusters.set(key, el);
        items.appendChild(el);
      }
      const list = c.fleet ? (missionState.m.g.fleets.find((f) => f.key === key)?.projects ?? []).flatMap((p) => missionState.m.members.get(p.key) ?? []) : (missionState.m.members.get(key) ?? []);
      const skipper = c.fleet ? missionState.m.g.fleets.find((f) => f.key === key)?.skipper : null;
      const run = list.some(isRunning) || (skipper ? isRunning(missionState.m.sess.get(skipper)!) : false);
      setClass(el, "fleet", c.fleet);
      setClass(el, "lit", key === missionState.lit);
      setClass(el, "dim", (missionState.running && !run) || (!!missionState.m.found && !list.some((s) => missionState.m.found!.has(s.id))));
      el.style.setProperty("--pc", c.fleet ? "var(--accent)" : accent(missionState.m.projects.get(key) ?? null));
      Object.assign(el.style, { left: `${c.x}px`, top: `${c.y}px`, width: `${c.w}px`, height: `${c.h}px` });
      refill(el.firstElementChild as HTMLElement, clusterHeadParts(key, c.fleet));
    }
    for (const [key, el] of missionState.clusters) {
      if (seenClusters.has(key)) continue;
      el.remove();
      missionState.clusters.delete(key);
    }

    // Plans: captions.
    const seenPlans = new Set<string>();
    for (const pl of missionState.placed.plans) {
      seenPlans.add(pl.key);
      let el = missionState.plans.get(pl.key);
      if (!el) {
        el = h("div", "mission-plan");
        missionState.plans.set(pl.key, el);
        items.appendChild(el);
      }
      const text = t("mission.plan", { title: pl.plan.title, done: pl.plan.done, total: pl.plan.cards.length });
      if (el.textContent !== text) el.textContent = text;
      setClass(el, "dim", missionState.running && !pl.plan.cards.some((id) => isRunning(missionState.m.sess.get(id)!)));
      Object.assign(el.style, { left: `${pl.x}px`, top: `${pl.y}px`, width: `${pl.w}px` });
    }
    for (const [key, el] of missionState.plans) {
      if (seenPlans.has(key)) continue;
      el.remove();
      missionState.plans.delete(key);
    }

    drawStrings();
    T.placeFaces(missionState.placed, missionState.m.sess, lookOf, (id) => `${id === missionState.lit ? "lit" : ""} ${missionState.cards.get(id)?.classList.contains("dim") ? "dim" : ""}`);
    counts();
    find.style.setProperty("--pc", missionState.cards.get(missionState.lit ?? "")?.style.getPropertyValue("--pc") ?? missionState.clusters.get(missionState.lit ?? "")?.style.getPropertyValue("--pc") ?? "");
    fault.hidden = hostCan("ledger");
    if (!fault.hidden) fault.textContent = t("mission.noLedgerFault", { host: hostName() });
    drawTicks();
    drawPanel();
  }

  /** The connectors: one per link, from its owner to the child. */
  function drawStrings() {
    const links: Link[] = [];
    const done = new Set<string>();
    for (const st of missionState.m.strs) {
      const to = missionState.placed.cards.has(st.to) ? st.to : null;
      const d = to ? route(missionState.placed, st.from, to) : null;
      const key = `${st.from}>${to}`;
      if (!to || !d || done.has(key)) continue;
      done.add(key);
      const child = missionState.m.sess.get(st.to)!;
      const state = child.state === "waiting" ? "waiting" : child.state === "working" ? "working" : child.state === "done" ? "done" : "idle";
      links.push({ key, d, state, dim: dimmed(child) });
    }
    drawConnectors(links);
    // The box holds every connector, so WebKit repaints them as its own content.
    const b = G.bounds(missionState.placed);
    const w = String(Math.max(0, Math.ceil(b.x + b.w + 64)));
    const h = String(Math.max(0, Math.ceil(b.y + b.h + 64)));
    if (strings.getAttribute("width") !== w) strings.setAttribute("width", w);
    if (strings.getAttribute("height") !== h) strings.setAttribute("height", h);
  }


  return { draw };
}

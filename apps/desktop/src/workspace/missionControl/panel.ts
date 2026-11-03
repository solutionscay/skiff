/** The panel under the Mission Control canvas. */
import type { SessionInfo } from "../../platform/types";
import { stateIcon } from "../../appearance/stateIcon";
import { DEFAULT_THEME, isLight } from "../../appearance/colors";
import { themeIdFor, xtermTheme } from "../../appearance/themes";
import { keyLabel } from "../../app/keys";
import { hostCan, hostInvoke } from "../../app/hostCaps";
import { skiffAgentName, skiffAgentSession } from "../../app/skiffAgent";
import { S, sessions } from "../../app/state";
import { place } from "../../app/stateQueries";
import { h } from "../../ui/dom";
import { ledgerTitle, needWord, taskState } from "../ledger";
import { childrenNote, statusNote } from "../programStatus";
import { t, tn } from "../../i18n";
import { sinceText } from "./model";
import { refill } from "./render";
import type { BoardState } from "../missionControl";

export function createPanel(missionState: BoardState, panel: HTMLElement, screen: HTMLElement, hooks: {
  saveKept: () => void; drawTicks: () => void; cardName: (s: SessionInfo) => string;
  stateWord: (s: SessionInfo) => string; ownerOf: (owner: string, admiral: SessionInfo | null) => SessionInfo | null;
}) {
  const { saveKept, drawTicks, cardName, stateWord, ownerOf } = hooks;
  const ROWS = 12;
  const OTHER = "project:\u0000other";
  const FOLD_SVG = '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M1.5 3.5 5 7l3.5-3.5"/></svg>';

  /** The session's terminal theme, as its pane shows it. */
  function paint(pre: HTMLElement, s: SessionInfo) {
    const id = themeIdFor(s);
    const t = S.themes.find((x) => x.id === id) ?? S.themes.find((x) => x.id === DEFAULT_THEME);
    if (!t) return;
    pre.style.background = t.background;
    pre.style.color = xtermTheme(t).foreground;
    pre.style.colorScheme = isLight(t.background) ? "light" : "dark";
  }

  function togglePanel() {
    missionState.panelOpen = !missionState.panelOpen;
    saveKept();
    drawPanel();
    requestAnimationFrame(drawTicks);
  }

  const panelHead = h("div", "mission-panel-head");
  const panelBody = h("div", "mission-panel-body");
  const side = h("div", "mission-side");
  panelBody.append(screen, side);
  panel.append(panelHead, panelBody);
  const foldBtn = h("button", "fold");
  foldBtn.type = "button";
  foldBtn.tabIndex = -1;
  foldBtn.innerHTML = FOLD_SVG;
  foldBtn.addEventListener("click", togglePanel);

  /** The panel: its nodes stay; a part is replaced only when its text changes. Folded, only the head updates. */
  function drawPanel() {
    panel.classList.toggle("folded", !missionState.panelOpen);
    foldBtn.title = t("mission.foldPanel", { key: keyLabel("ledger") });
    const sideHead = (text: string, mono = "") => {
      const el = h("div", "mission-side-head", text);
      el.appendChild(h("span", "mono", mono));
      return el;
    };
    const kv = (pairs: [string, string, string?][]) => {
      const el = h("div", "mission-kv");
      for (const [k, v, cls] of pairs) el.append(h("div", "", k), h("div", cls ?? "", v));
      return el;
    };
    const head = (parts: HTMLElement[]) => {
      refill(panelHead, parts);
      if (foldBtn.parentNode !== panelHead) panelHead.appendChild(foldBtn);
    };
    const s = missionState.lit ? missionState.m.sess.get(missionState.lit) : null;
    if (!s) {
      const list = missionState.lit ? (missionState.m.g.fleets.find((f) => f.key === missionState.lit)?.projects ?? []).flatMap((p) => missionState.m.members.get(p.key) ?? []).concat(missionState.m.members.get(missionState.lit) ?? []) : [];
      const name = missionState.lit?.startsWith("fleet:") ? missionState.lit.slice("fleet:".length) : (missionState.m.projects.get(missionState.lit ?? "")?.name ?? (missionState.lit === OTHER ? t("sidebar.other") : ""));
      head([h("span", "title", name), h("span", "state", `${tn("mission.waiting", list.filter((x) => x.state === "waiting").length)} · ${tn("mission.working", list.filter((x) => x.state === "working").length)} · ${tn("palette.sessions", list.length)}`)]);
      if (missionState.screenFor !== null) {
        screen.textContent = "";
        screen.classList.add("note");
        screen.removeAttribute("style");
        missionState.screenFor = null;
      }
      if (missionState.panelOpen) refill(side, [h("div", "mission-task none", t("mission.clusterNoPane"))]);
      return;
    }
    const p = place(s)?.project ?? null;
    const title = s.coordinator || !!s.skipper ? cardName(s) : p ? `${p.name} · ${cardName(s)}` : cardName(s);
    head([stateIcon(s), h("span", "title", title), h("span", `state${s.state === "waiting" ? " w" : ""}`, [stateWord(s), statusNote(s), childrenNote(s), sinceText(s, Date.now())].filter(Boolean).join(" · "))]);
    if (missionState.screenFor !== s.id) {
      missionState.screenFor = s.id;
      screen.textContent = "";
      screen.classList.remove("note");
    }
    if (!missionState.panelOpen) return;
    paint(screen, s);
    const parts: HTMLElement[] = [];
    const r = missionState.m.rows.get(s.id);
    if (s.coordinator || !!s.skipper) {
      const hos = s.coordinator ? missionState.m.idx.open : (missionState.m.idx.paneHandoffs.get(s.id) ?? []);
      parts.push(sideHead(t("mission.handoffsHead"), String(hos.length)));
      if (!hostCan("ledger")) parts.push(h("div", "mission-task none", t("mission.noLedger")));
      else if (!hos.length) parts.push(h("div", "mission-task none", t("mission.noneDot")));
      else parts.push(kv(hos.map((x) => [x.uid, `${ledgerTitle(x)} · ${s.coordinator ? (x.pane_id && sessions.get(x.pane_id) ? cardName(sessions.get(x.pane_id)!) : x.project) : t("mission.from", { name: skiffAgentName() })}`])));
    } else {
      parts.push(sideHead(t("mission.taskHead"), r ? r.uid : ""));
      if (!hostCan("ledger")) parts.push(h("div", "mission-task none", t("mission.noLedger")));
      else if (!r) parts.push(h("div", "mission-task none", t("mission.noTask")));
      else {
        const st = taskState(r);
        const owner = ownerOf(r.owner, skiffAgentSession());
        parts.push(h("div", "mission-task", r.task), kv([
          [t("mission.owner"), owner ? cardName(owner) : r.owner === "grand" ? skiffAgentName() : r.owner],
          [t("mission.state"), needWord(st) ?? st, needWord(st) ? "w" : ""],
          [t("mission.result"), t(r.state === "done" ? (r.result_exists ? "mission.resultReady" : r.flushed_at ? "mission.resultFlushed" : "mission.resultNone") : "mission.resultNotYet"), "mono"],
        ]));
      }
    }
    refill(side, parts);
  }

  /** Reads the lit card's screen. One read a second, for that card only. */
  async function read() {
    const id = missionState.screenFor;
    if (!id || missionState.reading || !missionState.panelOpen || missionState.paused) return;
    missionState.reading = true;
    try {
      const lines = await hostInvoke<string[]>("session_screen", "session_screen", { session: id, rows: ROWS });
      if (missionState.paused || missionState.screenFor !== id) return;
      const text = lines.join("\n");
      if (screen.textContent !== text) screen.textContent = text;
      screen.classList.remove("note");
    } catch (e) {
      if (missionState.paused || missionState.screenFor !== id) return;
      // An older skiffd does not know the request.
      screen.textContent = /unknown variant|bad request/.test(String(e)) ? t("mission.updateDaemon") : String(e);
      screen.classList.add("note");
    } finally {
      missionState.reading = false;
    }
  }


    return { drawPanel, read, togglePanel };
}

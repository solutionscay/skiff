/** Pointer pan, zoom and selection on the board. */
import type { BoardState } from "../missionControl";
import * as G from "../missionLayout";
import * as T from "./terminal";
import { ctxMenu } from "../../ui/contextMenu";
import { closeMission, goToSession } from "../missionMotion";
import { type Action, actionFor } from "../../app/keys";
import { settings } from "../../app/panels";
import { openSkiffAgent, skiffAgentSession } from "../../app/skiffAgent";
import { t, upper } from "../../i18n";

export interface Pan {
  x: number;
  y: number;
  /** The card or face under the pointer, for the click. */
  id: string | null;
  /** The cluster whose head is under the pointer. */
  head: string | null;
  moved: boolean;
  vx: number;
  vy: number;
}

export function attachPointer(missionState: BoardState, canvas: HTMLElement, root: HTMLElement, hooks: {
  light: (id: string | null, pan?: boolean) => void; go: () => void;
  applyView: () => void; zoomBy: (factor: number, x?: number, y?: number) => void;
}) {
  const { light, go, applyView, zoomBy } = hooks;
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const t = e.target as Element;
    const tick = t.closest<HTMLElement>(".mission-tick");
    if (tick?.dataset.id) {
      light(tick.dataset.id);
      return;
    }
    // A face is its window.
    const id = t.closest<HTMLElement>(".mission-card, .mission-face")?.dataset.id ?? null;
    const head = t.closest(".mission-cluster-head")?.closest<HTMLElement>(".mission-cluster")?.dataset.id ?? null;
    missionState.pan = { x: e.clientX, y: e.clientY, id, head, moved: false, vx: missionState.view.x, vy: missionState.view.y };
    canvas.setPointerCapture(e.pointerId);
    canvas.classList.add("panning");
    e.preventDefault();
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!missionState.pan) return;
    const dx = e.clientX - missionState.pan.x;
    const dy = e.clientY - missionState.pan.y;
    if (!missionState.pan.moved && Math.hypot(dx, dy) < 4) return;
    missionState.pan.moved = true;
    missionState.view.x = missionState.pan.vx + dx;
    missionState.view.y = missionState.pan.vy + dy;
    applyView();
  });
  const endPan = (e: PointerEvent) => {
    if (!missionState.pan) return;
    canvas.classList.remove("panning");
    const d = missionState.pan;
    missionState.pan = null;
    if (d.moved || e.type === "pointercancel") return;
    root.focus();
    if (d.head) return light(d.head, false);
    if (!d.id) return;
    light(d.id, false);
    go();
  };
  canvas.addEventListener("pointerup", endPan);
  canvas.addEventListener("pointercancel", endPan);
  // The wheel pans. With Ctrl (or a pinch), it zooms at the pointer. Shift+wheel pans sideways.
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const r = canvas.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey) zoomBy(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top);
      else {
        missionState.view.x -= e.shiftKey ? e.deltaY : e.deltaX;
        missionState.view.y -= e.shiftKey ? 0 : e.deltaY;
        applyView();
      }
    },
    { passive: false },
  );
  canvas.addEventListener("mousemove", (e) => {
    if (missionState.pan) return;
    const at = `${e.clientX},${e.clientY}`;
    if (at === missionState.pointer) return;
    missionState.pointer = at;
    const el = (e.target as Element).closest<HTMLElement>(".mission-card, .mission-face");
    if (el?.dataset.id && el.dataset.id !== missionState.lit) light(el.dataset.id, false);
  });


  }

  export function createNavigation(missionState: BoardState, dom: { canvas: HTMLElement; world: HTMLElement; zoomLevel: HTMLElement; zoomBar: HTMLElement; find: HTMLElement; root: HTMLElement }, hooks: {
    appZoom: () => number; zoomBtn: (text: string, title: string, run: () => void) => HTMLElement;
    drawTicks: () => void; drawPanel: () => void; read: () => Promise<void>;
}) {
  const { canvas, world, zoomLevel, zoomBar, find, root } = dom;
  const { appZoom, zoomBtn, drawTicks, drawPanel, read } = hooks;
  function applyView() {
    world.style.transform = `translate(${missionState.view.x}px, ${missionState.view.y}px) scale(${missionState.view.z})`;
    zoomLevel.textContent = `${Math.round((missionState.view.z / appZoom()) * 100)}%`;
    drawTicks();
  }

  function fit() {
    missionState.view = G.fitView(G.bounds(missionState.placed), canvas.clientWidth, canvas.clientHeight, 24 * appZoom());
    missionState.view.z = Math.min(missionState.view.z, appZoom());
    const b = G.bounds(missionState.placed);
    missionState.view.x = (canvas.clientWidth - b.w * missionState.view.z) / 2 - b.x * missionState.view.z;
    missionState.view.y = (canvas.clientHeight - b.h * missionState.view.z) / 2 - b.y * missionState.view.z;
    applyView();
  }

  /** The whole graph, then closer on the lit card when the fit is small. */
  function openView() {
    fit();
    const z = appZoom();
    if (missionState.view.z >= 0.85 * z) return;
    const r = rectOf(missionState.lit);
    if (!r) return;
    missionState.view = G.centerOn({ ...missionState.view, z: 0.85 * z }, r, canvas.clientWidth, canvas.clientHeight);
    missionState.view.y = Math.min(24, missionState.view.y);
    applyView();
  }

  function zoomBy(f: number, px = canvas.clientWidth / 2, py = canvas.clientHeight / 2) {
    const z = appZoom();
    const next = G.zoomView({ ...missionState.view, z: missionState.view.z / z }, f, px, py);
    missionState.view = { ...next, z: next.z * z };
    applyView();
  }

  /** Where `id` is on the canvas: its card or cluster. */
  function rectOf(id: string | null): G.Rect | null {
    if (!id) return null;
    const stand = missionState.placed.cards.has(id) || missionState.placed.clusters.has(id) ? id : null;
    return stand ? (missionState.placed.cards.get(stand) ?? missionState.placed.clusters.get(stand) ?? null) : null;
  }

  /** Brings `id` into view when it is out of it. */
  function panTo(id: string | null) {
    const r = rectOf(id);
    if (!r || G.inView(missionState.view, r, canvas.clientWidth, canvas.clientHeight)) return;
    missionState.view = G.centerOn(missionState.view, r, canvas.clientWidth, canvas.clientHeight);
    applyView();
  }

  zoomBar.append(zoomBtn("−", t("mission.zoomOut", { key: "-" }), () => zoomBy(1 / 1.2)), zoomBtn("+", t("mission.zoomIn", { key: "=" }), () => zoomBy(1.2)), zoomBtn("⌂", t("mission.fitTip", { key: "Home" }), fit), zoomLevel);

  // ---- The light, and the moves ----

  /** `pan`: bring the card into view, for keys. The pointer is already on it. */
  function light(id: string | null, pan = true) {
    if (!id) return;
    missionState.lit = id;
    for (const [k, el] of missionState.cards) el.classList.toggle("lit", k === id);
    for (const [k, el] of missionState.clusters) el.classList.toggle("lit", k === id);
    for (const el of T.faceLayer.children) el.classList.toggle("lit", (el as HTMLElement).dataset.id === id);
    find.style.setProperty("--pc", missionState.cards.get(id)?.style.getPropertyValue("--pc") ?? missionState.clusters.get(id)?.style.getPropertyValue("--pc") ?? "");
    if (pan) panTo(id);
    drawPanel();
    void read();
  }

  const isCluster = (id: string) => id.startsWith("project:") || id.startsWith("fleet:");

  /** The next waiting card after the lit one, around the board. */
  function nextWaitingCard(): string | null {
    const ids = [...missionState.cards].filter(([id, el]) => el.classList.contains("waiting") && !isCluster(id)).map(([id]) => id);
    if (!ids.length) return null;
    const i = ids.indexOf(missionState.lit ?? "");
    return ids[(i + 1) % ids.length];
  }

  /** The lit node's session: a card's own, a cluster's first. `null` with nothing to go to. */
  function litSession(): string | null {
    if (!missionState.lit) return null;
    const id = isCluster(missionState.lit) ? (missionState.m.g.fleets.find((f) => f.key === missionState.lit)?.projects.flatMap((p) => p.cards)[0] ?? [...missionState.m.g.fleets.flatMap((f) => f.projects), ...missionState.m.g.projects].find((p) => p.key === missionState.lit)?.cards[0] ?? null) : missionState.lit;
    return id && missionState.m.sess.has(id) ? id : null;
  }

  /** To the lit node's session, maximized or not. */
  function go(max = false) {
    const id = litSession();
    if (id) closeMission(id, max);
  }

  /** The board menu: the layout as a whole. */
  function openMenu() {
    const at = missionState.cards.get(missionState.lit ?? "") ?? missionState.clusters.get(missionState.lit ?? "");
    const r = (at ?? canvas).getBoundingClientRect();
    const back = () => requestAnimationFrame(() => root.focus());
    ctxMenu.open(
      r.left,
      at ? r.bottom : r.top,
      upper(t("cmd.missionControl")),
      [
        { label: t("mission.fit"), hint: "Home", run: () => (fit(), back()) },
      ],
      back,
    );
  }


    return { applyView, fit, openView, zoomBy, rectOf, light, nextWaitingCard, litSession, go, openMenu };
  }

  export function createKeyboard(missionState: BoardState, dom: { root: HTMLElement; find: HTMLElement; search: HTMLInputElement }, hooks: {
    missionOpen: () => boolean; light: (id: string | null, pan?: boolean) => void; go: (max?: boolean) => void;
    fit: () => void; zoomBy: (factor: number) => void; draw: () => void; toggleRunning: () => void;
    nextWaitingCard: () => string | null; togglePanel: () => void; openMenu: () => void;
}) {
  const { root, find, search } = dom;
  const { missionOpen, light, go, fit, zoomBy, draw, toggleRunning, nextWaitingCard, togglePanel, openMenu } = hooks;
  /** Runs a board command. `false`: the board has no such command. */
  function run(cmd: G.BoardCommand | null): boolean {
    if (!cmd) return false;
    switch (cmd) {
      case "parent": light(missionState.lit ? G.navParent(missionState.m.g, missionState.lit) : null); break;
      case "child": light(missionState.lit ? (G.navChildren(missionState.m.g, missionState.lit)[0] ?? null) : null); break;
      case "prev": light(missionState.lit ? G.navSibling(missionState.m.g, missionState.lit, -1) : null); break;
      case "next": light(missionState.lit ? G.navSibling(missionState.m.g, missionState.lit, 1) : null); break;
      case "go": go(); break;
      case "go-max": go(true); break;
      case "fit": fit(); break;
      case "zoom-in": zoomBy(1.2); break;
      case "zoom-out": zoomBy(1 / 1.2); break;
      case "close": closeMission(); break;
      case "clear-search":
        search.value = "";
        draw();
        break;
      case "search": toggleSearch(); break;
      case "running": toggleRunning(); break;
      case "next-waiting": light(nextWaitingCard()); break;
      case "panel": togglePanel(); break;
      case "menu": openMenu(); break;
      case "settings":
        closeMission();
        void settings.open();
        break;
      case "skipper": {
        const s = skiffAgentSession();
        if (s) goToSession(s.id);
        else {
          closeMission();
          openSkiffAgent();
        }
        break;
      }
    }
    return true;
  }

  /** The action keys with a meaning on the open board. The menu bar sends its accelerators here through `runAction()`. */
  const BOARD_ACTIONS = new Set<Action>(["session-next", "session-prev", "list-project", "list-back", "next-waiting", "ledger", "project-menu"]);

  /** An action key on the open board: the moves, the next waiting card, the panel and the menu. */
  function missionKey(a: Action | null): boolean {
    if (!a || !missionOpen() || !BOARD_ACTIONS.has(a)) return false;
    return run(G.boardCommand("", a, false, false, { ctrl: false, shift: false, alt: false, meta: false }));
  }

  /** Ctrl+Shift+1..9 here: the Skipper card with that number, as a click on it. */
  function pickSkipper(n: number) {
    const id = missionState.numbered[n - 1];
    if (!id || !missionState.placed.cards.has(id)) return;
    light(id, false);
    go();
  }

  /** Ctrl+Shift+P here: into the search, or back out to the board. The search stays. */
  function toggleSearch() {
    if (document.activeElement === search) root.focus();
    else {
      search.focus();
      search.select();
    }
  }

  search.addEventListener("input", () => draw());
  /** The board's keys, on the board and in its search. */
  function onKey(e: KeyboardEvent) {
    const a = actionFor(e);
    if (a === "quit") return;
    const cmd = G.boardCommand(e.key, a, e.target === search, !!search.value, { ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey, meta: e.metaKey });
    if (!run(cmd)) return;
    e.preventDefault();
    e.stopPropagation();
  }
  root.addEventListener("keydown", onKey);
  find.addEventListener("keydown", onKey);


  return { missionKey, pickSkipper, toggleSearch };
}

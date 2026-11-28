import { routeOf, pageRoute, withinHost, type HostRoute } from "../platform/invoke";
import { sameRoute } from "../platform/hostRoute";
import { bySessionPriority, byStart, isUnread } from "../workspace/model";
import { closePreview } from "../terminal/peek";
import { goToSession } from "../workspace/missionMotion";
import { connection, focusConnection, orderedConnections } from "./connections";
import { S, sessions } from "./state";

/** Read each live inventory, including panes outside the open workspace and folded trees. */
export function liveTrees<T>(read: (route: HostRoute) => T): T[] {
  return orderedConnections()
    .filter((c) => c.phase === "up")
    .map((c) => {
      const route = routeOf(c.entry)!;
      return withinHost(route, () => read(route));
    });
}

export function hostCounts() {
  let waiting = 0;
  let working = 0;
  let unread = 0;
  for (const s of sessions.values()) {
    if (s.state === "waiting") waiting++;
    if (s.state === "working") working++;
    if (isUnread(s)) unread++;
  }
  return { waiting, working, unread };
}

export function liveRoute(route: HostRoute) {
  return connection(route.entry)?.phase === "up" && sameRoute(route, routeOf(route.entry) ?? null);
}

export function openAttention(route: HostRoute, id: string): boolean {
  if (!liveRoute(route) || !withinHost(route, () => sessions.has(id))) return false;
  const c = connection(route.entry)!;
  c.folded = false;
  closePreview();
  focusConnection(c, false);
  goToSession(id);
  return true;
}

export function nextAttention(kind: "waiting" | "unread") {
  const candidates = liveTrees((route) =>
    [...sessions.values()].filter((s) => s.state === "waiting" || isUnread(s)).sort(byStart).map((s) => ({ route, s })),
  ).flat();
  const waiting = kind === "waiting" && candidates.some(({ s }) => s.state === "waiting");
  const panes = candidates.filter(({s}) => waiting ? s.state === "waiting" : isUnread(s));
  if (!panes.length) return;
  const current = panes.findIndex(({ route, s }) => route.entry === pageRoute()?.entry && s.id === S.focused);
  // Enter at the highest-priority pane. Once inside, cycle in stable Host and start
  // order so fresh output from an already visited pane cannot jump the queue.
  const next = current < 0
    ? panes.reduce((best, item) => bySessionPriority(item.s,best.s) < 0 ? item : best)
    : panes[(current + 1) % panes.length];
  openAttention(next.route, next.s.id);
}

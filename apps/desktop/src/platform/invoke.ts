import { invoke as send, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { advances, sameRoute, type HostRoute } from "./hostRoute";

export { Channel, type InvokeArgs } from "@tauri-apps/api/core";
export type { HostRoute };
export interface HostEvent<T> { route: HostRoute; value: T }

let current: HostRoute | null = null;
let removed = false;
let starting: Promise<void> | undefined;
const bindings = new Set<(host: string) => void>();

export const pageRoute = () => current;
export const accepts = (route: HostRoute | null) => !removed && sameRoute(route, current);

function bind(route: HostRoute) {
  if (removed) return;
  if (current && !advances(current, route)) return;
  if (current?.host && current.host !== route.host) {
    removed = true;
    location.reload();
    return;
  }
  current = route;
  if (route.host !== null) for (const run of bindings) run(route.host);
}

/** The outer key is the accepted Host. Inside each store, IDs stay Host-local. */
export function hostStore<T>(create: () => T, select: (store: T) => void): void {
  const pending = create();
  const hosts = new Map<string, T>();
  const bindHost = (host: string) => {
    const store = hosts.get(host) ?? (hosts.size ? create() : pending);
    hosts.set(host, store);
    select(store);
  };
  select(pending);
  bindings.add(bindHost);
  if (current?.host) bindHost(current.host);
}

async function start() {
  let newest: HostRoute | null = null;
  await listen<HostRoute>("skiff:host-removed", ({ payload }) => {
    if (current?.entry === payload.entry) removed = true;
  });
  await listen<HostRoute>("skiff:host-route", ({ payload }) => {
    if (current) bind(payload);
    else newest = payload;
  });
  bind(await send<HostRoute>("host_route"));
  if (newest) bind(newest);
}

export async function invoke<T = void>(cmd: string, args?: InvokeArgs, hostId?: string): Promise<T> {
  if (!current) await (starting ??= start());
  const route = current!;
  if (removed || (hostId !== undefined && hostId !== route.host)) throw new Error(CHANGED);
  const payload = { ...args, route };
  if (connectionCommands.has(cmd)) {
    const reply = await send<HostEvent<T>>(cmd, payload);
    bind(reply.route);
    if (!accepts(reply.route)) throw new Error(CHANGED);
    return reply.value;
  }
  const result = await send<T>(cmd, payload);
  if (cmd === "select_host" || cmd === "accept_host_identity") return result;
  if (cmd === "list_hosts" && !removed && route.entry === current?.entry && !accepts(route)) return invoke<T>(cmd, args);
  if (!accepts(route)) throw new Error(CHANGED);
  return result;
}

const CHANGED = "The Host changed. The page subscribes again.";
const connectionCommands = new Set(["daemon_status", "restart_daemon", "reload_daemon", "update_daemon"]);

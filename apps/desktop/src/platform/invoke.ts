import { invoke as send, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { advances, sameRoute, type HostRoute } from "./hostRoute";

export { Channel, type InvokeArgs } from "@tauri-apps/api/core";
export type { HostRoute };
export interface HostEvent<T> { route: HostRoute; value: T }

const routes = new Map<number, HostRoute>();
let current: HostRoute | null = null;
let starting: Promise<void> | undefined;
const bindings = new Set<(key: number) => void>();
const forget = new Set<(key: number) => void>();

export const pageRoute = () => current;
export const accepts = (r: HostRoute | null) => !!r && routes.has(r.entry) && sameRoute(r, current);
export const routeOf = (entry: number) => routes.get(entry);
export const allRoutes = () => [...routes.values()];

let visible: number | undefined;
export const foreground = () => current?.entry === visible;
export function showRoute(route: HostRoute) {
  visible = route.entry;
  bind(route);
}
function bind(route: HostRoute) {
  current = routes.get(route.entry) ?? route;
  for (const bind of bindings) bind(current.entry);
}

/** A synchronous read or update of another Host's cached tree. */
export function withinHost<T>(route: HostRoute, read: () => T): T {
  const previous = current;
  bind(route);
  try { return read(); }
  finally { if (previous) bind(previous); }
}

export function registerRoute(route: HostRoute) {
  const old = routes.get(route.entry);
  if (!old || advances(old, route)) routes.set(route.entry, route);
  if (current?.entry === route.entry) current = routes.get(route.entry)!;
}

export function forgetRoute(entry: number) {
  routes.delete(entry);
  for (const remove of forget) remove(entry);
}

export function hostStore<T>(create: () => T, select: (store: T) => void): void {
  const initial = create();
  const stores = new Map<number, T>();
  let first = true;
  const bind = (key: number) => {
    if (!stores.has(key)) {
      stores.set(key, first ? initial : create());
      first = false;
    }
    select(stores.get(key)!);
  };
  select(initial);
  bindings.add(bind);
  forget.add((key) => stores.delete(key));
  if (current) bind(current.entry);
}

async function start() {
  await listen<HostRoute>("skiff:host-route", ({ payload }) => {
    if (routes.has(payload.entry)) registerRoute(payload);
  });
  const route = await send<HostRoute>("host_route");
  registerRoute(route);
  showRoute(route);
}

const CHANGED = "The Host changed. The page subscribes again.";
const connectionCommands = new Set(["daemon_status", "restart_daemon", "reload_daemon", "update_daemon"]);

export async function invokeAt<T = void>(route: HostRoute, cmd: string, args?: InvokeArgs): Promise<T> {
  if (!sameRoute(route, routes.get(route.entry) ?? null)) throw new Error(CHANGED);
  const payload = { ...args, route };
  if (connectionCommands.has(cmd)) {
    const reply = await send<HostEvent<T>>(cmd, payload);
    if (!routes.has(route.entry)) throw new Error(CHANGED);
    registerRoute(reply.route);
    if (!sameRoute(reply.route, routes.get(route.entry)!)) throw new Error(CHANGED);
    return reply.value;
  }
  const result = await send<T>(cmd, payload);
  if (!sameRoute(route, routes.get(route.entry) ?? null)) throw new Error(CHANGED);
  return result;
}

export async function invoke<T = void>(cmd: string, args?: InvokeArgs, hostId?: string): Promise<T> {
  if (!current) await (starting ??= start());
  const route = current!;
  if (hostId !== undefined && hostId !== route.host) throw new Error(CHANGED);
  const value = await invokeAt<T>(route, cmd, args);
  if (current?.entry !== route.entry) throw new Error(CHANGED);
  return value;
}

/** Modules with mutable scalar fields save them alongside their Host-local maps. */
export function hostFields<T extends object>(make: () => T, read: () => T, write: (fields: T) => void) {
  let previous: T | undefined;
  hostStore(make, fields => {
    if (fields === previous) return;
    if (previous) Object.assign(previous, read());
    previous = fields;
    write(fields);
  });
}

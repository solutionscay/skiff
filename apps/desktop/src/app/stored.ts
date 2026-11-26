/** View state kept in localStorage, so it survives a restart of the app. */
import type { Layout } from "../platform/types";

/** A set of strings saved under `key` on every change. */
export function storedSet(key: string): Set<string> {
  let items: string[] = [];
  try {
    const v: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    if (Array.isArray(v)) items = v.filter((x): x is string => typeof x === "string");
  } catch {
    /* start empty */
  }
  const set = new Set(items);
  const save = () => {
    try {
      localStorage.setItem(key, JSON.stringify([...set]));
    } catch {
      /* the state lasts for this run */
    }
  };
  const add = set.add.bind(set);
  const del = set.delete.bind(set);
  set.add = (v) => (add(v), save(), set);
  set.delete = (v) => {
    const had = del(v);
    if (had) save();
    return had;
  };
  return set;
}

/** Where the user was when the app last drew: restored at the next start. */
export type Place = {
  project: string | null;
  session: string | null;
  group: string | null;
  picked: boolean;
  mode: "focus" | "maximize" | null;
  /** The Skipper split on screen: its Skipper, its child pane and their tree. */
  split?: { skipper: string; child: string; layout: Layout } | null;
};

const LEGACY_PLACE_KEY = "skiff.place";
const placeKey = (host: string | null) => host === null ? "skiff.place.local" : `skiff.place.host.${host}`;
/** Off until the start has restored the saved place, so the first empty draw does not overwrite it. */
let trackedPlaceKey: string | null = null;
let placeSig = "";

export function loadPlace(host: string | null, identity?: string): Partial<Place> {
  const key = identity ? identityPlaceKey(identity) : placeKey(host);
  try {
    const saved = localStorage.getItem(key);
    const prior = saved === null && identity ? localStorage.getItem(placeKey(host)) : null;
    const legacy = prior ?? (saved === null && host === null ? localStorage.getItem(LEGACY_PLACE_KEY) : null);
    const v: unknown = JSON.parse(saved ?? legacy ?? "{}");
    if (!v || typeof v !== "object") return {};
    if (legacy !== null) {
      try {
        localStorage.setItem(key, legacy);
      } catch {
        /* the legacy place can still be restored */
      }
    }
    return v as Partial<Place>;
  } catch {
    return {};
  }
}

const identityPlaceKey = (id: string) => `skiff.place.identity.${id}`;

export function trackPlace(host: string | null, identity?: string) {
  trackedPlaceKey ??= identity ? identityPlaceKey(identity) : placeKey(host);
}

export function savePlace(p: Place) {
  if (trackedPlaceKey === null) return;
  const sig = JSON.stringify(p);
  if (sig === placeSig) return;
  placeSig = sig;
  try {
    localStorage.setItem(trackedPlaceKey, sig);
  } catch {
    /* the place lasts for this run */
  }
}

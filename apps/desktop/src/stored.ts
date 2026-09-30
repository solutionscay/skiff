/** View state kept in localStorage, so it survives a restart of the app. */

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
export type Place = { project: string | null; session: string | null; group: string | null; picked: boolean };

const PLACE_KEY = "skiff.place";
/** Off until the start has restored the saved place, so the first empty draw does not overwrite it. */
let tracking = false;
let placeSig = "";

export function loadPlace(): Partial<Place> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(PLACE_KEY) ?? "{}");
    return v && typeof v === "object" ? (v as Partial<Place>) : {};
  } catch {
    return {};
  }
}

export function trackPlace() {
  tracking = true;
}

export function savePlace(p: Place) {
  if (!tracking) return;
  const sig = JSON.stringify(p);
  if (sig === placeSig) return;
  placeSig = sig;
  try {
    localStorage.setItem(PLACE_KEY, sig);
  } catch {
    /* the place lasts for this run */
  }
}

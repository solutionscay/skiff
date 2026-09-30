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

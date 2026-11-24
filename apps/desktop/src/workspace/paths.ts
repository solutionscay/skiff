/** Path text helpers. They read text only and never touch the Host. */

export function trimSlash(p: string): string {
  return p.length > 1 ? p.replace(/\/+$/, "") : p;
}

export function samePath(a: string, b: string): boolean {
  return trimSlash(a) === trimSlash(b);
}

/** The last name of a path. A trailing slash does not count. */
export function basename(p: string): string {
  const t = trimSlash(p);
  return t.slice(t.lastIndexOf("/") + 1) || t;
}

/** The folder part of a path: everything before the last `/`, or "" without one. */
export function dirname(p: string): string {
  const i = p.lastIndexOf("/");
  return i < 0 ? "" : p.slice(0, i);
}

export function splitPath(p: string): { name: string; dir: string } {
  return { name: p.slice(p.lastIndexOf("/") + 1), dir: dirname(p) };
}

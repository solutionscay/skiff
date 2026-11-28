export interface NamedItem {
  entry: number;
  host: string;
  id: string;
  name: string;
  path: string;
}

/** A Host prefix disambiguates across trees. A path or an ID disambiguates inside one tree. */
export function flatNames(items: NamedItem[]): Map<NamedItem, string> {
  const byName = new Map<string, NamedItem[]>();
  for (const item of items) {
    const peers = byName.get(item.name) ?? [];
    peers.push(item);
    byName.set(item.name, peers);
  }
  const out = new Map<NamedItem, string>();
  for (const peers of byName.values()) {
    const across = peers.some((p) => p.entry !== peers[0].entry);
    for (const item of peers) {
      const local = peers.filter((p) => p.entry === item.entry && p !== item);
      let detail = "";
      if (local.length) {
        if (item.path && local.every((p) => p.path !== item.path)) detail = item.path;
        else {
          let length = 6;
          while (length < item.id.length && local.some((p) => p.id.startsWith(item.id.slice(0, length)))) length++;
          detail = item.id.slice(0, length);
        }
      }
      out.set(item, (across ? item.host + ":" : "") + item.name + (detail ? " · " + detail : ""));
    }
  }
  return out;
}

/** Playful call signs for new agent sessions. */
const DEFAULT_RANKS = [
  "Captain", "First Mate", "Bosun", "Commodore", "Quartermaster", "Deckhand",
  "Buccaneer", "Corsair", "Skipper", "Sea Dog", "Navigator", "Privateer",
];

const DEFAULT_NOUNS = [
  "Anchovy", "Barnacle", "Blackbeard", "Cannon", "Cutlass", "Doubloon",
  "Galleon", "Jolly Roger", "Kraken", "Marlin", "Mermaid", "Parrot",
  "Pegleg", "Pelican", "Porthole", "Rumrunner", "Sextant", "Starfish",
  "Tidepool", "Tortuga", "Treasure", "Triton", "Tugboat", "Whirlpool",
];

type NameList = "ranks" | "nouns";
const DEFAULTS: Record<NameList, readonly string[]> = { ranks: DEFAULT_RANKS, nouns: DEFAULT_NOUNS };
const KEY = (l: NameList) => `skiff.agentNames.${l}`;

/** The names in use for a list: the user's own, or the defaults when there are none. */
export function agentNameList(list: NameList): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY(list)) ?? "null");
    if (Array.isArray(v)) {
      const items = v.filter((x): x is string => typeof x === "string" && x.trim() !== "");
      if (items.length) return items;
    }
  } catch {
    /* use the defaults */
  }
  return [...DEFAULTS[list]];
}

/** Saves a list. An empty list goes back to the defaults. */
export function setAgentNameList(list: NameList, names: string[]): void {
  const items = names.map((n) => n.trim()).filter(Boolean);
  try {
    if (items.length) localStorage.setItem(KEY(list), JSON.stringify(items));
    else localStorage.removeItem(KEY(list));
  } catch {
    /* the names last for this run only */
  }
}


function pick<T>(items: readonly T[]): T {
  return items[crypto.getRandomValues(new Uint32Array(1))[0] % items.length]!;
}

/** A new call sign, preferring one no live session is already using. */
export function agentCallsign(existing: Iterable<string>): string {
  const used = new Set(existing);
  const ranks = agentNameList("ranks");
  const nouns = agentNameList("nouns");
  for (let i = 0; i < 12; i++) {
    const name = `${pick(ranks)} ${pick(nouns)}`;
    if (!used.has(name)) return name;
  }
  return `${pick(ranks)} ${pick(nouns)}`;
}

/** Playful call signs for new agent sessions. */
const RANKS = [
  "Captain", "First Mate", "Bosun", "Commodore", "Quartermaster", "Deckhand",
  "Buccaneer", "Corsair", "Skipper", "Sea Dog", "Navigator", "Privateer",
];

const NAUTICAL = [
  "Anchovy", "Barnacle", "Blackbeard", "Cannon", "Cutlass", "Doubloon",
  "Galleon", "Jolly Roger", "Kraken", "Marlin", "Mermaid", "Parrot",
  "Pegleg", "Pelican", "Porthole", "Rumrunner", "Sextant", "Starfish",
  "Tidepool", "Tortuga", "Treasure", "Triton", "Tugboat", "Whirlpool",
];

function pick<T>(items: readonly T[]): T {
  return items[crypto.getRandomValues(new Uint32Array(1))[0] % items.length]!;
}

/** A new call sign, preferring one no live session is already using. */
export function agentCallsign(existing: Iterable<string>): string {
  const used = new Set(existing);
  for (let i = 0; i < 12; i++) {
    const name = `${pick(RANKS)} ${pick(NAUTICAL)}`;
    if (!used.has(name)) return name;
  }
  return `${pick(RANKS)} ${pick(NAUTICAL)}`;
}

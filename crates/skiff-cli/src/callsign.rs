//! Call signs for new agent sessions, as the app picks them
//! (`apps/desktop/src/appearance/agentNames.ts`, default lists).

use std::hash::{BuildHasher, Hasher};

const RANKS: &[&str] = &[
    "Captain", "First Mate", "Bosun", "Commodore", "Quartermaster", "Deckhand",
    "Buccaneer", "Corsair", "Skipper", "Sea Dog", "Navigator", "Privateer",
];

const NOUNS: &[&str] = &[
    "Anchovy", "Barnacle", "Blackbeard", "Cannon", "Cutlass", "Doubloon",
    "Galleon", "Jolly Roger", "Kraken", "Marlin", "Mermaid", "Parrot",
    "Pegleg", "Pelican", "Porthole", "Rumrunner", "Sextant", "Starfish",
    "Tidepool", "Tortuga", "Treasure", "Triton", "Tugboat", "Whirlpool",
];

fn pick(items: &[&'static str]) -> &'static str {
    // A fresh RandomState is seeded per call: random enough for a name.
    let n = std::collections::hash_map::RandomState::new().build_hasher().finish();
    items[(n % items.len() as u64) as usize]
}

/// A new call sign, preferring one no live session is already using.
pub fn callsign<'a>(existing: impl IntoIterator<Item = &'a str>) -> String {
    let used: std::collections::HashSet<&str> = existing.into_iter().collect();
    for _ in 0..12 {
        let name = format!("{} {}", pick(RANKS), pick(NOUNS));
        if !used.contains(name.as_str()) {
            return name;
        }
    }
    format!("{} {}", pick(RANKS), pick(NOUNS))
}

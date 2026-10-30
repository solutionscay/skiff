/**
 * A Skipper's avatar: a flat body with two eye marks, fixed by its name.
 * `seed` picks another one for Shuffle. Returns an SVG on a 32×32 grid with
 * no size, so the CSS sets it.
 *
 * The body is the element's --pc, the accent of the Skipper's theme, else --project-default,
 * so a color or theme change repaints it. Black mixed in keeps the white eyes
 * readable on a pale color.
 */
const FILL = "color-mix(in srgb, var(--pc, var(--project-default)) 72%, #000)";

/** Bodies, each with the height of its eye line. A round 3-unit stroke softens every corner. */
const BODIES: [string, number][] = [
  ["M16 5c8.5 0 11 2.5 11 11s-2.5 11-11 11S5 24.5 5 16 7.5 5 16 5z", 15], // squircle
  ["M5 26V16a11 11 0 0 1 22 0v10z", 17], // dome
  ["M5 22c0-8 2.5-14 6-14 2 0 3 2 5 2s3-2 5-2c3.5 0 6 6 6 14 0 3-1.5 5-4.5 5h-13C6.5 27 5 25 5 22z", 18], // bun
  ["M16 5l10 6v10l-10 6-10-6V11z", 16], // hex
  ["M4 16C8 8 24 8 28 16 24 24 8 24 4 16z", 15], // lemon
  ["M6 6h20v9c0 6.5-4.5 10.5-10 12.5C10.5 25.5 6 21.5 6 15z", 13], // shield
  ["M5 7h22l-4.5 19h-13z", 13], // keystone
  ["M9 7l10-2 8 7.5-1.5 10.5-10 4L5 21z", 15], // pebble
];

/** One eye at 0,0. `s` is +1 for the right eye, -1 for the left. */
const EYES: ((s: number) => string)[] = [
  () => `<circle r="2.3"/>`, // dots
  () => `<rect x="-1.6" y="-3.2" width="3.2" height="6.4" rx="1.6"/>`, // tall
  (s) => `<rect x="-1.6" y="-3.2" width="3.2" height="6.4" rx="1.6" transform="rotate(${-22 * s})"/>`, // set
  (s) => `<rect x="-1.6" y="-3.2" width="3.2" height="6.4" rx="1.6" transform="rotate(${22 * s})"/>`, // unsure
  () => `<rect x="-3" y="-1.3" width="6" height="2.6" rx="1.3"/>`, // calm
  () => `<path d="M-2.8 1.2q2.8-4 5.6 0" fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round"/>`, // glad
];

/** FNV-1a: the same name gives the same avatar on every machine. */
function hash(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return h >>> 0;
}

export function skipperAvatar(name: string, seed = 0): string {
  const h = hash(seed ? `${name}#${seed}` : name);
  const [d, y] = BODIES[h % 8];
  const eye = EYES[(h >>> 3) % 6];
  const tilt = (((h >>> 6) % 9) - 4) * 2;
  const gap = 4.5 + ((h >>> 10) % 3);
  const look = ((h >>> 12) % 3) - 1;
  const at = (s: number) => `<g transform="translate(${16 + look + s * gap} ${y})">${eye(s)}</g>`;
  return `<svg viewBox="0 0 32 32" aria-hidden="true"><g transform="rotate(${tilt} 16 16)"><path d="${d}" style="fill:${FILL};stroke:${FILL}" stroke-width="3" stroke-linejoin="round"/><g fill="#fff">${at(-1)}${at(1)}</g></g></svg>`;
}

/** The Admiral: one fixed avatar, a bicorne hat with level eyes, in the --pc color as the others are. */
export const ADMIRAL_AVATAR = `<svg viewBox="0 0 32 32" aria-hidden="true"><path d="M3 22c3-9 8-13 13-13s10 4 13 13c-4-2-8.5-3-13-3s-9 1-13 3z" style="fill:${FILL};stroke:${FILL}" stroke-width="3" stroke-linejoin="round"/><g fill="#fff"><rect x="10.4" y="14" width="3.2" height="4.6" rx="1.6"/><rect x="18.4" y="14" width="3.2" height="4.6" rx="1.6"/></g></svg>`;

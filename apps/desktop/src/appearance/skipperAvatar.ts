/**
 * A Skipper's avatar: a flat body with two eye marks, fixed by its name.
 * `seed` picks another one for Shuffle. Returns an SVG on a 32×32 grid with
 * no size, so the CSS sets it.
 *
 * The body is the element's --pc, the face's color (`avatarColor`), else
 * --project-default, so a color or theme change repaints it. Black mixed in keeps the white eyes
 * readable on a pale color. The eyes sit on a visor in the Skiff black, the
 * mascot's visor color, which follows the eye style and the face's tilt.
 *
 * The faces live: the eyes blink, and now and then the head turns (the face
 * leans, the visor and the eyes slide aside, the eyes further), in CSS
 * (styles/skippers.css). Each face carries its own rhythm as variables on
 * its root, from the same hash, so no two faces blink in step.
 *
 * A finish (Settings › Appearance › Avatar finish) dresses the same face:
 * - "plush": a toy. Three fur layers (a faint halo, long fur, a short core),
 *   a low-contrast grain and a soft contact shadow. The default.
 * - "gummy": candy. A shade over the body, a lit inner rim at the bottom, a
 *   dark inner rim at the top, a specular gloss and a soft drop shadow.
 * - "flat": the plain body. The draw functions' default: no filters.
 * The visor and the eyes stay out of the filters, so they stay crisp and the
 * blinks and turns move as before. The gradients and filters live once per
 * document (`finishDefs`), in the face's own 32-unit grid, so the finish is
 * the same fraction of the face at every size.
 */
export type Finish = "flat" | "gummy" | "plush";
export const FINISHES: readonly Finish[] = ["plush", "gummy", "flat"];

/** The finish when none is stored. */
export const DEFAULT_FINISH: Finish = "plush";

/** A stored finish, else the default. */
export const parseFinish = (v: unknown): Finish => (FINISHES.includes(v as Finish) ? (v as Finish) : DEFAULT_FINISH);

/**
 * The avatar colors: twelve vivid swatches around the hue circle, at one
 * lightness and strength, so any mix of them reads as one set on a dark and
 * on a light theme. The body mixes black into the color (FILL), so a pale
 * color comes out grey: the swatches stay out of the pale end.
 */
export const AVATAR_SWATCHES: readonly string[] = [
  "#ff4f4f", "#ff8a3d", "#ffbf2e", "#a8d930", "#3fcf7a", "#24c9b4",
  "#3cb4f5", "#4f7fff", "#8466ff", "#b35cff", "#f25fd0", "#ff5f94",
];

/** A stored avatar color: "#rrggbb" in lower case, else null. */
const parseAvatarColor = (v: unknown): string | null =>
  typeof v === "string" && /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : null;

/** Hue in degrees and saturation, 0 to 1, of "#rrggbb". */
function hueSat(hex: string): [number, number] {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min, l = (max + min) / 2;
  if (!d) return [0, 0];
  const hue = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [hue * 60, d / (1 - Math.abs(2 * l - 1))];
}

/**
 * The face's color. A stored color wins. Else a swatch: the theme's color
 * picks the nearest swatch by hue, and the Skipper's name picks that swatch
 * or one of its two neighbours, so the faces in one theme differ but stay in
 * its color family. A grey theme color (saturation under 0.25) has no hue:
 * the name picks from all twelve.
 */
export function avatarColor(name: string, stored: string | null, themeColor: string | null): string {
  const own = parseAvatarColor(stored);
  if (own) return own;
  const h = hash(name);
  const theme = parseAvatarColor(themeColor);
  const [hue, sat] = theme ? hueSat(theme) : [0, 0];
  const n = AVATAR_SWATCHES.length;
  if (sat < 0.25) return AVATAR_SWATCHES[h % n];
  const gap = (c: string) => { const d = Math.abs(hueSat(c)[0] - hue) % 360; return Math.min(d, 360 - d); };
  const near = AVATAR_SWATCHES.reduce((best, c, i) => (gap(c) < gap(AVATAR_SWATCHES[best]) ? i : best), 0);
  return AVATAR_SWATCHES[(near + (h % 3) - 1 + n) % n];
}

const FILL = "color-mix(in srgb, var(--pc, var(--project-default)) 85%, #000)";
const VISOR = "#192230";

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

/** One eye at 0,0, `s` +1 for the right eye and -1 for the left, with the eye's width, height and middle for its visor. */
const EYES: [(s: number) => string, number, number, number][] = [
  [() => `<circle r="2.3"/>`, 4.6, 4.6, 0], // dots
  [() => `<rect x="-1.6" y="-3.2" width="3.2" height="6.4" rx="1.6"/>`, 3.2, 6.4, 0], // tall
  [(s) => `<rect x="-1.6" y="-3.2" width="3.2" height="6.4" rx="1.6" transform="rotate(${-22 * s})"/>`, 5.4, 7.1, 0], // set
  [(s) => `<rect x="-1.6" y="-3.2" width="3.2" height="6.4" rx="1.6" transform="rotate(${22 * s})"/>`, 5.4, 7.1, 0], // unsure
  [() => `<rect x="-3" y="-1.3" width="6" height="2.6" rx="1.3"/>`, 6, 2.6, 0], // calm
  [() => `<path d="M-2.8 1.2q2.8-4 5.6 0" fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round"/>`, 8, 4.4, 0.2], // glad
];

/** The visor behind two eyes `gap` from `x`, on the line `y`: 1.6 around the eyes, round at the ends. */
function visor(x: number, y: number, gap: number, [, w, ht, mid]: (typeof EYES)[number], finish: Finish): string {
  const vw = Math.min(2 * gap + w + 3.2, 21), vh = ht + 3.2, r = Math.min(vh / 2, 3.5);
  const at = `x="${x - vw / 2}" y="${y + mid - vh / 2}" width="${vw}" height="${vh}" rx="${r}"`;
  return `<rect ${at} fill="${VISOR}"/>` + visorLight(at, finish);
}

/** Gummy and plush: a soft light on the top of the visor, so it is a part of the toy, not a sticker. */
const visorLight = (at: string, finish: Finish) => (finish === "flat" ? "" : `<rect ${at} fill="url(#sk-visor)"/>`);

/**
 * The body. Gummy and plush add an opaque shade over it, so the fill and the
 * 3-unit stroke, which overlap inside the edge, paint once, and put both in
 * the finish's filter. Gummy blends its grey shade hard-light. Plush blends
 * overlay: the body's own color leads, so a light top-left and a dark rim
 * shape the toy and the color keeps its strength. WebKitGTK blends in linear
 * light, where the neutral grey is #bcbcbc, not #808080 (#808080 darkens by
 * a sixth), so the plush shade centres on #bcbcbc.
 */
function body(shape: string, finish: Finish): string {
  const flat = `<path d="${shape}" style="fill:${FILL};stroke:${FILL}" stroke-width="3" stroke-linejoin="round"/>`;
  if (finish === "flat") return flat;
  const g = `url(#sk-shade-${finish})`;
  return `<g filter="url(#sk-${finish})">${flat}<path d="${shape}" fill="${g}" stroke="${g}" stroke-width="3" stroke-linejoin="round" style="mix-blend-mode:${finish === "plush" ? "overlay" : "hard-light"}"/></g>`;
}

/** FNV-1a: the same name gives the same avatar on every machine. */
function hash(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return h >>> 0;
}

/**
 * The face's rhythm: a blink every 4 to 7 s, a double blink for one face in
 * five, and a head turn every 10 to 18 s. The turns go one way, the other
 * way, then up, so a cycle holds three; `--dir` picks the first side. Each
 * at its own phase.
 */
function rhythm(h: number): string {
  const blink = 4 + ((h >>> 16) % 31) / 10;
  const turn = 3 * (10 + ((h >>> 21) % 9));
  // The delays count from the page clock, not from when this face was drawn: a
  // row drawn again (the Admiral's, on every state change) picks up its
  // rhythm where it was, instead of starting over and never reaching a turn.
  const now = performance.now() / 1000;
  const phase = (period: number, offset: number) => (-((now + offset) % period)).toFixed(2);
  return `--blink:${blink}s;--blink-delay:${phase(blink, ((h >>> 4) % 70) / 10)}s;--blink-anim:${(h >>> 25) % 5 ? "sk-blink" : "sk-blink2"};`
    + `--face-turn:${turn}s;--face-turn-delay:${phase(turn, (h >>> 9) % turn)}s;--dir:${(h >>> 29) & 1 ? 1 : -1}`;
}

/** Animation phase for a face mounted now, including a cached face. */
export const avatarStyle = (name: string, seed = 0) => rhythm(hash(seed ? `${name}#${seed}` : name));

export function skipperAvatar(name: string, seed = 0, finish: Finish = "flat"): string {
  const h = hash(seed ? `${name}#${seed}` : name);
  const [d, y] = BODIES[h % 8];
  const eye = EYES[(h >>> 3) % 6];
  const [draw] = eye;
  const tilt = (((h >>> 6) % 9) - 4) * 2;
  const gap = 4.5 + ((h >>> 10) % 3);
  const look = ((h >>> 12) % 3) - 1;
  const at = (s: number) => `<g transform="translate(${16 + look + s * gap} ${y})">${draw(s)}</g>`;
  return `<svg viewBox="0 0 32 32" aria-hidden="true"${over(finish)} style="${rhythm(h)}"><g class="sk-turn"><g transform="rotate(${tilt} 16 16)">${body(d, finish)}<g class="sk-front">${visor(16 + look, y, gap, eye, finish)}<g class="sk-look"><g class="sk-eyes" fill="#fff">${at(-1)}${at(1)}</g></g></g></g></g></svg>`;
}

/**
 * The Admiral: the Skiff mascot's head, filling the grid as the crew's bodies
 * do. A wide rounded-square white head; the Skiff-black visor across most of
 * its front, with two tall white eyes set wide, which blink and turn as the
 * crew's do; a sailor cap sitting low and wide: a white crown and a band in
 * the --pc color with a gold button, straight on the head, as on the mascot. A thin Skiff-black
 * edge keeps the white on a light theme.
 */
const EDGE = `stroke="${VISOR}" stroke-width="1.6" stroke-linejoin="round"`;
export function admiralAvatar(finish: Finish = "flat"): string {
  // Gummy keeps the ink edge, inside the filter. Plush drops it: there the ink
  // frays into dirt, and the shade's grey rim holds the white on a light theme.
  const edge = finish === "plush" ? "" : EDGE;
  const shade = (el: string) => (finish === "flat" ? "" : el.replace('fill="#fff"', 'fill="url(#sk-shade-white)"').replace(edge, ""));
  const wrap = (s: string) => (finish === "flat" ? s : `<g filter="url(#sk-${finish})">${s}${shade(s)}</g>`);
  const head = `<rect x="2.4" y="10.6" width="27.2" height="19.4" rx="8" fill="#fff" ${edge}/>`;
  const crown = `<path d="M4.6 9.4C5.4 4.6 10 2 16 2s10.6 2.6 11.4 7.4z" fill="#fff" ${edge}/>`;
  const visorAt = `x="5.6" y="15.2" width="20.8" height="11.6" rx="4.8"`;
  return `<svg viewBox="0 0 32 32" aria-hidden="true"${over(finish)} style="${rhythm(hash("Admiral"))}"><g class="sk-turn">`
    + wrap(head)
    // The cap's crown and band stay back in a turn; the front, on one centerline,
    // moves as one: the button, the visor and the eyes. No brim, as on the mascot.
    + `<g class="sk-cap">${wrap(crown)}${capBand(finish)}</g>`
    + `<g class="sk-front">${capButton(finish)}`
    + `<rect ${visorAt} fill="${VISOR}"/>${visorLight(visorAt, finish)}`
    + `<g class="sk-look"><g class="sk-eyes" fill="#fff"><rect x="9.2" y="17.6" width="3.4" height="6.6" rx="1.7"/><rect x="19.4" y="17.6" width="3.4" height="6.6" rx="1.7"/></g></g></g></g></svg>`;
}

/**
 * The cap's band and gold button. Plush: the band in a short felt pile with a
 * top-lit shade, its theme color lifted back after the pile dulls it, and
 * the button a plush gold ball with a stitched ring. The pile frays the ink
 * edge into a thick rim, so it is thinner there. Other finishes: flat.
 */
const BAND_AT = `x="4.2" y="8" width="23.6" height="4.4" rx="1.8"`;
const PILE_EDGE = `stroke="${VISOR}" stroke-width=".9" stroke-linejoin="round"`;
const capBand = (finish: Finish) =>
  finish !== "plush"
    ? `<rect ${BAND_AT} style="fill:${FILL}" ${EDGE}/>`
    : `<g filter="url(#sk-band-lift)"><g filter="url(#sk-pile)"><rect ${BAND_AT} style="fill:${FILL}" ${PILE_EDGE}/>`
      + `<rect ${BAND_AT} fill="url(#sk-band-shade)" stroke="url(#sk-band-shade)" stroke-width=".9" stroke-linejoin="round" style="mix-blend-mode:hard-light"/></g></g>`;
const capButton = (finish: Finish) =>
  finish !== "plush"
    ? `<circle cx="16" cy="8.2" r="1.9" fill="#f4c430" ${EDGE}/>`
    : `<g filter="url(#sk-pile)"><circle cx="16" cy="8.2" r="1.9" fill="url(#sk-button)" ${PILE_EDGE}/></g>`
      + `<circle cx="16" cy="8.2" r="1.25" fill="none" stroke="#8a5a00" stroke-width=".2" stroke-dasharray=".45 .3" opacity=".75"/>`;

/** A finish's shadow and fur reach past the 32-unit grid. */
const over = (finish: Finish) => (finish === "flat" ? "" : ` overflow="visible"`);

/** The gradients and filters of the finishes, once per document, in a hidden SVG. */
export const FINISH_DEFS_ID = "sk-finish-defs";
export const finishDefs = () => `<svg id="${FINISH_DEFS_ID}" width="0" height="0" style="position:absolute" aria-hidden="true" focusable="false"><defs>
<radialGradient id="sk-shade-white" cx=".36" cy=".28" r=".8">
  <stop offset="0" stop-color="#fff"/><stop offset=".55" stop-color="#eef0f4"/><stop offset="1" stop-color="#aeb6c4"/>
</radialGradient>
<linearGradient id="sk-visor" x1="0" y1="0" x2="0" y2="1">
  <stop offset="0" stop-color="#fff" stop-opacity=".16"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/>
</linearGradient>
<radialGradient id="sk-shade-gummy" gradientUnits="userSpaceOnUse" cx="12" cy="10" r="19">
  <stop offset="0" stop-color="#a8a8a8"/><stop offset=".35" stop-color="#868686"/>
  <stop offset=".7" stop-color="#767676"/><stop offset="1" stop-color="#3a3a3a"/>
</radialGradient>
<!-- WebKitGTK blends these in linear light: #bcbcbc, not #808080, changes nothing. Plush (overlay): lit top-left over it, neutral through the middle, a dark rim. -->
<radialGradient id="sk-shade-plush" gradientUnits="userSpaceOnUse" cx="13.5" cy="11.5" r="17.5">
  <stop offset="0" stop-color="#ffffff"/><stop offset=".3" stop-color="#d8d8d8"/><stop offset=".55" stop-color="#bcbcbc"/>
  <stop offset=".8" stop-color="#8c8c8c"/><stop offset="1" stop-color="#4a4a4a"/>
</radialGradient>
<filter id="sk-gummy" x="-25%" y="-25%" width="150%" height="160%" color-interpolation-filters="sRGB">
  <feGaussianBlur in="SourceAlpha" stdDeviation="1.6" result="soft"/>
  <feOffset in="soft" dy="-1.8" result="up"/>
  <feComposite in="SourceAlpha" in2="up" operator="out" result="rim0"/>
  <feGaussianBlur in="rim0" stdDeviation=".7" result="rim1"/>
  <feComposite in="rim1" in2="SourceAlpha" operator="in" result="rim"/>
  <feFlood flood-color="#fff" flood-opacity=".38"/>
  <feComposite in2="rim" operator="in" result="glow"/>
  <feOffset in="soft" dy="1.4" result="down"/>
  <feComposite in="SourceAlpha" in2="down" operator="out" result="top0"/>
  <feGaussianBlur in="top0" stdDeviation=".7" result="top1"/>
  <feComposite in="top1" in2="SourceAlpha" operator="in" result="toprim"/>
  <feFlood flood-color="#000" flood-opacity=".3"/>
  <feComposite in2="toprim" operator="in" result="dark"/>
  <feGaussianBlur in="SourceAlpha" stdDeviation="3.2" result="bump"/>
  <feSpecularLighting in="bump" surfaceScale="2.4" specularConstant=".75" specularExponent="34" lighting-color="#fff" result="spec">
    <fePointLight x="10" y="4" z="22"/>
  </feSpecularLighting>
  <feGaussianBlur in="spec" stdDeviation=".5" result="spec2"/>
  <feComposite in="spec2" in2="SourceAlpha" operator="in" result="gloss"/>
  <feMerge result="body"><feMergeNode in="SourceGraphic"/><feMergeNode in="dark"/><feMergeNode in="glow"/><feMergeNode in="gloss"/></feMerge>
  <feGaussianBlur in="SourceAlpha" stdDeviation="1.4" result="sb"/>
  <feOffset in="sb" dy="1.8" result="so"/>
  <feFlood flood-color="#000" flood-opacity=".32"/>
  <feComposite in2="so" operator="in" result="shadow"/>
  <feMerge><feMergeNode in="shadow"/><feMergeNode in="body"/></feMerge>
</filter>
<filter id="sk-plush" x="-30%" y="-30%" width="160%" height="160%" color-interpolation-filters="sRGB">
  <feTurbulence type="fractalNoise" baseFrequency="1.15" numOctaves="4" seed="11" result="noise"/>
  <feDisplacementMap in="SourceGraphic" in2="noise" scale="6" xChannelSelector="R" yChannelSelector="G" result="far"/>
  <feGaussianBlur in="far" stdDeviation=".8" result="far2"/>
  <feComponentTransfer in="far2" result="fluff1"><feFuncA type="linear" slope=".55"/></feComponentTransfer>
  <feDisplacementMap in="SourceGraphic" in2="noise" scale="4.2" xChannelSelector="G" yChannelSelector="R" result="long"/>
  <feGaussianBlur in="long" stdDeviation=".35" result="fluff2"/>
  <feDisplacementMap in="SourceGraphic" in2="noise" scale="2" xChannelSelector="R" yChannelSelector="G" result="core"/>
  <feGaussianBlur in="core" stdDeviation=".15" result="core2"/>
  <feMerge result="fluff"><feMergeNode in="fluff1"/><feMergeNode in="fluff2"/><feMergeNode in="core2"/></feMerge>
  <feTurbulence type="fractalNoise" baseFrequency="1.3 .9" numOctaves="2" seed="5" result="grain"/>
  <feColorMatrix in="grain" type="saturate" values="0" result="gg"/>
  <feGaussianBlur in="gg" stdDeviation=".2" result="gb"/>
  <feComponentTransfer in="gb" result="g"><feFuncR type="linear" slope=".55" intercept=".22"/><feFuncG type="linear" slope=".55" intercept=".22"/><feFuncB type="linear" slope=".55" intercept=".22"/></feComponentTransfer>
  <!-- The grain on top of the color: in2 is the backdrop. The other way the grey grain is the base and the color only tints it. -->
  <feBlend in="g" in2="fluff" mode="soft-light" result="furred"/>
  <feComposite in="furred" in2="fluff" operator="in" result="toy"/>
  <feGaussianBlur in="SourceAlpha" stdDeviation="1.8" result="sb"/>
  <feOffset in="sb" dy="1.8" result="so"/>
  <feFlood flood-color="#000" flood-opacity=".22"/>
  <feComposite in2="so" operator="in" result="shadow"/>
  <feMerge><feMergeNode in="shadow"/><feMergeNode in="toy"/></feMerge>
</filter>
<filter id="sk-pile" x="-60%" y="-80%" width="220%" height="260%" color-interpolation-filters="sRGB">
  <feTurbulence type="fractalNoise" baseFrequency="1.6" numOctaves="4" seed="31" result="noise"/>
  <feDisplacementMap in="SourceGraphic" in2="noise" scale="1.5" xChannelSelector="R" yChannelSelector="G" result="far"/>
  <feGaussianBlur in="far" stdDeviation=".3" result="far2"/>
  <feComponentTransfer in="far2" result="fluff1"><feFuncA type="linear" slope=".55"/></feComponentTransfer>
  <feDisplacementMap in="SourceGraphic" in2="noise" scale=".7" xChannelSelector="G" yChannelSelector="R" result="core"/>
  <feGaussianBlur in="core" stdDeviation=".08" result="core2"/>
  <feMerge result="fluff"><feMergeNode in="fluff1"/><feMergeNode in="core2"/></feMerge>
  <feTurbulence type="fractalNoise" baseFrequency="2.2 1.4" numOctaves="2" seed="9" result="grain"/>
  <feColorMatrix in="grain" type="saturate" values="0" result="gg"/>
  <feGaussianBlur in="gg" stdDeviation=".12" result="gb"/>
  <feComponentTransfer in="gb" result="g"><feFuncR type="linear" slope=".6" intercept=".2"/><feFuncG type="linear" slope=".6" intercept=".2"/><feFuncB type="linear" slope=".6" intercept=".2"/></feComponentTransfer>
  <feBlend in="g" in2="fluff" mode="soft-light" result="furred"/>
  <feComposite in="furred" in2="fluff" operator="in" result="toy"/>
  <feGaussianBlur in="SourceAlpha" stdDeviation=".6" result="sb"/>
  <feOffset in="sb" dy=".6" result="so"/>
  <feFlood flood-color="#000" flood-opacity=".28"/>
  <feComposite in2="so" operator="in" result="shadow"/>
  <feMerge><feMergeNode in="shadow"/><feMergeNode in="toy"/></feMerge>
</filter>
<filter id="sk-band-lift" x="-20%" y="-60%" width="140%" height="220%" color-interpolation-filters="sRGB">
  <feColorMatrix type="saturate" values="1.3"/>
  <feComponentTransfer><feFuncR type="linear" slope="1.3"/><feFuncG type="linear" slope="1.3"/><feFuncB type="linear" slope="1.3"/></feComponentTransfer>
</filter>
<linearGradient id="sk-band-shade" gradientUnits="userSpaceOnUse" x1="0" y1="7.2" x2="0" y2="13.2">
  <stop offset="0" stop-color="#c4c4c4"/><stop offset=".5" stop-color="#8a8a8a"/><stop offset="1" stop-color="#6a6a6a"/>
</linearGradient>
<radialGradient id="sk-button" gradientUnits="userSpaceOnUse" cx="15.3" cy="7.4" r="2.6">
  <stop offset="0" stop-color="#fff2b0"/><stop offset=".45" stop-color="#f4c430"/><stop offset="1" stop-color="#b8860b"/>
</radialGradient>
</defs></svg>`;

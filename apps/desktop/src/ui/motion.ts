/**
 * Shared motion values. The durations and the curve live in styles/tokens.css.
 * Motion is for light layers only, with opacity and transform. A terminal, a
 * pane or a box that holds one never animates: it shows and goes at once.
 */

export const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Read once: a read forces a style pass, and an animation starts in the middle of DOM work. */
const read = new Map<string, string>();

function token(name: string): string {
  let value = read.get(name);
  if (!value) {
    value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    if (value) read.set(name, value);
  }
  return value ?? "";
}

/** The timing of a fade: a surface or a row comes or goes. */
export const fade = (): KeyframeAnimationOptions => ({ duration: parseFloat(token("--motion-fade")), easing: token("--motion-ease") });
/** The timing of a move: a row goes to its new place. */
export const move = (): KeyframeAnimationOptions => ({ duration: parseFloat(token("--motion-move")), easing: token("--motion-ease") });

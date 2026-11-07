/** When a session that waits gets a desktop notification: once per wait, and
 *  only when its pane does not have the keys. No imports: the page tests load
 *  this file as it is. */

/** A stop of the wait shorter than this is part of the same wait. A resize
 *  makes an agent clear its screen and draw its prompt again: for a moment
 *  skiffd sees no prompt and moves the session out of waiting. */
export const SAME_WAIT_MS = 2000;

/** One wait of one session. `left` is when it went out of waiting, `null` while it waits. */
type Wait = { left: number | null };

export class Waits {
  private waits = new Map<string, Wait>();

  /** The session moved to `state` at `now`. `keys` is true when its pane has
   *  the keys. True when this starts a new wait out of sight: notify. */
  moved(id: string, state: string, now: number, keys: boolean): boolean {
    const w = this.waits.get(id);
    if (state !== "waiting") {
      if (w && w.left === null) w.left = now;
      return false;
    }
    if (w && (w.left === null || now - w.left < SAME_WAIT_MS)) {
      w.left = null;
      return false;
    }
    this.waits.set(id, { left: null });
    return !keys;
  }

  /** The session is gone. */
  forget(id: string) {
    this.waits.delete(id);
  }
}

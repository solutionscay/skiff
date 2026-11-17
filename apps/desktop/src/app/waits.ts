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

/** The number on the app icon (#275): the top bar's Waiting count. A rise
 *  shows at once. A drop shows after `SAME_WAIT_MS`, so a resize does not
 *  flicker it. Without a link to the Host, or before its session list is
 *  read, the icon has no number: no count is better than a stale count. */
export class BadgeCount {
  /** The number on the icon; `null` before the first send (a reload or a start). */
  private shown: number | null = null;
  private want = 0;
  /** When `want` last dropped. */
  private since = 0;
  /** The session list was read on a live link. */
  private live = false;

  /** The top bar's count at `now`. Ignored until the session list is read. */
  count(n: number, now: number) {
    if (!this.live) return;
    if (n < this.want) this.since = now;
    this.want = n;
  }

  /** The link to the Host. Down: no number. Up with a fresh list: `n`. */
  link(up: boolean, n = 0, now = 0) {
    this.live = up;
    this.want = up ? n : 0;
    this.since = now - SAME_WAIT_MS;
  }

  /** What to send at `now`: the number (0 clears), or `null` for no change;
   *  `wait` is when to ask again, in ms, or `null`. */
  step(now: number): { send: number | null; wait: number | null } {
    let target = this.want;
    let wait: number | null = null;
    const held = now - this.since;
    if (this.live && this.shown !== null && target < this.shown && held < SAME_WAIT_MS) {
      target = this.shown;
      wait = SAME_WAIT_MS - held;
    }
    if (target === this.shown) return { send: null, wait };
    this.shown = target;
    return { send: target, wait };
  }
}

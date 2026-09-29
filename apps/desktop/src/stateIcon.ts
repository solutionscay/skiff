/** The mark for a session's state: skiff under way, anchor, bell, or a finished flag. */
import { h, icon } from "./dom";
import type { SessionInfo } from "./types";

const SKIFF = '<path d="M2 15h20l-4 5H6z"></path><path d="M12 15V3l7 10h-7"></path>';
const ANCHOR = '<circle cx="12" cy="5" r="3"></circle><path d="M12 8v14"></path><path d="M5 12H2a10 10 0 0 0 20 0h-3"></path>';
const BELL = '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"></path><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"></path>';
const OK = '<circle cx="12" cy="12" r="10"></circle><path d="M8 12l3 3 5-6"></path>';
const FAILED = '<circle cx="12" cy="12" r="10"></circle><path d="M9 9l6 6M15 9l-6 6"></path>';

export function stateIcon(s: SessionInfo): HTMLElement {
  const mark = (cls: string, title: string, paths: string) => {
    const el = h("span", cls);
    el.title = title;
    el.appendChild(icon(paths));
    return el;
  };
  switch (s.state) {
    case "working":
      // A skiff under way: it rocks while the water runs past.
      return mark("busy", "Working", SKIFF);
    case "idle":
      // At anchor: the skiff is not moving.
      return mark("anchored", "Idle", ANCHOR);
    case "waiting":
      // The bell rang: the process wants you.
      return mark("belled", "Needs attention", BELL);
    case "done":
      return s.exit_code && s.exit_code !== 0
        ? mark("finished failed", `Exited with code ${s.exit_code}`, FAILED)
        : mark("finished", "Exited", OK);
  }
}

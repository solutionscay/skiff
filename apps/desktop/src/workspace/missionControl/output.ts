/**
 * The windows' output: the last screen lines of Admiral and each Skipper,
 * read from skiffd as the panel reads the lit card's screen
 * (`session_screen`). One round a second at most, for the windows in view
 * only, each window's own rows. The Host API reads one session a call, so a
 * round is one call per window in view, all at once; a round never starts
 * while the last one runs. The lit window takes its lines from the panel's
 * read instead (`feed`). The board's timer runs the rounds, so a hidden
 * board reads nothing.
 */
import { hostInvoke } from "../../app/hostCaps";
import * as G from "../missionLayout";

/** Each window's output column and its rows, by session. */
const outs = new Map<string, { el: HTMLElement; rows: number }>();
let busy = false;

/** The output column of `id`'s window, with the rows it shows. */
export function track(id: string, el: HTMLElement, rows: number) {
  outs.set(id, { el, rows });
}

/** Forgets every window but `keep`. */
export function untrack(keep: Set<string>) {
  for (const id of outs.keys()) if (!keep.has(id)) outs.delete(id);
}

/** Lines read for `id` elsewhere, as the panel's 12: the window shows the last of them. */
export function feed(id: string, lines: string[]) {
  const o = outs.get(id);
  if (!o) return;
  const text = lines.slice(-o.rows).join("\n");
  if (o.el.textContent !== text) o.el.textContent = text;
}

/**
 * One round: the windows that show at all in `view`, less `skip` (the lit
 * one, which the panel reads). `view` null reads every window, for the first
 * draw before the board shows. `alive`: the board still wants the lines.
 */
export async function readRound(placed: G.Placed, view: G.View | null, vw: number, vh: number, skip: string | null, alive: () => boolean) {
  if (busy) return;
  const ids = [...outs.keys()].filter((id) => {
    const r = placed.cards.get(id);
    return id !== skip && !!r && (!view || !G.edgeTick(view, r, vw, vh));
  });
  if (!ids.length) return;
  busy = true;
  try {
    await Promise.all(
      ids.map(async (id) => {
        const rows = outs.get(id)?.rows ?? 0;
        const lines = await hostInvoke<string[]>("session_screen", "session_screen", { session: id, rows }).catch(() => null);
        if (lines && alive()) feed(id, lines);
      }),
    );
  } finally {
    busy = false;
  }
}

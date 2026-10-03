/**
 * What a sidebar row does, by row key. Rows declare it when they render; the
 * keys and the preview controller read it. A row starts nothing by itself.
 */

/** What the main area shows while the row is current. */
export type Preview =
  | { kind: "diff"; wt: string; branch: string; file?: string }
  | { kind: "file"; path: string }
  | { kind: "folder"; path: string };

export interface RowActs {
  preview?: Preview;
  /** Space, Left and Right: open or close the row. */
  fold?: (open?: boolean) => void;
  /** Shift+Enter and double-click: open the file itself. */
  open?: () => void;
}

const acts = new Map<string, RowActs>();

export const setRowActs = (key: string, a: RowActs) => void acts.set(key, a);
export const rowActs = (key: string | null | undefined): RowActs | undefined => (key ? acts.get(key) : undefined);

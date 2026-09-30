

import { leaf, sessionsOf } from "./layout";



import type { Layout, SplitDir } from "../platform/types";

export const isSlot = (id: string) => id.startsWith("slot:");

export const slot = () => leaf(`slot:${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`);
const sp = (dir: SplitDir, a: Layout, b: Layout, ratio = 0.5): Layout => ({ type: "split", dir, ratio, a, b });

/** The layouts the + menu offers. Four panes at most, as any split. */
export const PRESETS: { name: string; make: () => Layout }[] = [
  { name: "2 side by side", make: () => sp("row", slot(), slot()) },
  { name: "2 stacked", make: () => sp("col", slot(), slot()) },
  { name: "1 left, 2 right", make: () => sp("row", slot(), sp("col", slot(), slot())) },
  { name: "1 over 2", make: () => sp("col", slot(), sp("row", slot(), slot())) },
  { name: "3 side by side", make: () => sp("row", slot(), sp("row", slot(), slot()), 1 / 3) },
  { name: "2 by 2", make: () => sp("col", sp("row", slot(), slot()), sp("row", slot(), slot())) },
];

export const slotsOf = (l: Layout | null) => sessionsOf(l).filter(isSlot);
export const filledOf = (l: Layout | null) => sessionsOf(l).filter((id) => !isSlot(id));

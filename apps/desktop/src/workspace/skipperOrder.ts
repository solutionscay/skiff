/**
 * The Skippers list's own order. It lives in `[skiff_agent] skippers_order`, by
 * scope, and the tree's order does not read it. A row with no stored place comes
 * after the stored rows, in tree order. A stored scope with no row is kept and
 * skipped. Ctrl+Shift+1..9 and Mission control's numbers count down this order.
 */
import type { Project, SessionInfo } from "../platform/types";
import { fleetSkipperOf, projectSkipperOf, skiffAgentFolder, skiffAgentName, skiffAgentSession } from "../app/skiffAgent";
import { hostInvoke } from "../app/hostCaps";
import { render } from "../app/renderRequest";
import { S } from "../app/state";
import { showError } from "../ui/alerts";
import { treeSections } from "./tree";

/** One Skipper in the list. `s` is null only for the Skiff agent with no session. */
export type Entry = { kind: "admiral" | "fleet" | "project"; s: SessionInfo | null; name: string; scope: string | null; project: Project | null };

/** `scopes` in tree order, set in the `stored` order. A scope not stored goes last, in tree order. */
export function arrangeScopes(scopes: string[], stored: string[]): string[] {
  const shown = new Set(scopes);
  const placed = new Set(stored);
  return [...new Set(stored)].filter((s) => shown.has(s)).concat(scopes.filter((s) => !placed.has(s)));
}

/**
 * What to store after a drop: `moving` next to `anchor`. Each scope on screen gets
 * a stored place, so a later change in the tree does not move it. A stored scope
 * with no row keeps its place among the others.
 */
export function movedScopes(scopes: string[], stored: string[], moving: string, anchor: string, side: "before" | "after"): string[] {
  const all = [...new Set(stored)];
  all.push(...scopes.filter((s) => !all.includes(s)));
  const out = all.filter((s) => s !== moving);
  out.splice(out.indexOf(anchor) + (side === "after" ? 1 : 0), 0, moving);
  return out;
}

const storedOrder = () => S.skiffAgent?.skippers_order ?? [];

/** The Skiff agent first, then each fleet Skipper and project Skipper in the list's order. */
export function skipperEntries(): Entry[] {
  const rows = new Map<string, Entry>();
  for (const sec of treeSections()) {
    const fs = sec.fleet ? fleetSkipperOf(sec.fleet) : null;
    if (fs && sec.fleet) rows.set(`fleet:${sec.fleet}`, { kind: "fleet", s: fs, name: sec.fleet, scope: `fleet:${sec.fleet}`, project: null });
    for (const p of sec.projects) {
      const ps = projectSkipperOf(p.name);
      if (ps) rows.set(p.name, { kind: "project", s: ps, name: p.name, scope: p.name, project: p });
    }
  }
  const admiral: Entry[] = skiffAgentFolder() ? [{ kind: "admiral", s: skiffAgentSession(), name: skiffAgentName(), scope: null, project: null }] : [];
  return admiral.concat(arrangeScopes([...rows.keys()], storedOrder()).map((scope) => rows.get(scope)!));
}

/** The fleet and project Skippers with a number key, in list order: Ctrl+Shift+1 opens the first. */
export const numberedSkippers = (all = skipperEntries()) => all.filter((e) => e.kind !== "admiral").slice(0, 9);

/** A drop: the row moves at once. A failed write puts it back and shows why. */
export async function moveSkipper(moving: string, anchor: string, side: "before" | "after") {
  const agent = S.skiffAgent;
  if (!agent) return;
  const before = agent.skippers_order;
  const scopes = skipperEntries().flatMap((e) => (e.scope ? [e.scope] : []));
  const value = movedScopes(scopes, storedOrder(), moving, anchor, side);
  agent.skippers_order = value;
  render("skippers-order");
  try {
    await hostInvoke("skipper_config", "set_skiff_agent", { edit: { key: "skippers_order", value } });
  } catch (e) {
    agent.skippers_order = before;
    showError(String(e));
    render("skippers-order");
  }
}

/**
 * What an end or kill confirm adds when it ends a Skipper: the Skiff agent, a
 * project Skipper or a fleet Skipper. One session gets its face in the title
 * and a title that names it as a Skipper. A confirm for many sessions lists
 * each Skipper among them on its own line. Both count the open Ledger tasks
 * the Skipper owns.
 */
import { coordinatorOf, skiffAgentName, skipperWord } from "../app/skiffAgent";
import { sessions } from "../app/state";
import { h } from "../ui/dom";
import { t, tn } from "../i18n";
import { ledgerRows } from "./ledgerStore";
import { agentName, scopeFleet } from "./model";
import { skipperFace } from "./skipperFace";

/** The Skipper's scope, else null: "grand", a project's name, or "fleet:<fleet>". */
const scopeOf = (id: string) => coordinatorOf(id);

/** Open Ledger tasks and hand-offs the scope owns: not done, in work that is not closed. */
export function openTasks(scope: string): number {
  return ledgerRows().filter((r) => r.kind !== "plan" && r.owner === scope && r.state !== "done" && !r.closed_at && r.bucket !== "finished").length;
}

/** "the skiff Skipper", "the Harbor fleet Skipper", "Skiff, the Skiff agent". */
function skipperNamed(scope: string): string {
  if (scope === "grand") return t("endSkipper.agent", { name: skiffAgentName() });
  const fleet = scopeFleet(scope);
  return fleet ? t("endSkipper.fleet", { name: fleet, word: skipperWord() }) : t("endSkipper.project", { name: scope, word: skipperWord() });
}

const faceScope = (scope: string) => (scope === "grand" ? null : scope);

/** The End confirm for one session that is a Skipper, else null. */
export function skipperEndConfirm(id: string): { title: string; body: string; action: string; face: HTMLElement } | null {
  const scope = scopeOf(id);
  const s = sessions.get(id);
  if (!scope || !s) return null;
  const n = openTasks(scope);
  const body = t("endSkipper.body", { agent: agentName(s) });
  return {
    title: t("endSkipper.title", { name: skipperNamed(scope) }),
    body: n ? `${body}\n${tn("endSkipper.tasks", n)}` : body,
    action: t(scope === "grand" ? "endSkipper.actionAgent" : "endSkipper.action"),
    face: skipperFace(faceScope(scope), 32),
  };
}

/** One line for each Skipper in `ids`, with its face, for a confirm that ends many sessions. Empty when none is a Skipper. */
export function skipperEndLines(ids: string[]): HTMLElement[] {
  const rows: HTMLElement[] = [];
  for (const id of ids) {
    const scope = scopeOf(id);
    if (!scope || !sessions.has(id)) continue;
    const n = openTasks(scope);
    const text = t("endSkipper.line", { name: skipperNamed(scope) });
    const row = h("div", "confirm-skipper");
    row.append(skipperFace(faceScope(scope), 20), h("span", "", n ? `${text} ${tn("endSkipper.tasks", n)}` : text));
    rows.push(row);
  }
  if (!rows.length) return [];
  const box = h("div", "confirm-skippers");
  box.append(...rows);
  return [box];
}

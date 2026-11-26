import { allRoutes, pageRoute, withinHost, type HostRoute } from "../platform/invoke";
import { flatNames, type NamedItem } from "../platform/hostNames";
import { hostPrefix } from "./hosts";
import { S, sessions } from "./state";
import { branchName, taskTitle } from "../workspace/model";

type Kind = "session" | "group" | "worktree" | "project";
export function acrossTrees<T>(fn: (route: HostRoute) => T[]): T[] {
  return allRoutes()
    .filter((r) => r.host)
    .flatMap((r) => withinHost(r, () => fn(r)));
}

export function namesFor(kind: Kind) {
  const items = acrossTrees<NamedItem>((r) => {
    const here = { entry: r.entry, host: hostPrefix(r.endpoint) };
    if (kind === "session")
      return [...sessions.values()].map((s) => ({
        ...here,
        id: s.id,
        name: s.coordinator ? S.skiffAgent?.name || "Admiral" : taskTitle(s),
        path: s.cwd,
      }));
    if (kind === "group") return S.groups.map((g) => ({ ...here, id: g.id, name: g.name, path: g.cwd ?? "" }));
    if (kind === "project")
      return [...S.projects, ...S.closedProjects].map((p) => ({ ...here, id: p.path, name: p.name, path: p.path }));
    return S.projects.flatMap((p) => p.worktrees.map((w) => ({ ...here, id: w.path, name: branchName(w), path: p.path })));
  });
  const labels = new Map<string, string>();
  for (const [item, label] of flatNames(items)) labels.set(JSON.stringify([item.entry, item.id]), label);
  return (id: string, base: string) => labels.get(JSON.stringify([pageRoute()?.entry, id])) ?? base;
}

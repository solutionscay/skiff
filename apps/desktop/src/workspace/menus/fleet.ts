import { ctxMenu } from "../../ui/contextMenu";
import { fleetSkipperOf, joinFleet, renameFleetOn } from "../../app/skiffAgent";
import { confirmAction, promptAction } from "../../ui/confirm";
import { t, tn } from "../../i18n";
import type { Project } from "../../platform/types";
import type { Fleet } from "../fleets";
import { fleetWorkspaceEntries, keepInView, renameWorkspaceFleet } from "../profile";
import { showError } from "../../ui/alerts";
import { projectsHidden, renameHidden, setProjectsHidden } from "../tree";
import { render } from "../../app/render";
import { S } from "../../app/state";
import { refocusTerminal } from "../view";
import { hostCan, hostInvoke } from "../../app/hostCaps";
import { endSession } from "./end";
import { fleetSkipperEntries } from "./skipper";

/** The fleets now, for the picker. */
const fleetNames = () => [...new Set(S.projects.map((x) => x.fleet).filter((f): f is string => !!f))].sort((a, b) => a.localeCompare(b));

/** Pick the fleet the project sits in. Projects with the same fleet go under one fleet head in the tree. */
export function setFleet(p: Project) {
  promptAction({
    title: t("fleet.title", { name: p.name }),
    body: t("fleet.body"),
    placeholder: t("fleet.placeholder"),
    value: p.fleet ?? "",
    allowEmpty: true,
    options: fleetNames(),
    clearLabel: p.fleet ? t("fleet.clear") : undefined,
    action: t("fleet.action"),
    // A project Skipper that comes under a running fleet Skipper is taken over, after one confirm.
    submit: async (fleet) => {
      const joined = await joinFleet(p.name, fleet.trim());
      if (joined === null) return;
      // A fleet the open workspace does not name, or none, would take the project out of view.
      keepInView(p, fleet.trim() || null);
      if (joined === false) await hostInvoke("project_config", "set_project_fleet", { project: p.name, fleet });
    },
    onClose: refocusTerminal,
  });
}

/** Give the fleet a new name: its projects, and its Skipper's folder, rows and session. */
function renameFleet(f: Fleet, name: string) {
  renameHidden(f.name, name);
  renameWorkspaceFleet(f.name, name.trim());
  return renameFleetOn(f.name, name, f.projects.map((p) => p.name));
}

/** Take every project out of the fleet. Each goes back under Projects. */
function dissolveFleet(f: Fleet) {
  for (const p of f.projects) keepInView(p, null);
  return hostInvoke<void>("project_config", "reorder_projects", { order: S.projects.map((x) => x.name), fleets: f.projects.map((p) => [p.name, ""]) });
}

/** A right-click on a fleet tile. */
export function fleetMenu(f: Fleet, x: number, y: number) {
  // The Host keeps its fleets: one that takes no project changes keeps them as they are.
  if (!hostCan("project_config")) return;
  const skipper = fleetSkipperEntries(f);
  ctxMenu.open(x, y, t("menu.fleet", { name: f.name }), [
    ...skipper,
    ...(skipper.length ? [{ sep: true as const }] : []),
    // The label says what a click does. Hidden, the head keeps its Skipper and the sessions that need a look.
    {
      icon: projectsHidden(f.name) ? "senses-eye" : "senses-eye-off",
      label: t(projectsHidden(f.name) ? "menu.showProjects" : "menu.hideProjects"),
      run: () => {
        setProjectsHidden(f.name, !projectsHidden(f.name));
        render();
      },
    },
    { sep: true },
    {
      icon: "tools-pencil",
      label: t("menu.renameFleet"),
      run: () =>
        promptAction({
          title: t("renameFleet.title", { name: f.name }),
          body: tn("renameFleet.body", f.projects.length),
          placeholder: t("fleet.placeholder"),
          value: f.name,
          action: t("renameFleet.action"),
          submit: (name) => renameFleet(f, name),
          onClose: refocusTerminal,
        }),
    },
    {
      icon: "code-ungroup",
      label: t(fleetSkipperOf(f.name) ? "menu.removeFleetAsk" : "menu.removeFleet"),
      run: async () => {
        // The fleet is gone, so its Skipper stops: one confirm.
        const s = fleetSkipperOf(f.name);
        if (s && !(await confirmAction({ title: t("removeFleet.title", { name: f.name }), body: t("removeFleet.body", { projects: f.projects.map((p) => p.name).join(", "), name: f.name }), action: t("removeFleet.action") }))) return refocusTerminal();
        if (s) await endSession(s.id);
        await dissolveFleet(f).catch(showError);
      },
    },
    ...fleetWorkspaceEntries(f.name),
  ]);
}

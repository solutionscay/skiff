import { fleetSkipperOf, fleetSkippersHere, projectSkipperOf, startFleetSkipper, startProjectSkipper, skiffAgentFolder } from "../../app/skiffAgent";
import { confirmAction } from "../../ui/confirm";
import { t } from "../../i18n";
import { type MenuEntry } from "../../ui/menu";
import { isRemote } from "../../app/hosts";
import type { SkipperStart } from "../../ui/launchMenu";
import { agentName } from "../model";
import type { Project } from "../../platform/types";
import type { Fleet } from "../fleets";
import { refocusTerminal } from "../view";
import { hostCan } from "../../app/hostCaps";
import { endSession } from "./end";

/**
 * The fleet head's Skipper items, as a project row has them: Start fleet
 * Skipper; while one runs, Open fleet Skipper and Stop fleet Skipper….
 */
export function fleetSkipperEntries(f: Fleet): MenuEntry[] {
  if (!fleetSkippersHere()) return [];
  const s = fleetSkipperOf(f.name);
  if (!s) {
    if (!skiffAgentFolder()) return [];
    return [{ icon: "tools-sparkles", label: t("menu.startFleetSkipper"), run: () => void startFleetSkipper(f.name) }];
  }
  return [
    { icon: "tools-sparkles", label: t("menu.openFleetSkipper"), run: () => void startFleetSkipper(f.name) },
    {
      icon: "indicators-square-stop",
      label: t("menu.stopFleetSkipper"),
      danger: true,
      run: async () => {
        if (await confirmAction({ title: t("stopSkipper.title", { name: f.name }), body: "", action: t("stopFleetSkipper.action") })) void endSession(s.id);
        else refocusTerminal();
      },
    },
  ];
}

/**
 * The Skipper the start menus offer first: Open fleet Skipper while the
 * fleet's runs, Open project Skipper while the project's runs, else Start
 * project Skipper. Start opens no dialog.
 */
export function skipperStart(p: Project): SkipperStart | null {
  // This Mac starts them itself; a remote Host, through its skiff-host.
  if (isRemote() && !hostCan("skipper_run")) return null;
  const s = projectSkipperOf(p.name);
  // One Skipper per project: the fleet's Skipper commands it while it runs.
  if (p.fleet && !s && fleetSkipperOf(p.fleet)) return { label: t("menu.openFleetSkipper"), run: () => void startFleetSkipper(p.fleet!) };
  // startProjectSkipper shows the one that runs.
  if (s) return { label: t("menu.openProjectSkipper"), run: () => startProjectSkipper(p.name) };
  if (!skiffAgentFolder()) return null;
  return { label: t("menu.startProjectSkipper"), disabled: !!p.error, run: () => startProjectSkipper(p.name) };
}

/**
 * The project menu's Skipper entries. While one runs, Stop project Skipper…
 * asks first, as End does. Its panes keep running. `start` false leaves out
 * the skipperStart entry: the start choices above carry it.
 */
export function projectSkipperEntries(p: Project, start = true): MenuEntry[] {
  const s = projectSkipperOf(p.name);
  const go = start ? skipperStart(p) : null;
  const entries: MenuEntry[] = go ? [{ icon: "tools-sparkles", ...go }] : [];
  if (s) entries.push({
    icon: "indicators-square-stop",
    label: t("menu.stopProjectSkipper"),
    danger: true,
    run: async () => {
      const ok = await confirmAction({
        title: t("stopSkipper.title", { name: p.name }),
        body: t("stopProjectSkipper.body", { agent: agentName(s) }),
        action: t("stopProjectSkipper.action"),
      });
      if (ok) void endSession(s.id);
      else refocusTerminal();
    },
  });
  return entries;
}

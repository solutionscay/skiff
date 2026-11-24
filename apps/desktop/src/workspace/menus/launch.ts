import type { AgentInfo } from "../../platform/types";
import { launchIcon } from "../../appearance/agentIcon";
import { launchChoices, launchDigit } from "../../app/stateQueries";
import { t } from "../../i18n";
import type { MenuEntry } from "../../ui/menu";

/** Shell, then enabled agents, with the same labels and digit hints in each menu. */
export function launchEntries(run: (agent: AgentInfo | null) => void, disabled?: boolean): MenuEntry[] {
  return launchChoices().map((agent, i) => ({
    glyph: launchIcon(agent),
    label: agent?.id ?? t("menu.shell"),
    hint: launchDigit(i),
    ...(disabled === undefined ? {} : { disabled }),
    run: () => run(agent),
  }));
}

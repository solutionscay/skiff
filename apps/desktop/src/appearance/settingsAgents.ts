/**
 * Settings › Agents: the agents table, the restore button and the call-sign
 * boxes. Page only: settings.ts holds the agents and makes every Host call.
 */
import { h } from "../ui/dom";
import { t, upper } from "../i18n";
import { rowTable, sectionHead, tableWords, type RowTableState } from "./rowTable";
import { agentNameList, setAgentNameList } from "./agentNames";
import type { AgentInfo } from "../platform/types";

/** The agents Skiff ships with. Restore defaults brings back any the user removed. */
export const DEFAULT_AGENTS = ["claude", "codex", "gemini", "grok", "opencode", "hermes", "kimi"];

export interface AgentsPageOptions {
  /** Read at call time: an edit replaces the list. */
  agents: () => AgentInfo[];
  error: string | null;
  /** The Host name when its Skiff is too old to take changes, else null. */
  readOnly: string | null;
  table: RowTableState;
  render(): void;
  toggle(id: string): void;
  restore(): void;
  /** An agent edit. Answers with the error to show. */
  edit(cmd: string, args: Record<string, string>): Promise<string | null>;
}

/** The Agents page: `.set-main`. */
export function agentsPage(o: AgentsPageOptions): HTMLElement {
  const agents = o.agents();
  const main = h("div", "set-main");
  const intro = h("div", "set-head");
  const title = h("div", "set-h", t("settings.tab.agents"));
  title.id = "set-h-agents";
  intro.append(title);
  // A Host whose Skiff is too old to take changes: its agents, read-only.
  if (o.readOnly !== null) {
    const rows = agents.map((a) => h("div", "set-note mono", `${a.id}: ${a.command} (${t(a.installed ? (a.enabled ? "settings.remote.on" : "settings.remote.off") : "settings.remote.notOnPath")})`));
    main.append(intro, h("div", "set-note", t("settings.remote.agentsOld", { host: o.readOnly })), ...(rows.length ? rows : [h("div", "set-note", t("settings.remote.noAgents"))]));
    if (o.error) main.appendChild(h("div", "set-error", o.error));
    return main;
  }
  // Only a removed default can come back.
  if (DEFAULT_AGENTS.some((d) => !agents.some((a) => a.id === d))) {
    intro.classList.add("set-head-row");
    const back = h("button", "set-rm", t("settings.restoreDefaults"));
    back.type = "button";
    back.addEventListener("click", o.restore);
    intro.appendChild(back);
  }
  main.append(intro, agentTable(o));
  if (o.error) {
    const err = h("div", "set-error", o.error);
    err.setAttribute("role", "alert");
    main.appendChild(err);
  }
  main.appendChild(callSigns());
  return main;
}

/** One row per agent: its switch, name, command and status. The draft row adds a custom agent. */
function agentTable(o: AgentsPageOptions): HTMLElement {
  return rowTable<AgentInfo>(
    {
      grid: "g-agents",
      words: tableWords("agent"),
      labelledBy: "set-h-agents",
      endHead: upper(t("settings.colStatus")),
      rows: o.agents(),
      id: (a) => a.id,
      rowClass: (a) => "set-agent" + (a.enabled ? " on" : "") + (a.installed ? "" : " missing"),
      cols: [
        {
          key: "on",
          head: "",
          name: t("settings.agents.on"),
          kind: "lead",
          draw: (a) => {
            const sw = h("button", "c-on");
            sw.type = "button";
            sw.disabled = !a.installed;
            sw.setAttribute("role", "switch");
            sw.setAttribute("aria-checked", String(a.enabled));
            sw.setAttribute("aria-label", t(a.enabled ? "settings.agents.isOn" : "settings.agents.isOff", { id: a.id }));
            sw.title = a.installed ? "" : t("settings.agents.notOnPath", { command: a.command.split(/\s+/)[0] });
            sw.appendChild(h("span", "switch"));
            sw.addEventListener("click", () => o.toggle(a.id));
            return sw;
          },
        },
        { key: "name", head: upper(t("settings.agents.colAgent")), name: t("settings.agents.name"), kind: "key", required: true, get: (a) => a.id },
        {
          key: "command",
          head: upper(t("settings.agents.colCommand")),
          name: t("settings.agents.command"),
          field: t("settings.agents.commandField"),
          kind: "text",
          mono: true,
          required: true,
          // A known agent shows its default as the placeholder. A custom one has no default.
          placeholder: (a) => (a ? a.default_command : t("settings.agents.command")),
          get: (a) => (a.custom || a.command !== a.default_command ? a.command : ""),
        },
      ],
      status: (a) => {
        const status = h("span", "c-state");
        status.append(h("span", "sq"), t(a.installed ? "settings.agents.installed" : "settings.agents.missing"));
        return status;
      },
      // − takes any agent away that nothing starts: a default goes to `[agents] removed`. An empty command does not remove.
      // "Used by Admiral and 3 more" shows as "Admiral +3" in the STATUS cell.
      kept: (a) => a.used_by?.replace(/^Used by /, "").replace(/ and (\d+) more$/, " +$1") ?? null,
      checkEdit: (a, key, v) => (key === "command" && a.custom && !v ? t("settings.agents.needsCommand", { id: a.id }) : null),
      add: async (d) => {
        const err = await o.edit("set_agent_command", { agent: d.name, command: d.command });
        // A skiffd from before custom agents saves the line but does not list it.
        if (err || o.agents().some((a) => a.id === d.name)) return err;
        return t("settings.agents.savedRestart", { name: d.name });
      },
      edit: (a, key, v) =>
        key === "name" ? o.edit("rename_agent", { agent: a.id, name: v }) : o.edit("set_agent_command", { agent: a.id, command: v }),
      remove: (a) => o.edit("remove_agent", { agent: a.id }),
    },
    o.table,
    o.render,
  );
}

/** The words that make up the names of new agent sessions. */
function callSigns(): HTMLElement {
  const box = h("div", "set-names-box");
  box.appendChild(sectionHead(t("settings.agentNames")));
  const cols = h("div", "set-names-cols");
  for (const [list, label, aria] of [["ranks", t("settings.agentNames.titles"), t("settings.agentNames.titlesAria")], ["nouns", t("settings.agentNames.names"), t("settings.agentNames.namesAria")]] as const) {
    const col = h("div", "set-names-col");
    const area = h("textarea", "mono");
    area.spellcheck = false;
    area.value = agentNameList(list).join("\n");
    area.setAttribute("aria-label", aria);
    area.addEventListener("blur", () => {
      setAgentNameList(list, area.value.split("\n"));
      area.value = agentNameList(list).join("\n");
    });
    col.append(h("div", "set-names-label", upper(label)), area);
    cols.appendChild(col);
  }
  box.appendChild(cols);
  return box;
}

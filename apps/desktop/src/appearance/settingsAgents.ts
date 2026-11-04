/**
 * Settings › Agents: the agents table, Add agent…, Restore defaults and the
 * agent names. Page only: settings.ts holds the agents and makes every Host call.
 */
import { h } from "../ui/dom";
import { picker, smallButton, switchButton } from "../ui/controls";
import { promptAction } from "../ui/confirm";
import { rune } from "../ui/runes";
import { t } from "../i18n";
import { rowTable, sectionHead, type RowTableState } from "./rowTable";
import { agentNameList, setAgentNameList } from "./agentNames";
import { pickProgram } from "./programPicker";
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

/** The program's path with `~` for home. */
const shortPath = (p: string) => p.replace(/^\/(home|Users)\/[^/]+/, "~");

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
    const rows = agents.map((a) => h("div", "set-note mono", `${a.id}: ${a.command} (${t(a.installed ? (a.enabled ? "settings.remote.on" : "settings.remote.off") : "settings.agents.missing")})`));
    main.append(intro, h("div", "set-note", t("settings.remote.agentsOld", { host: o.readOnly })), ...(rows.length ? rows : [h("div", "set-note", t("settings.remote.noAgents"))]));
    if (o.error) main.appendChild(h("div", "set-error", o.error));
    return main;
  }
  const acts = h("div", "set-head-acts");
  // Only a removed default can come back.
  if (DEFAULT_AGENTS.some((d) => !agents.some((a) => a.id === d))) acts.append(smallButton(t("settings.restoreDefaults"), o.restore));
  // The program comes from a picker. Its file name is the agent's name, and the row renames it.
  acts.append(smallButton(t("settings.agents.add"), () => void pickProgram(t("settings.agents.add"), (path) => void add(o, path)), { go: true }));
  intro.append(acts);
  main.append(intro, agentTable(o));
  if (o.error) {
    const err = h("div", "set-error", o.error);
    err.setAttribute("role", "alert");
    main.appendChild(err);
  }
  main.appendChild(callSigns());
  return main;
}

async function add(o: AgentsPageOptions, path: string) {
  const name = path.split("/").pop() ?? path;
  let err = await o.edit("set_agent_command", { agent: name, command: path });
  // A skiffd from before custom agents saves the line but does not list it.
  if (!err && !o.agents().some((a) => a.id === name)) err = t("settings.agents.notListed", { name });
  o.table.error = err;
  o.render();
}

/** The words of a command after its program. */
const flagsOf = (a: AgentInfo) => a.command.trim().split(/\s+/).slice(1);

/** The Flags cell: a menu for the flags Skiff knows, and any other flag read only. It stays in projects.toml. */
export function flagsCell(o: AgentsPageOptions, a: AgentInfo): HTMLElement {
  const known = a.skip_approvals ?? [];
  const words = flagsOf(a);
  const rest = words.filter((w) => !known.includes(w));
  const cell = h("span", "c-flags");
  if (known.length) {
    const skip = words.some((w) => known.includes(w));
    cell.append(picker({
      label: t("settings.agents.flagsOf", { id: a.id }),
      value: skip ? "skip" : "ask",
      disabled: !a.installed,
      entries: () => [{ value: "ask", label: t("settings.agents.flagAsk") }, { value: "skip", label: t("settings.agents.flagSkip") }],
      pick: (v) => {
        const program = a.command.trim().split(/\s+/)[0];
        const command = [program, ...rest, ...(v === "skip" ? [known[0]] : [])].join(" ");
        void o.edit("set_agent_command", { agent: a.id, command }).then((err) => { o.table.error = err; o.render(); });
      },
    }));
  }
  if (rest.length) cell.append(h("span", "c-flags-rest mono", rest.join(" ")));
  return cell;
}

/** The Program cell: where it was found, read only, and the picker. */
function programCell(o: AgentsPageOptions, a: AgentInfo): HTMLElement {
  const cell = h("span", "c-prog");
  const first = a.command.trim().split(/\s+/)[0] ?? "";
  const where = a.installed ? shortPath(a.path ?? first) : "";
  const path = h("span", "c-prog-path mono", where);
  path.title = a.path ?? "";
  const change = () => void pickProgram(t("settings.agents.colProgram"), (p) => {
    void o.edit("set_agent_command", { agent: a.id, command: [p, ...flagsOf(a)].join(" ") }).then((err) => { o.table.error = err; o.render(); });
  });
  cell.append(path, smallButton(t(a.installed ? "skipper.change" : "settings.agents.choose"), change, { label: t("settings.agents.programOf", { id: a.id }) }));
  return cell;
}

/** One row per agent: its switch, name, program, flags and status. */
function agentTable(o: AgentsPageOptions): HTMLElement {
  return rowTable<AgentInfo>(
    {
      grid: "g-agents",
      labelledBy: "set-h-agents",
      endHead: t("settings.colStatus"),
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
            const sw = switchButton({ label: t(a.enabled ? "settings.agents.isOn" : "settings.agents.isOff", { id: a.id }), on: a.enabled, disabled: !a.installed, toggle: () => o.toggle(a.id) });
            sw.classList.add("c-on");
            return sw;
          },
        },
        { key: "name", head: t("settings.agents.colAgent"), name: t("settings.agents.name"), kind: "key", get: (a) => a.id },
        { key: "program", head: t("settings.agents.colProgram"), name: t("settings.agents.colProgram"), kind: "lead", draw: (a) => programCell(o, a) },
        { key: "flags", head: t("settings.agents.colFlags"), name: t("settings.agents.colFlags"), kind: "lead", draw: (a) => flagsCell(o, a) },
      ],
      status: (a) => {
        const status = h("span", "c-state");
        status.append(h("span", "sq"), t(a.installed ? "settings.agents.installed" : "settings.agents.missing"));
        return status;
      },
      // − takes any agent away that nothing starts: a default goes to `[agents] removed`.
      // "Used by Admiral and 3 more" shows as "Admiral +3" in the STATUS cell.
      kept: (a) => a.used_by?.replace(/^Used by /, "").replace(/ and (\d+) more$/, " +$1") ?? null,
      edit: (a, _key, v) => o.edit("rename_agent", { agent: a.id, name: v }),
      remove: (a) => o.edit("remove_agent", { agent: a.id }),
    },
    o.table,
    o.render,
  );
}

/** The words that make up the names of new agent sessions: a tile for each, one row high. */
function callSigns(): HTMLElement {
  const box = h("div", "set-names-box");
  for (const [list, label] of [["ranks", t("settings.names.titles")], ["nouns", t("settings.names.names")]] as const) {
    const names = agentNameList(list);
    box.append(sectionHead(label, undefined, String(names.length)));
    const grid = h("div", "set-names-grid");
    const redraw = () => box.replaceWith(callSigns());
    for (const name of names) {
      const tile = h("div", "set-name-tile");
      const rm = smallButton(t("table.remove"), () => { setAgentNameList(list, names.filter((n) => n !== name)); redraw(); }, { icon: rune("indicators-minus"), danger: true, label: t("table.removeRow", { id: name }) });
      tile.append(h("span", "set-name-text", name), rm);
      grid.append(tile);
    }
    const addName = smallButton(t("settings.names.add"), () => promptAction({
      title: label,
      placeholder: label,
      action: t("settings.names.addAction"),
      submit: async (name: string) => { setAgentNameList(list, [...names, name]); redraw(); },
    }));
    addName.classList.add("set-name-add");
    grid.append(addName);
    box.append(grid);
  }
  return box;
}

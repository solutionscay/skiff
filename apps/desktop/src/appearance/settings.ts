import { HARBOR } from "./colors";
import { escapeButton } from "../ui/dialogParts";
import { PANE_OPACITY_MIN, paneOpacity, setPaneOpacity } from "./backdrop";
import { h } from "../ui/dom";
import { invoke } from "@tauri-apps/api/core";
import type { AgentInfo, TerminalTheme } from "../platform/types";
import { agentNameList, setAgentNameList } from "./agentNames";
import { themeGrid } from "./themeCards";
import { type Action, actionFor } from "../app/keys";

/**
 * Settings over the sidebar and terminal: agents, appearance, and Open with.
 */
export interface AppearanceHooks {
  themes: () => TerminalTheme[];
  /** A theme id, or null for Harbor. */
  current: () => string | null;
  load: () => Promise<void>;
  set: (id: string | null) => void;
}

export function createSettings(onAgents: (a: AgentInfo[]) => void, onClose: () => void, look: AppearanceHooks) {
  const root = h("div");
  root.id = "settings";
  root.hidden = true;
  root.setAttribute("role", "dialog");
  root.setAttribute("aria-label", "Settings");
  document.getElementById("body")!.appendChild(root);

  let agents: AgentInfo[] = [];
  const TABS = ["agents", "appearance", "open"] as const;
  let tab: (typeof TABS)[number] = "agents";
  type OpenKey = "diff" | "text" | "markdown" | "html";
  type OpenSettings = Record<OpenKey, string> & { default_diff: string };
  let openSet: OpenSettings | null = null;
  let error: string | null = null;
  let saving = false;

  async function toggle(id: string) {
    if (saving) return;
    const enabled = agents.filter((a) => (a.id === id ? !a.enabled : a.enabled)).map((a) => a.id);
    saving = true;
    error = null;
    // Show the change at once; the daemon's answer replaces it.
    agents = agents.map((a) => (a.id === id ? { ...a, enabled: !a.enabled } : a));
    render();
    try {
      agents = await invoke<AgentInfo[]>("set_agents", { enabled });
      onAgents(agents);
    } catch (e) {
      error = String(e);
      agents = await invoke<AgentInfo[]>("list_agents").catch(() => agents);
    } finally {
      saving = false;
      render();
    }
  }

  async function setCommand(id: string, command: string) {
    error = null;
    try {
      agents = await invoke<AgentInfo[]>("set_agent_command", { agent: id, command });
      onAgents(agents);
    } catch (e) {
      error = String(e);
    }
    render();
  }

  function render() {
    // A rebuild keeps the list where it was scrolled.
    const scroll = root.querySelector(".set-main")?.scrollTop ?? 0;
    // So does the focused field: a save on blur finishes after the click that moved focus to the next field.
    const a = document.activeElement;
    const field = a instanceof HTMLInputElement && root.contains(a) ? { label: a.getAttribute("aria-label"), at: a.selectionStart, to: a.selectionEnd } : null;
    draw();
    const main = root.querySelector(".set-main");
    if (main) main.scrollTop = scroll;
    if (field?.label) {
      const next = [...root.querySelectorAll<HTMLInputElement>("input")].find((i) => i.getAttribute("aria-label") === field.label);
      next?.focus();
      if (next && field.at !== null && field.to !== null) next.setSelectionRange(field.at, field.to);
    }
  }

  function draw() {
    const nav = h("div", "set-nav");
    const head = h("div", "set-nav-head");
    head.append(h("div", "set-title", "Settings"), h("div", "set-path mono", "~/.config/skiff/projects.toml"));
    const names = { agents: "Agents", appearance: "Appearance", open: "Open with" };
    const tabs = TABS.map((t) => {
      const b = h("button", "set-tab" + (tab === t ? " active" : ""), names[t]);
      b.type = "button";
      b.addEventListener("click", () => {
        tab = t;
        draw();
      });
      return b;
    });
    nav.append(head, ...tabs);
    if (tab === "appearance") {
      root.replaceChildren(nav, appearance(), closeButton());
      return;
    }
    if (tab === "open") {
      root.replaceChildren(nav, openWith(), closeButton());
      return;
    }

    const main = h("div", "set-main");
    const intro = h("div", "set-head");
    intro.append(h("div", "set-h", "Agents"));
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "c-on", ""), h("span", "c-name", "AGENT"), h("span", "c-cmd", "COMMAND"), h("span", "c-status", "STATUS"));
    main.append(intro, cols);
    for (const a of agents) {
      const row = h("div", "set-row set-agent" + (a.enabled ? " on" : "") + (a.installed ? "" : " missing"));
      const sw = h("button", "c-on");
      sw.type = "button";
      sw.disabled = !a.installed;
      sw.setAttribute("role", "switch");
      sw.setAttribute("aria-checked", String(a.enabled));
      sw.setAttribute("aria-label", `${a.id} ${a.enabled ? "on" : "off"}`);
      sw.title = a.installed ? "" : `${a.command.split(/\s+/)[0]} is not on PATH`;
      sw.appendChild(h("span", "switch"));
      sw.addEventListener("click", () => void toggle(a.id));

      const cmd = h("input", "c-cmd mono");
      cmd.value = a.command === a.default_command ? "" : a.command;
      cmd.placeholder = a.default_command;
      cmd.spellcheck = false;
      cmd.setAttribute("aria-label", `${a.id} command`);
      const save = () => {
        const v = cmd.value.trim();
        const cur = a.command === a.default_command ? "" : a.command;
        if (v !== cur) void setCommand(a.id, v);
      };
      cmd.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          cmd.blur();
        } else if (e.key === "Escape") {
          cmd.value = a.command === a.default_command ? "" : a.command;
          cmd.blur();
        }
      });
      cmd.addEventListener("blur", save);

      const status = h("span", "c-status");
      status.append(h("span", "sq"), a.installed ? "installed" : "not on PATH");
      row.append(sw, h("span", "c-name", a.id), cmd, status);
      main.appendChild(row);
    }
    if (error) {
      const err = h("div", "set-error", error);
      err.setAttribute("role", "alert");
      main.appendChild(err);
    }
    main.appendChild(callSigns());
    root.replaceChildren(nav, main, closeButton());
  }

  /** Esc, in the top right corner as in the theme picker. A click closes. */
  function closeButton(): HTMLButtonElement {
    return escapeButton("set-x", "Close settings", close);
  }

  /** The words that make up the names of new agent sessions. */
  function callSigns(): HTMLElement {
    const box = h("div", "set-names-box");
    const head = h("div", "set-head");
    head.append(h("div", "set-h", "Agent names"));
    box.appendChild(head);
    const cols = h("div", "set-names-cols");
    for (const [list, label] of [["ranks", "TITLES"], ["nouns", "NAMES"]] as const) {
      const col = h("div", "set-names-col");
      const area = h("textarea", "mono");
      area.spellcheck = false;
      area.value = agentNameList(list).join("\n");
      area.setAttribute("aria-label", `Agent ${label.toLowerCase()}`);
      area.addEventListener("blur", () => {
        setAgentNameList(list, area.value.split("\n"));
        area.value = agentNameList(list).join("\n");
      });
      col.append(h("div", "set-names-label", label), area);
      cols.appendChild(col);
    }
    box.appendChild(cols);
    return box;
  }

  /** App colors made from a terminal theme. A click applies it. */
  function appearance(): HTMLElement {
    const main = h("div", "set-main");
    const intro = h("div", "set-head");
    intro.append(
      h("div", "set-h", "Appearance"),
      h("div", "set-sub", "App theme. Colors the app and every project and session without a theme of its own."),
    );
    const cur = look.current()?.replace(/^foot:/, "builtin:") ?? null;
    const cards = h("div", "set-cards");
    cards.appendChild(
      themeGrid(look.themes(), {
        active: cur ?? HARBOR,
        pick: (id) => {
          look.set(id === HARBOR ? null : id);
          render();
        },
      }),
    );
    // Over a project's background image, panes are this opaque.
    const op = h("div", "set-row set-opacity");
    const slider = h("input", "c-cmd");
    slider.type = "range";
    slider.min = String(Math.round(PANE_OPACITY_MIN * 100));
    slider.max = "100";
    slider.value = String(Math.round(paneOpacity() * 100));
    slider.setAttribute("aria-label", "Terminal opacity over a background image");
    const val = h("span", "c-status", `${slider.value}%`);
    slider.addEventListener("input", () => {
      setPaneOpacity(Number(slider.value) / 100);
      val.textContent = `${slider.value}%`;
    });
    op.append(h("span", "c-name", "Terminal opacity"), slider, val);
    main.append(intro, op, cards);
    return main;
  }

  async function setOpen(key: OpenKey, command: string) {
    error = null;
    try {
      openSet = await invoke<OpenSettings>("set_open", { key, command });
    } catch (e) {
      error = String(e);
    }
    render();
  }

  const OPEN_ROWS: { key: OpenKey; label: string }[] = [
    { key: "diff", label: "Diffs" },
    { key: "text", label: "Text files" },
    { key: "markdown", label: "Markdown" },
    { key: "html", label: "HTML" },
  ];

  /** Which apps Skiff hands things to. */
  function openWith(): HTMLElement {
    const main = h("div", "set-main");
    const intro = h("div", "set-head");
    intro.append(h("div", "set-h", "Open with"));
    const cols = h("div", "set-row set-open set-cols");
    cols.append(h("span", "c-name", "SHOWS"), h("span", "c-cmd", "COMMAND"));
    main.append(intro, cols);
    const o = openSet;
    if (o) for (const { key, label } of OPEN_ROWS) {
      const row = h("div", "set-row set-open set-agent on");
      const cmd = h("input", "c-cmd mono");
      cmd.value = o[key];
      cmd.placeholder = key === "diff" ? o.default_diff : "default app";
      cmd.spellcheck = false;
      cmd.setAttribute("aria-label", `${label} command`);
      cmd.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          cmd.blur();
        } else if (e.key === "Escape") {
          cmd.value = o[key];
          cmd.blur();
        }
      });
      cmd.addEventListener("blur", () => {
        if (cmd.value.trim() !== o[key].trim()) void setOpen(key, cmd.value);
      });
      row.append(h("span", "c-name", label), cmd);
      main.appendChild(row);
    }
    if (error) {
      const err = h("div", "set-error", error);
      err.setAttribute("role", "alert");
      main.appendChild(err);
    }
    return main;
  }

  function close() {
    // Leaving a name list saves it.
    if (document.activeElement instanceof HTMLTextAreaElement) document.activeElement.blur();
    root.hidden = true;
    onClose();
  }

  /** Ctrl+Shift+Up/Down: the previous or next section, as on the rail. The keys go to its tab. */
  function stepTab(dir: 1 | -1) {
    // Leaving a field saves it, as a click on a tab does.
    (document.activeElement as HTMLElement | null)?.blur();
    tab = TABS[(TABS.indexOf(tab) + dir + TABS.length) % TABS.length];
    draw();
    document.body.classList.add("kbd");
    root.querySelector<HTMLButtonElement>(".set-tab.active")?.focus();
  }

  /** Ctrl+Shift+Right: into the section, at its first control. Ctrl+Shift+Left: back to its tab. */
  function enterSection(into: boolean) {
    document.body.classList.add("kbd");
    if (!into) return root.querySelector<HTMLButtonElement>(".set-tab.active")?.focus();
    root.querySelector<HTMLElement>(".set-main :is(button, input, textarea, [tabindex]):not(:disabled)")?.focus();
  }

  /** The list keys, as Settings reads them. True when Settings used the key. */
  function runKey(a: Action | null): boolean {
    if (a === "session-next" || a === "session-prev") stepTab(a === "session-next" ? 1 : -1);
    else if (a === "list-project" || a === "list-back") enterSection(a === "list-back");
    else return false;
    return true;
  }

  // On the document, so Escape works wherever focus is. Menus and dialogs
  // over settings stop the key first.
  document.addEventListener("keydown", (e) => {
    if (root.hidden || e.defaultPrevented || document.querySelector("#ctx-menu:not([hidden]), #launch-menu:not([hidden]), .confirm-overlay")) return;
    if (runKey(actionFor(e))) return e.preventDefault();
    if (e.key !== "Escape") return;
    e.preventDefault();
    close();
  });

  return {
    async open() {
      root.hidden = false;
      await look.load();
      openSet = await invoke<OpenSettings>("open_settings").catch(() => null);
      agents = await invoke<AgentInfo[]>("list_agents").catch((e) => {
        error = String(e);
        return [];
      });
      render();
      root.querySelector<HTMLButtonElement>(".set-agent .c-on:not(:disabled)")?.focus();
    },
    close,
    /** The native menu runs its accelerators here, not through the keydown above. */
    runKey: (a: Action) => !root.hidden && runKey(a),
    get isOpen() {
      return !root.hidden;
    },
  };
}

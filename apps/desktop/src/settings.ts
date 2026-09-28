import { invoke } from "@tauri-apps/api/core";
import type { AgentInfo, TerminalTheme } from "./types";
import { themeGrid } from "./themeCards";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

/**
 * Full-window settings over the sidebar and terminal. One section for now:
 * which agents the + menu offers.
 */
export interface AppearanceHooks {
  themes: () => TerminalTheme[];
  /** A theme id, or null for Harbor. */
  current: () => string | null;
  load: () => Promise<void>;
  set: (id: string | null) => void;
}

const HARBOR = "builtin:harbor";

export function createSettings(onAgents: (a: AgentInfo[]) => void, onClose: () => void, look: AppearanceHooks) {
  const root = el("div");
  root.id = "settings";
  root.hidden = true;
  root.setAttribute("role", "dialog");
  root.setAttribute("aria-label", "Settings");
  document.getElementById("body")!.appendChild(root);

  let agents: AgentInfo[] = [];
  let tab: "agents" | "appearance" = "agents";
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
    draw();
    const main = root.querySelector(".set-main");
    if (main) main.scrollTop = scroll;
  }

  function draw() {
    const nav = el("div", "set-nav");
    const head = el("div", "set-nav-head");
    head.append(el("div", "set-title", "Settings"), el("div", "set-path mono", "~/.config/skiff/projects.toml"));
    const tabs = (["agents", "appearance"] as const).map((t) => {
      const b = el("button", "set-tab" + (tab === t ? " active" : ""), t === "agents" ? "Agents" : "Appearance");
      b.type = "button";
      b.addEventListener("click", () => {
        tab = t;
        draw();
      });
      return b;
    });
    const done = el("button", "set-done", "Done");
    done.type = "button";
    done.addEventListener("click", close);
    nav.append(head, ...tabs, el("div", "spacer"), done);
    if (tab === "appearance") {
      root.replaceChildren(nav, appearance());
      return;
    }

    const main = el("div", "set-main");
    const intro = el("div", "set-head");
    intro.append(
      el("div", "set-h", "Agents"),
      el("div", "set-sub", "Turn on the agents you use. The + button on a worktree offers them, plus a plain shell."),
    );
    const cols = el("div", "set-row set-cols");
    cols.append(el("span", "c-on", ""), el("span", "c-name", "AGENT"), el("span", "c-cmd", "COMMAND"), el("span", "c-status", "STATUS"));
    main.append(intro, cols);
    for (const a of agents) {
      const row = el("div", "set-row set-agent" + (a.enabled ? " on" : "") + (a.installed ? "" : " missing"));
      const sw = el("button", "c-on");
      sw.type = "button";
      sw.disabled = !a.installed;
      sw.setAttribute("role", "switch");
      sw.setAttribute("aria-checked", String(a.enabled));
      sw.setAttribute("aria-label", `${a.id} ${a.enabled ? "on" : "off"}`);
      sw.title = a.installed ? "" : `${a.command.split(/\s+/)[0]} is not on PATH`;
      sw.appendChild(el("span", "switch"));
      sw.addEventListener("click", () => void toggle(a.id));

      const cmd = el("input", "c-cmd mono");
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
          e.stopPropagation();
          cmd.value = a.command === a.default_command ? "" : a.command;
          cmd.blur();
        }
      });
      cmd.addEventListener("blur", save);

      const status = el("span", "c-status");
      status.append(el("span", "sq"), a.installed ? "installed" : "not on PATH");
      row.append(sw, el("span", "c-name", a.id), cmd, status);
      main.appendChild(row);
    }
    if (error) {
      const err = el("div", "set-error", error);
      err.setAttribute("role", "alert");
      main.appendChild(err);
    }
    main.appendChild(el("div", "set-note", "Empty command: the default runs. Type a full command line to change it, for example claude --dangerously-skip-permissions. Saved under [agents] in projects.toml."));
    root.replaceChildren(nav, main);
  }

  /** App colors made from a terminal theme. A click applies it. */
  function appearance(): HTMLElement {
    const main = el("div", "set-main");
    const intro = el("div", "set-head");
    intro.append(
      el("div", "set-h", "Appearance"),
      el("div", "set-sub", "Colors the app and every terminal. Right-click a terminal to give it its own theme."),
    );
    const cur = look.current()?.replace(/^foot:/, "builtin:") ?? null;
    const cards = el("div", "set-cards");
    cards.appendChild(
      themeGrid(look.themes(), {
        active: cur ?? HARBOR,
        pick: (id) => {
          look.set(id === HARBOR ? null : id);
          render();
        },
      }),
    );
    main.append(intro, cards);
    return main;
  }

  function close() {
    root.hidden = true;
    onClose();
  }

  root.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  });

  return {
    async open() {
      root.hidden = false;
      await look.load();
      agents = await invoke<AgentInfo[]>("list_agents").catch((e) => {
        error = String(e);
        return [];
      });
      render();
      root.querySelector<HTMLButtonElement>(".set-agent .c-on:not(:disabled)")?.focus();
    },
    close,
    get isOpen() {
      return !root.hidden;
    },
  };
}

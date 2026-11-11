import { DEFAULT_THEME } from "./colors";
import { escapeButton } from "../ui/dialogParts";
import { h } from "../ui/dom";
import { invoke } from "@tauri-apps/api/core";
import { imageCount } from "../terminal/images";
import { size as byteSize } from "./skiffFolderDialog";
import type { AgentInfo, Appearance, TerminalTheme } from "../platform/types";
import { type MessageKey, t } from "../i18n";
import { nextCard, themeGrid, type ThemeFilters } from "./themeCards";
import { type Action, actionFor, isMac } from "../app/keys";
import { editField, fieldEditKey, textField } from "../ui/fieldClipboard";
import { mainName, type InstructionFile, type SkiffAgentSettings, type TaskType } from "../app/skiffAgent";
import { addTaskButton, finishRow, folderRows, type SkiffAgentEdit, skippersPage, taskTable } from "./skiffAgentSettings";
import { peekText } from "../terminal/peek";
import { changeSkiffFolder } from "./skiffFolderDialog";
import { sectionHead, tableState } from "./rowTable";
import { type HostsPage, hostName, isRemote, knownHosts } from "../app/hosts";
import { createHostsTab } from "./hostSettings";
import { openWithPage, type OpenChoices, type OpenSettings, type OpenKey } from "./openWithSettings";
import { hostCan, hostInvoke } from "../app/hostCaps";

import { countDisk, diskState, diskUseSection } from "./diskUse";
import { imagesSection, imagesState } from "./imagesPage";
import { onImagesChanged } from "../terminal/images";
import { activityState, daemonActivitySection, startActivity, stopActivity } from "./daemonActivity";
import { revealSession } from "../workspace/view";
import { locate } from "../workspace/model";
import { openClosedProject } from "../workspace/projectClose";
import { S, sessions } from "../app/state";
import { arrowKeys, redrawKeeping } from "./settingsFocus";
import { createSettingsFiles, type FilesState } from "./settingsFiles";
import { agentsPage } from "./settingsAgents";
import { languageRow, type MenuLayout, menuLayout } from "./settingsLanguage";
import { loadSchedulesPage, schedulesPage, schedulesState } from "./schedulesPage";
import { schedules, type Schedule } from "../workspace/scheduleStore";

/**
 * Settings over the sidebar and terminal. Two groups. The shown Host's pages:
 * Agents, Skippers, Sessions, Disk use, Images. The app's pages: Appearance,
 * Open with, Hosts.
 */
interface AppearanceHooks {
  themes: () => TerminalTheme[];
  /** A theme id, or null for the default Foot theme. */
  current: () => string | null;
  load: () => Promise<void>;
  set: (id: string | null) => void;
  /** Themes that replace the app theme on screen, each with a label that names its scope. */
  overrides?: () => { id: string; label: string }[];
}

export function createSettings(onAgents: (a: AgentInfo[]) => void, onClose: () => void, look: AppearanceHooks, onShowChange: () => void = () => {}) {
  const root = h("div");
  root.id = "settings";
  root.hidden = true;
  root.setAttribute("role", "dialog");
  root.setAttribute("aria-label", t("cmd.settings"));
  document.getElementById("body")!.appendChild(root);

  let agents: AgentInfo[] = [];
  const HOST_TABS = ["agents", "skippers", "task-types", "schedules", "activity", "disk", "images"] as const;
  const APP_TABS = ["appearance", "open", "hosts"] as const;
  const TABS = [...HOST_TABS, ...APP_TABS] as const;
  type Tab = (typeof TABS)[number];
  let tab: Tab = "agents";
  const TAB_NAMES: Record<Tab, MessageKey> = {
    agents: "settings.tab.agents", skippers: "settings.tab.skippers", "task-types": "settings.tab.taskTypes", schedules: "settings.tab.schedules", images: "settings.tab.images",
    disk: "settings.tab.disk", activity: "settings.tab.activity", appearance: "settings.tab.appearance", open: "settings.tab.open", hosts: "settings.tab.hosts",
  };
  /** The sections that moved: each old id opens the page that holds it now. */
  const OLD_TABS: Record<string, Tab> = { language: "appearance" };
  /** A count on a page's row, when Skiff knows it without a read. */
  const tabCount = (id: Tab): string => {
    if (id === "agents") return agents.length ? t("settings.ofCount", { on: agents.filter((a) => a.enabled).length, all: agents.length }) : "";
    if (id === "skippers") return fs.files.length ? String(fs.files.length) : "";
    if (id === "task-types") return agentSet ? String((agentSet.tasks ?? []).length) : "";
    if (id === "schedules") return hostCan("schedules") ? String(schedules.length) : "";
    if (id === "activity") return String([...sessions.values()].filter((x) => !x.peek).length);
    if (id === "disk") return disk.usage ? byteSize(disk.usage.tree.bytes) : "";
    if (id === "images") return String(imageCount());
    if (id === "hosts") return String(knownHosts().length + 1);
    return "";
  };
  const themeFilters: ThemeFilters = { query: "", family: null };
  let openSet: OpenSettings | null = null;
  let openFound: OpenChoices = {};
  let menuSet: MenuLayout | null = null;
  /** `[appearance] language`: a code, or null for the system language. */
  let langSet: string | null = null;
  let agentSet: SkiffAgentSettings | null = null;
  /** The random faces of Settings › Appearance › Finish: one set per open. */
  let sampleSeeds: number[] = [];
  // Settings › Skippers › Instructions, and one error line per row.
  const fs: FilesState<InstructionFile> = { files: [], fileErrors: new Map(), typeErrors: new Map() };
  let error: string | null = null;
  let saving = false;
  // The draft rows and their error lines survive every redraw.
  const agentsTable = tableState();
  const tasksTable = tableState();
  const hostsTab = createHostsTab(() => render());
  const sched = schedulesState();
  const schedCtx = { render: () => render(), mainName: () => mainName(agentSet?.name), editPrompt };

  /**
   * An Edit's file in the peek over Settings. Settings stays on its page.
   * When the peek ends, `done` reads the page again, and the keys go back to
   * the button that opened the file, else to the page's tab.
   */
  function peekOver(script: string, cwd: string, path: string, done: () => unknown) {
    const label = document.activeElement instanceof HTMLElement && root.contains(document.activeElement) ? document.activeElement.getAttribute("aria-label") : null;
    peekText(script, cwd, path, () => {
      void Promise.resolve(done()).then(() => {
        if (root.hidden) return;
        const back = label ? [...root.querySelectorAll<HTMLElement>("[aria-label]")].find((el) => el.getAttribute("aria-label") === label) : null;
        (back ?? root.querySelector<HTMLElement>(".set-tab.active"))?.focus();
      });
    });
  }

  /**
   * Schedules › Prompt › Edit: the schedule's file opens as Open with ›
   * Text files says, as Task types › Edit does, with its `# <name>` line
   * written first when there is none. A terminal tool runs in the peek over
   * this page, and the size reads again when it ends.
   */
  async function editPrompt(s: Schedule): Promise<string | null> {
    let o: { kind: "run"; script: string; cwd: string } | { kind: "default" } | { kind: "no_app"; message: string };
    try {
      o = await invoke("edit_schedule_prompt", { uid: s.uid });
    } catch (e) {
      return String(e);
    }
    if (o.kind === "run") {
      peekOver(o.script, o.cwd, `${o.cwd}/${s.uid}.md`, () => loadSchedulesPage(schedCtx));
      return null;
    }
    // The file is new or saved: read its size again, as Task types › Edit does.
    void loadSchedulesPage(schedCtx);
    return o.kind === "no_app" ? t("settings.noApp", { file: `${s.uid}.md` }) : null;
  }
  const { loadFiles, editFile, loadTypes, editTypeFile, resetFile } = createSettingsFiles<InstructionFile, SkiffAgentSettings, TaskType>({
    ask: invoke,
    peek: peekOver,
    hostName,
    showing: (at) => !root.hidden && tab === (OLD_TABS[at] ?? at),
    render: () => render(),
    setError: (e) => { error = e; },
    setAgentSet: (a) => { agentSet = a; },
  }, fs);

  /** The agents, from the selected Host. A remote Host's come with the revision its next change is made from. */
  function loadAgents(): Promise<AgentInfo[]> {
    if (isRemote() && hostCan("agent_config")) return hostInvoke<AgentInfo[]>("agent_config", "agent_config");
    return hostInvoke<AgentInfo[]>("session_agents", "list_agents");
  }

  /** One agent change on the selected Host: This Mac's config, or a remote Host's through its skiff-host. */
  const agentCall = (cmd: string, args: Record<string, unknown> = {}) => hostInvoke<AgentInfo[]>("agent_config", cmd, args);
  const disk = diskState();
  const gallery = imagesState();
  const activity = activityState();
  // A drop, a paste or a delete elsewhere redraws the gallery.
  onImagesChanged(() => {
    if (!root.hidden && tab === "images") render();
  });

  async function toggle(id: string) {
    if (saving) return;
    const enabled = agents.filter((a) => (a.id === id ? !a.enabled : a.enabled)).map((a) => a.id);
    saving = true;
    error = null;
    // Show the change at once; the daemon's answer replaces it.
    agents = agents.map((a) => (a.id === id ? { ...a, enabled: !a.enabled } : a));
    render();
    try {
      agents = await agentCall("set_agents", { enabled });
      onAgents(agents);
    } catch (e) {
      error = String(e);
      agents = await loadAgents().catch(() => agents);
    } finally {
      saving = false;
      render();
    }
  }

  /** Sends an agent edit to the daemon and shows the list it answers with. */
  async function editAgents(cmd: string, args: Record<string, string> = {}) {
    error = null;
    try {
      agents = await agentCall(cmd, args);
      onAgents(agents);
    } catch (e) {
      error = String(e);
      agents = await loadAgents().catch(() => agents);
    }
    render();
  }

  /** An agent edit from its table. Answers with the error to show; the table redraws. */
  async function agentEdit(cmd: string, args: Record<string, string>): Promise<string | null> {
    try {
      agents = await agentCall(cmd, args);
      onAgents(agents);
    } catch (e) {
      // A conflict forgot the revision: read the Host's agents again.
      agents = await loadAgents().catch(() => agents);
      return String(e);
    }
    // A renamed agent takes its task types with it.
    agentSet = await hostInvoke<SkiffAgentSettings>("skipper", "get_skiff_agent").catch(() => agentSet);
    return null;
  }

  const render = () => redrawKeeping(root, tab, draw);

  function draw() {
    const nav = h("div", "set-nav");
    const head = h("div", "set-nav-head");
    head.append(h("div", "set-title", t("cmd.settings")));
    const tabButton = (id: Tab) => {
      const b = h("button", "set-tab" + (tab === id ? " active" : ""));
      b.append(h("span", "set-tab-name", t(TAB_NAMES[id])));
      if (id === "hosts" && knownHosts().some((x) => x.state === "attention")) b.append(h("i", "set-tab-bad"));
      b.append(h("span", "set-tab-count", tabCount(id)));
      b.type = "button";
      b.addEventListener("click", () => {
        if (tab === "activity") stopActivity(activity);
        tab = id;
        draw();
        if (tab === "skippers") void loadFiles();
        if (tab === "task-types") void loadTypes();
        if (tab === "schedules") void loadSchedulesPage(schedCtx);
        if (tab === "disk") void countDisk(disk, render);
        if (tab === "activity") startActivity(activity, render);
        if (tab === "appearance") root.querySelector<HTMLInputElement>(".tc-search")?.focus();
      });
      return b;
    };
    nav.append(head, sectionHead(hostName()), ...HOST_TABS.map(tabButton), sectionHead(t("settings.group.app")), ...APP_TABS.map(tabButton));
    if (tab === "appearance") {
      root.replaceChildren(nav, pinClose(appearance()));
      return;
    }
    if (tab === "open") {
      root.replaceChildren(nav, pinClose(openWithPage(openSet, openFound, error, { set: (key, command, peek) => void setOpen(key, command, peek), setApp: (key, path) => void setOpenApp(key, path) })));
      return;
    }
    if (tab === "skippers") {
      root.replaceChildren(nav, pinClose(skippers()));
      return;
    }
    if (tab === "task-types") {
      root.replaceChildren(nav, pinClose(taskTypes()));
      return;
    }
    if (tab === "schedules") {
      root.replaceChildren(nav, pinClose(schedulesPage(sched, schedCtx)));
      return;
    }
    if (tab === "hosts") {
      root.replaceChildren(nav, pinClose(hostsTab.draw()));
      return;
    }
    if (tab === "images") {
      root.replaceChildren(nav, pinClose(imagesSection(gallery, close)));
      return;
    }
    if (tab === "disk") {
      const main = h("div", "set-main");
      // A file opens as Settings › Open with says: a terminal tool in the peek over this tab.
      const opener = {
        local: !isRemote(),
        peek: (script: string, cwd: string, name: string) => peekOver(script, cwd, `${cwd}/${name}`, () => countDisk(disk, render)),
      };
      main.append(...diskUseSection(disk, render, (days) => void setCacheDays(days), opener, agentSet && !(isRemote() && !hostCan("skipper_config")) ? folderRows(agentSet, changeFolder) : []));
      root.replaceChildren(nav, pinClose(main));
      return;
    }
    if (tab === "activity") {
      const main = h("div", "set-main");
      main.append(...daemonActivitySection(activity, render, async (id, closed) => {
        if (closed) {
          const session = activity.list.find((x) => x.id === id);
          const at = session && locate(S.closedProjects, session.cwd);
          if (at) await openClosedProject(at.project);
        }
        close(); revealSession(id);
      }));
      root.replaceChildren(nav, pinClose(main));
      return;
    }

    root.replaceChildren(nav, pinClose(agentsPage({
      agents: () => agents,
      error,
      readOnly: isRemote() && !hostCan("agent_config") ? hostName() : null,
      table: agentsTable,
      render,
      toggle,
      restore: () => void editAgents("restore_agents"),
      edit: agentEdit,
    })));
  }

  /** Esc in the page's head, which stays at the top as the page scrolls. A click closes. */
  function pinClose(main: HTMLElement): HTMLElement {
    (main.querySelector(":scope > .set-head") ?? main).append(escapeButton("set-x", t("settings.close"), close));
    return main;
  }

  /** App colors made from a terminal theme. A click applies it. */
  function appearance(): HTMLElement {
    const main = h("div", "set-main");
    const intro = h("div", "set-head");
    intro.append(h("div", "set-h", t("settings.tab.appearance")));
    const cur = look.current() ?? null;
    const cards = h("div", "set-cards");
    cards.appendChild(
      themeGrid(look.themes(), {
        active: look.themes().some((t) => t.id === cur) ? cur : DEFAULT_THEME,
        filters: themeFilters,
        scopes: look.overrides?.(),
        reload: async () => { await look.load(); return look.themes(); },
        pick: (id) => {
          look.set(id === DEFAULT_THEME ? null : id);
        },
      }),
    );
    main.append(intro, languageRow(langSet, (code) => void setLanguageSetting(code)));
    if (menuSet?.available) main.appendChild(menuLayout(menuSet, (v) => void setMenuLayout(v)));
    if (error) {
      const err = h("div", "set-error", error);
      err.setAttribute("role", "alert");
      main.appendChild(err);
    }
    // The sample faces wear the theme colors Skippers get: each theme's main color.
    const colors = look.themes().flatMap((t) => (t.main_color ? [t.main_color] : []));
    main.append(finishRow(draw, sampleSeeds, colors), sectionHead(t("settings.appTheme"), undefined, String(look.themes().length)), cards);
    return main;
  }

  async function setLanguageSetting(code: string | null) {
    error = null;
    try {
      await invoke("set_language", { language: code });
      langSet = code;
    } catch (e) {
      error = String(e);
    }
    render();
  }

  async function setMenuLayout(layout: string | null) {
    error = null;
    try {
      menuSet = await invoke<MenuLayout>("set_menu_layout", { layout });
    } catch (e) {
      error = String(e);
    }
    render();
  }

  /** One Open with row: its command, and whether it opens in the peek. */
  async function setOpen(key: OpenKey, command: string, peek: boolean) {
    error = null;
    try {
      openSet = await invoke<OpenSettings>("set_open", { key, command });
      // Diffs and Images have no peek setting: set_open_peek refuses them.
      if (key !== "diff" && key !== "image") openSet = await invoke<OpenSettings>("set_open_peek", { key, peek });
    } catch (e) {
      error = String(e);
    }
    render();
  }

  /** One Open with row, from the file picker. */
  async function setOpenApp(key: OpenKey, path: string) {
    error = null;
    try {
      openSet = await invoke<OpenSettings>("set_open_app", { key, path });
    } catch (e) {
      error = String(e);
    }
    render();
  }

  /** One Skiff agent setting on the selected Host: This Mac, or a remote Host through its skiff-host. */
  const skiffAgentEdit = (edit: SkiffAgentEdit) => hostInvoke<SkiffAgentSettings>("skipper_config", "set_skiff_agent", { edit });

  /** A task type edit. Answers with the error to show; the table redraws. */
  async function saveTask(edit: SkiffAgentEdit): Promise<string | null> {
    try {
      agentSet = await skiffAgentEdit(edit);
      return null;
    } catch (e) {
      return String(e);
    }
  }

  async function setSkiffAgent(edit: SkiffAgentEdit) {
    error = null;
    try {
      agentSet = await skiffAgentEdit(edit);
    } catch (e) {
      error = String(e);
    }
    render();
  }

  /** Settings › Disk use › Auto flush: `[skiff_agent] cache_days`. */
  async function setCacheDays(days: number) {
    try {
      await invoke("set_cache_days", { days });
      if (disk.usage) disk.usage.cache_days = days;
      disk.error = null;
    } catch (e) {
      disk.error = String(e);
    }
    render();
  }

  /** Settings › Disk use › Folder › Change…. The page reads the folder again after a change. */
  function changeFolder() {
    if (!agentSet) return;
    changeSkiffFolder({
      current: agentSet.configured,
      changed: async () => {
        agentSet = await hostInvoke<SkiffAgentSettings>("skipper", "get_skiff_agent").catch(() => agentSet);
        render();
      },
    });
  }

  /** Settings › Skippers: one row for each Skipper. */
  function skippers(): HTMLElement {
    const main = h("div", "set-main");
    const intro = h("div", "set-head");
    intro.append(h("div", "set-h", t("settings.tab.skippers")));
    main.append(intro);
    // A Host whose Skiff is too old to take changes: its Skiff agent, read-only.
    if (isRemote() && !hostCan("skipper_config")) {
      main.append(h("div", "set-note", agentSet
        ? t("settings.remote.skipperOld", { host: hostName() })
        : t("settings.remote.noSkipper", { host: hostName() })));
      if (agentSet) {
        const a = agentSet;
        main.append(
          h("div", "set-note mono", t("settings.remote.skipperName", { name: mainName(a.name), agent: a.agent ?? t("settings.remote.default") })),
          h("div", "set-note mono", t(a.folder_error ? "settings.remote.folderRefused" : "settings.remote.folder", { folder: a.folder ?? a.configured, error: a.folder_error ?? "" })),
        );
      }
      return main;
    }
    if (error) {
      const err = h("div", "set-error", error);
      err.setAttribute("role", "alert");
      main.appendChild(err);
    }
    if (agentSet) main.append(skippersPage(agentSet, agents, {
      save: (e) => void setSkiffAgent(e),
      redraw: draw,
      files: { files: fs.files, errors: fs.fileErrors, editable: hostCan("skipper_file"), edit: (f) => void editFile(f), reset: (f) => void resetFile(f) },
    }));
    return main;
  }

  /** Settings › Task types: one row for each type, and Add task type…. */
  function taskTypes(): HTMLElement {
    const main = h("div", "set-main");
    const intro = h("div", "set-head");
    const title = h("div", "set-h", t("settings.tab.taskTypes"));
    title.id = "set-h-tasks";
    intro.append(title);
    main.append(intro);
    // A Host whose Skiff is too old to take changes: its task types, read-only.
    if (isRemote() && !hostCan("skipper_config")) {
      main.append(h("div", "set-note", agentSet
        ? t("settings.remote.skipperOld", { host: hostName() })
        : t("settings.remote.noSkipper", { host: hostName() })));
      if (agentSet) main.append(...(agentSet.tasks ?? []).map((task) => h("div", "set-note mono", `${task.type}: ${task.command ?? task.agent}${task.error ? ` (${task.error})` : ""}`)));
      return main;
    }
    if (!agentSet) return main;
    const acts = { saveTask, render, tasksTable, types: { edit: editTypeFile, errors: fs.typeErrors } };
    const buttons = h("div", "set-head-acts");
    buttons.append(addTaskButton(agentSet, agents, acts));
    intro.append(buttons);
    main.append(taskTable(agentSet, agents, acts));
    return main;
  }

  function close() {
    // Leaving a name list saves it.
    if (document.activeElement instanceof HTMLTextAreaElement) document.activeElement.blur();
    root.hidden = true;
    stopActivity(activity);
    gallery.keys = null;
    onClose();
    onShowChange();
  }

  /** Ctrl+Shift+Up/Down: the previous or next section, as in the tree. The keys go to its tab. */
  function stepTab(dir: 1 | -1) {
    // Leaving a field saves it, as a click on a tab does.
    (document.activeElement as HTMLElement | null)?.blur();
    tab = TABS[(TABS.indexOf(tab) + dir + TABS.length) % TABS.length];
    if (tab !== "activity") stopActivity(activity);
    draw();
    if (tab === "skippers") void loadFiles();
    if (tab === "task-types") void loadTypes();
    if (tab === "schedules") void loadSchedulesPage(schedCtx);
    if (tab === "disk") void countDisk(disk, render);
    if (tab === "activity") startActivity(activity, render);
    document.body.classList.add("kbd");
    root.querySelector<HTMLButtonElement>(".set-tab.active")?.focus();
  }

  /** Ctrl+Shift+Right: into the section, at its first control. Ctrl+Shift+Left: back to its tab. */
  function enterSection(into: boolean) {
    document.body.classList.add("kbd");
    if (!into) return root.querySelector<HTMLButtonElement>(".set-tab.active")?.focus();
    if (tab === "appearance") return root.querySelector<HTMLInputElement>(".tc-search")?.focus();
    root.querySelector<HTMLElement>(".set-main :is(button, input, [tabindex]):not(:disabled, .set-x)")?.focus();
  }

  arrowKeys(root, { stepTab, enterSection, nextCard });

  /**
   * The text field with the keys, in Settings or in a dialog over it, such as
   * Folder › Change…. The native menu sends copy and paste here while a dialog is open.
   */
  const field = () => {
    const f = textField(document.activeElement);
    return f && (root.contains(f) || f.closest(".confirm-overlay")) ? f : null;
  };

  /** The list keys, and copy and paste in a text field, as Settings reads them. True when Settings used the key. */
  function runKey(a: Action | null): boolean {
    // A tool in the peek over Settings keeps its keys.
    if (document.activeElement?.closest(".peek")) return false;
    const f = a === "copy" || a === "paste" ? field() : null;
    if (f) void editField(f, a as "copy" | "paste");
    else if (a === "session-next" || a === "session-prev") stepTab(a === "session-next" ? 1 : -1);
    else if (a === "list-project" || a === "list-back") enterSection(a === "list-back");
    else return false;
    return true;
  }

  // Capture on the window so a terminal that still has focus while Settings
  // loads cannot stop Escape before Settings sees it. Menus and dialogs over
  // settings stop the key first.
  window.addEventListener("keydown", (e) => {
    if (root.hidden || e.defaultPrevented || document.querySelector("#ctx-menu:not([hidden]), #launch-menu:not([hidden]), .confirm-overlay")) return;
    if (e.target instanceof Element && e.target.closest(".peek")) return;
    if (runKey(actionFor(e))) return e.preventDefault();
    // Cut on macOS. The webview needs an Edit menu item for it.
    const edit = fieldEditKey(e, isMac);
    const f = edit && field();
    if (edit && f) {
      e.preventDefault();
      return void editField(f, edit);
    }
    if (e.key !== "Escape") return;
    e.preventDefault();
    close();
  }, true);

  // An external app saved a file: its status shows when the window has the keys again.
  window.addEventListener("focus", () => {
    if (!root.hidden && tab === "skippers") void loadFiles();
    if (!root.hidden && tab === "task-types") void loadTypes();
    if (!root.hidden && tab === "schedules") void loadSchedulesPage(schedCtx);
  });

  const settings = {
    /** A new read of the Host list redraws an open Hosts tab. main.ts wires it to
     *  onHostsChanged: panels.ts makes Settings while hosts.ts is still loading. */
    hostsChanged() {
      if (!root.hidden && tab === "hosts") render();
    },
    /** `at`: the section to show, else the last one. A Hosts page opens
     *  Settings › Hosts at its list or its Add form. */
    async open(at?: HostsPage | Tab | keyof typeof OLD_TABS) {
      if (typeof at === "string") tab = OLD_TABS[at] ?? (at as Tab);
      else if (at) {
        tab = "hosts";
        hostsTab.open(at);
      }
      root.hidden = false;
      // New random faces and colors for the Finish samples on each open; a redraw keeps them.
      sampleSeeds = [];
      onShowChange();
      // Disk use shows at once and starts its count: the loads below can take a while.
      if (tab === "disk") {
        render();
        void countDisk(disk, render);
      }
      await look.load();
      openSet = await invoke<OpenSettings>("open_settings").catch(() => null);
      openFound = await invoke<OpenChoices>("open_choices").catch(() => ({}));
      menuSet = await invoke<MenuLayout>("menu_layout").catch(() => null);
      langSet = (await invoke<Appearance>("get_appearance").catch(() => null))?.language ?? null;
      agentSet = await hostInvoke<SkiffAgentSettings>("skipper", "get_skiff_agent").catch(() => null);
      agents = await loadAgents().catch((e) => {
        error = String(e);
        return [];
      });
      await hostsTab.load();
      render();
      if (tab === "hosts") hostsTab.focusServer();
      if (tab === "skippers") void loadFiles();
      if (tab === "schedules") void loadSchedulesPage(schedCtx);
      if (tab === "disk") void countDisk(disk, render);
      if (tab === "activity") startActivity(activity, render);
      if (tab === "appearance") root.querySelector<HTMLInputElement>(".tc-search")?.focus();
      else if (tab === "images") root.querySelector<HTMLInputElement>(".img-page .lib-search")?.focus();
      else root.querySelector<HTMLButtonElement>(".set-agent .c-on:not(:disabled)")?.focus();
    },
    close,
    /** The native menu runs its accelerators here, not through the keydown above. */
    runKey: (a: Action) => !root.hidden && runKey(a),
    get isOpen() {
      return !root.hidden;
    },
  };
  return settings;
}

/** The one open workspace and what follows from it: the profile, shows, the ledger scope, missing references and the tree view. */
import { render } from "../../app/renderRequest";
import { S } from "../../app/state";
import type { Project } from "../../platform/types";
import { fleetKey, projectKey } from "../tree";
import { t } from "../../i18n";
import { basename } from "../paths";

/** The profile as `workspace_read` gives it and `workspace_write` takes it. */
export interface Profile {
  /** Project folders. */
  projects: string[];
  /** Fleet names. */
  fleets: string[];
  folded_projects: string[];
  /** Fleet names. "" is the Projects head (`sectionKey`). */
  folded_fleets: string[];
  /** The folder of the last project the user picked here. */
  selected: string | null;
}

interface Doc {
  path: string;
  profile: Profile;
  /** The revision last read or written. */
  revision: string | null;
  /** `JSON` of the profile last read or written: a change from it is saved. */
  written: string;
  /** The file is gone or cannot be read. Changes wait until it is saved again. */
  missing: boolean;
  /** The last write failed with this error. The next change tries again. */
  failed: string | null;
}

export const EXT = "skiffw";
export const trimPath = (p: string) => (p.length > 1 ? p.replace(/\/+$/, "") : p);
/** The workspace's name: its file name without `.skiffw`. */
export const nameOf = (path: string) => basename(path).replace(new RegExp(`\\.${EXT}$`), "");

export const emptyProfile = (): Profile => ({ projects: [], fleets: [], folded_projects: [], folded_fleets: [], selected: null });

export function createWorkspaceState() {
  let doc: Doc | null = null;
  /** Rows the screen opened in this workspace. Not saved. */
  const autoOpened = new Set<string>();
  /** Above 0 while the app, not the user, moves the selection. */
  let quiet = 0;
  /** Off until the launch has opened its workspace: the first restore is no change. */
  let ready = false;
  /** The Ledger shows every row, whatever the workspace. Never saved. */
  let allWork = false;
  let announce = (_text: string) => {};
  let changed = () => {};

  const workspaceOpen = () => doc !== null;
  const workspaceName = () => (doc ? nameOf(doc.path) : t("workspace.allProjects"));

  // ── What shows ─────────────────────────────────────────────────────────────

  /** The project is in the open workspace, by itself or by its fleet. Always true in All projects. */
  function shows(p: Project): boolean {
    if (!doc) return true;
    return doc.profile.projects.some((x) => trimPath(x) === trimPath(p.path)) || (!!p.fleet && doc.profile.fleets.includes(p.fleet));
  }

  const shownProjects = () => S.projects.filter(shows);

  /**
   * The project names whose work the Ledger shows, or null for every row.
   * A row with no project shows in every workspace.
   */
  function ledgerScope(): { label: string; names: Set<string> } | null {
    if (!doc || allWork) return null;
    return { label: workspaceName(), names: new Set(shownProjects().map((p) => p.name)) };
  }

  const ledgerAllWork = () => allWork;

  /** All work, or back to the workspace's work. Not saved. */
  function setLedgerAllWork(on: boolean) {
    if (allWork === on) return;
    allWork = on;
    announce(on ? t("workspace.ledgerAll") : t("workspace.ledgerScoped", { name: workspaceName() }));
    render();
  }

  /** A reference with nothing to point at. It stays in the file until the user changes or removes it. */
  interface Missing {
    kind: "project" | "fleet";
    ref: string;
    why: string;
  }

  function missing(): Missing[] {
    if (!doc) return [];
    const out: Missing[] = [];
    for (const path of doc.profile.projects) {
      if (S.projects.some((p) => trimPath(p.path) === trimPath(path))) continue;
      const closed = S.closedProjects.find((p) => trimPath(p.path) === trimPath(path));
      out.push({ kind: "project", ref: path, why: closed ? t("workspace.whyClosed", { name: closed.name }) : t("workspace.whyNoProject") });
    }
    const fleets = new Set(S.projects.map((p) => p.fleet).filter((f): f is string => !!f));
    for (const f of doc.profile.fleets) if (!fleets.has(f)) out.push({ kind: "fleet", ref: f, why: t("workspace.whyNoFleet") });
    return out;
  }

  // ── Folds and picks ────────────────────────────────────────────────────────

  const pathOf = (name: string) => {
    const p = S.projects.find((x) => x.name === name);
    return p ? trimPath(p.path) : null;
  };

  /** A tree key, as the file names it: a project by folder, a fleet by name. */
  function refOf(key: string): { list: "folded_projects" | "folded_fleets"; ref: string } | null {
    if (key.startsWith("project:")) {
      const ref = pathOf(key.slice("project:".length));
      return ref ? { list: "folded_projects", ref } : null;
    }
    if (key.startsWith("fleet:")) return { list: "folded_fleets", ref: key.slice("fleet:".length) };
    return null;
  }

  const treeView = {
    shows,
    isFolded(key: string) {
      const r = doc && refOf(key);
      return !!r && !autoOpened.has(key) && doc!.profile[r.list].some((x) => x === r.ref);
    },
    fold(key: string, fold: boolean) {
      const r = doc && refOf(key);
      if (!r) return;
      autoOpened.delete(key);
      const list = doc!.profile[r.list].filter((x) => x !== r.ref);
      if (fold) list.push(r.ref);
      doc!.profile[r.list] = list;
      changed();
    },
    autoOpen(key: string) {
      autoOpened.add(key);
    },
    keys() {
      if (!doc) return [];
      const out: string[] = [];
      for (const p of S.projects) if (treeView.isFolded(projectKey(p.name))) out.push(projectKey(p.name));
      for (const f of doc.profile.folded_fleets) if (treeView.isFolded(fleetKey(f))) out.push(fleetKey(f));
      return out;
    },
  };

  /** The user picked a project. In a workspace that shows it, the pick is saved. */
  function noteSelected(name: string) {
    if (!doc || !ready || quiet) return;
    const p = S.projects.find((x) => x.name === name);
    if (!p || !shows(p)) return;
    const path = trimPath(p.path);
    if (doc.profile.selected === path) return;
    doc.profile.selected = path;
    changed();
  }

  function quietly(f: () => void) {
    quiet++;
    try {
      f();
    } finally {
      quiet--;
    }
  }

  return {
    get doc() { return doc; },
    set doc(next: Doc | null) { doc = next; },
    get ready() { return ready; },
    set ready(next: boolean) { ready = next; },
    get allWork() { return allWork; },
    set allWork(next: boolean) { allWork = next; },
    setHooks(next: { announce: (text: string) => void; changed: () => void }) { ({ announce, changed } = next); },
    autoOpened, workspaceOpen, workspaceName, shows, ledgerScope, ledgerAllWork, setLedgerAllWork,
    missing, treeView, noteSelected, quietly,
  };
}

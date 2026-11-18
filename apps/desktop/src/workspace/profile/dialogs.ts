/** The workspace dialog: New and Edit. */
import { dialogFrame } from "../../ui/dialogParts";
import { button, h } from "../../ui/dom";
import { picker, smallButton } from "../../ui/controls";
import { t } from "../../i18n";
import type { Project } from "../../platform/types";
import { emptyProfile, type Profile, type createWorkspaceState } from "./state";
import type { NameFault } from "./transitions";
import type { S as AppState } from "../../app/state";

// ── Workspace dialog ───────────────────────────────────────────────────────

function trapTab(panel: HTMLElement, e: KeyboardEvent) {
  const stops = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled)")];
  const i = stops.indexOf(document.activeElement as HTMLElement);
  stops[(i + (e.shiftKey ? -1 : 1) + stops.length) % stops.length]?.focus();
}

/**
 * New (`isNew`) or Edit: the name, the projects and fleets, and for each
 * reference that is not found, Remove or Use instead. Done creates the
 * workspace, or renames it and changes it; each is saved at once.
 */
export function workspaceDialog(state: ReturnType<typeof createWorkspaceState>, isNew: boolean, hooks: {
  home: () => HTMLElement | null; changed: () => void; showView: () => void; S: typeof AppState; trimPath: (path: string) => string;
  create: (name: string, profile: Profile) => Promise<NameFault | null>; renameTo: (name: string) => Promise<NameFault | null>;
}) {
  const { home, changed, showView, S, trimPath, create, renameTo } = hooks;
  const shows = state.shows;
  const clone = (p: Profile): Profile => JSON.parse(JSON.stringify(p));
  const d = isNew ? null : state.doc;
  if (!isNew && !d) return;
  const base: Profile = d ? d.profile : emptyProfile();
  const back = document.activeElement as HTMLElement | null;
  const heading = isNew ? t("workspace.newTitle") : t("workspace.editTitle");
  const { overlay, panel } = dialogFrame("confirm-panel ws-edit", "dialog", heading);
  const title = h("div", "confirm-title", heading);
  const body = h("div", "ws-edit-body");
  const nameLabel = h("label", "ws-field");
  const name = h("input", "prompt-input");
  name.type = "text";
  name.value = isNew ? "" : state.workspaceName();
  name.spellcheck = false;
  nameLabel.append(h("span", "ws-label", t("workspace.name")), name);
  const fault = h("div", "prompt-error");
  fault.setAttribute("role", "alert");
  fault.hidden = true;
  body.append(nameLabel, fault);

  const fleets = [...new Set(S.projects.map((p) => p.fleet).filter((f): f is string => !!f))];
  // One tree in the sidebar's order: projects in no fleet, then each fleet with its projects under it.
  const box = (label: string, checked: boolean, nested = false) => {
    const l = h("label", nested ? "ws-check nested" : "ws-check");
    const c = h("input");
    c.type = "checkbox";
    c.checked = checked;
    l.append(c, h("span", "ws-check-name", label));
    return { l, c };
  };
  const fleetBoxes = new Map<string, HTMLInputElement>();
  const projectBoxes = new Map<string, HTMLInputElement>();
  // Fleet coverage changes the checkbox display, not an explicit inclusion.
  const explicitProjects = new Set(base.projects.map(trimPath));
  const kidsOf = new Map<string, HTMLInputElement[]>();
  const loose: HTMLInputElement[] = [];
  const tree = h("div", "ws-tree");
  tree.setAttribute("role", "group");
  tree.setAttribute("aria-label", t("workspace.treeLabel"));
  const inList = (p: Project) => base.projects.some((x) => trimPath(x) === trimPath(p.path));
  const projectBox = (p: Project, nested: boolean) => {
    const { l, c } = box(p.name, inList(p), nested);
    c.value = trimPath(p.path);
    l.title = p.path;
    projectBoxes.set(trimPath(p.path), c);
    c.addEventListener("change", () => {
      if (c.checked) explicitProjects.add(trimPath(p.path));
      else explicitProjects.delete(trimPath(p.path));
    });
    tree.append(l);
    return c;
  };
  for (const p of S.projects) if (!p.fleet) loose.push(projectBox(p, false));
  for (const f of fleets) {
    const { l, c } = box(f, base.fleets.includes(f));
    fleetBoxes.set(f, c);
    tree.append(l);
    kidsOf.set(f, S.projects.filter((p) => p.fleet === f).map((p) => projectBox(p, true)));
  }

  /** A checked fleet covers its projects: they show checked and cannot change. Some projects alone: the fleet is mixed. */
  const syncFleet = (f: string) => {
    const fc = fleetBoxes.get(f)!;
    const kids = kidsOf.get(f)!;
    for (const k of kids) {
      k.checked = fc.checked || explicitProjects.has(k.value);
      k.disabled = fc.checked;
    }
    fc.indeterminate = !fc.checked && kids.some((k) => k.checked);
  };
  const all = h("input");
  all.type = "checkbox";
  const syncAll = () => {
    const tops = [...fleetBoxes.values(), ...loose];
    const on = tops.filter((c) => c.checked).length;
    const some = on > 0 || [...projectBoxes.values()].some((c) => c.checked);
    all.checked = tops.length > 0 && on === tops.length;
    all.indeterminate = !all.checked && some;
  };
  for (const [f, fc] of fleetBoxes) {
    fc.addEventListener("change", () => {
      syncFleet(f);
      syncAll();
    });
    for (const k of kidsOf.get(f)!)
      k.addEventListener("change", () => {
        syncFleet(f);
        syncAll();
      });
  }
  for (const c of loose) c.addEventListener("change", syncAll);
  all.addEventListener("change", () => {
    for (const c of [...fleetBoxes.values(), ...loose]) c.checked = all.checked;
    if (!all.checked) explicitProjects.clear();
    else for (const p of S.projects) if (!p.fleet) explicitProjects.add(trimPath(p.path));
    for (const f of fleets) syncFleet(f);
    syncAll();
  });
  for (const f of fleets) syncFleet(f);
  syncAll();
  // Up and Down step through the boxes that can change.
  const step = (e: KeyboardEvent) => {
    if ((e.key !== "ArrowDown" && e.key !== "ArrowUp") || e.ctrlKey || e.metaKey || e.altKey) return;
    const stops = [all, ...tree.querySelectorAll<HTMLInputElement>("input:not(:disabled)")];
    const i = stops.indexOf(document.activeElement as HTMLInputElement);
    if (i < 0) return;
    stops[(i + (e.key === "ArrowDown" ? 1 : -1) + stops.length) % stops.length].focus();
    e.preventDefault();
  };
  tree.addEventListener("keydown", step);

  const head = h("label", "ws-sec");
  if (S.projects.length) {
    all.setAttribute("aria-label", t("workspace.selectAllLabel"));
    head.append(all, h("span", "", t("workspace.selectAll")));
    all.addEventListener("keydown", step);
  } else head.textContent = t("workspace.projects");
  body.append(head);
  if (!S.projects.length) body.append(h("div", "ws-note", t("workspace.noProjects")));
  body.append(tree);
  /** What Done does with each reference that is not found: null keeps it, "" removes it, else the folder or fleet to use instead. */
  const repairs = new Map<ReturnType<typeof state.missing>[number], string | null>();
  const lost = isNew ? [] : state.missing();
  if (lost.length) {
    const sec = h("div", "ws-sec", t("workspace.notFound"));
    body.append(sec);
    for (const m of lost) {
      const row = h("div", "ws-missing");
      repairs.set(m, null);
      const choices = m.kind === "project"
        ? S.projects.map((p) => ({ value: trimPath(p.path), label: t("workspace.useProject", { name: p.name, path: p.path }) }))
        : fleets.map((f) => ({ value: f, label: t("workspace.useFleet", { name: f }) }));
      const use = picker({
        label: t("workspace.useInsteadLabel", { ref: m.ref }),
        value: "",
        shows: t("workspace.useInstead"),
        entries: () => choices,
        pick: (value) => {
          repairs.set(m, value);
          const shown = use.querySelector(".ctl-pick-value");
          if (shown) shown.textContent = choices.find((c) => c.value === value)?.label ?? value;
        },
      });
      const remove = smallButton(t("workspace.remove"), () => {
        repairs.set(m, "");
        row.remove();
        if ([...repairs.values()].every((v) => v === "")) sec.remove();
      }, { label: t("workspace.removeLabel", { ref: m.ref }) });
      const acts = h("div", "ws-missing-acts");
      acts.append(use, remove);
      row.append(h("span", "ws-missing-ref", m.ref), h("span", "ws-missing-why", m.why), acts);
      body.append(row);
    }
  }
  const foot = h("div", "confirm-foot");
  const cancel = button("confirm-cancel", t("workspace.cancel"), () => close());
  const apply = button("confirm-act go", t("workspace.done"), () => void submit());
  foot.append(cancel, apply);
  panel.append(title, body, foot);
  overlay.append(panel);
  document.body.append(overlay);

  function close() {
    overlay.remove();
    (back?.isConnected ? back : home())?.focus();
  }
  let busy = false;
  async function submit() {
    if (busy) return;
    if (d && state.doc !== d) return close();
    const next = clone(base);
    const shownBefore = new Set(S.projects.filter(shows).map((p) => trimPath(p.path)));
    // References that are not found keep their place unless changed or removed.
    let projects = base.projects.filter((x) => !S.projects.some((p) => trimPath(p.path) === trimPath(x)));
    let fleetsNext = base.fleets.filter((f) => !fleets.includes(f));
    for (const path of projectBoxes.keys()) if (explicitProjects.has(path)) projects.push(path);
    for (const [f, c] of fleetBoxes) if (c.checked) fleetsNext.push(f);
    for (const [m, to] of repairs) {
      if (to === null) continue;
      if (m.kind === "project") {
        projects = projects.filter((x) => x !== m.ref);
        if (to) projects.push(to);
        next.folded_projects = next.folded_projects.map((x) => (x === m.ref && to ? to : x)).filter((x) => x !== m.ref);
        if (next.selected === m.ref) next.selected = to || null;
      } else {
        fleetsNext = fleetsNext.filter((x) => x !== m.ref);
        if (to) fleetsNext.push(to);
        next.folded_fleets = next.folded_fleets.map((x) => (x === m.ref && to ? to : x)).filter((x) => x !== m.ref);
      }
    }
    next.projects = [...new Set(projects)];
    next.fleets = [...new Set(fleetsNext)];
    busy = true;
    const why = isNew ? await create(name.value, next) : await renameTo(name.value);
    busy = false;
    if (why) {
      fault.textContent = why === "empty" ? t("workspace.nameEmpty") : t("workspace.nameTaken", { name: name.value.trim() });
      fault.hidden = false;
      name.focus();
      return;
    }
    overlay.remove();
    if (!d || state.doc !== d) return;
    d.profile = next;
    // A project that left the view takes its fold and pick with it.
    const shownNow = new Set(S.projects.filter(shows).map((p) => trimPath(p.path)));
    const left = (x: string) => shownBefore.has(trimPath(x)) && !shownNow.has(trimPath(x));
    next.folded_projects = next.folded_projects.filter((x) => !left(x));
    if (next.selected && left(next.selected)) next.selected = null;
    changed();
    showView();
  }
  overlay.addEventListener("mousedown", (e) => e.target === overlay && close());
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") close();
    else if (e.key === "Enter" && e.target === name) void submit();
    else if (e.key === "Tab") trapTab(panel, e);
    else return;
    e.preventDefault();
  });
  name.focus();
  name.select();
}

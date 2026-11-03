/** The choice dialog and its Tab trap. */
import { dialogFrame } from "../../ui/dialogParts";
import { button, h } from "../../ui/dom";
import { t } from "../../i18n";
import type { Project } from "../../platform/types";
import type { Profile, createWorkspaceState } from "./state";
import type { S as AppState } from "../../app/state";

// ── Choice dialog ──────────────────────────────────────────────────────────

export function trapTab(panel: HTMLElement, e: KeyboardEvent) {
  const stops = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled)")];
  const i = stops.indexOf(document.activeElement as HTMLElement);
  stops[(i + (e.shiftKey ? -1 : 1) + stops.length) % stops.length]?.focus();
}

/**
 * A modal with Cancel and the given choices. Cancel has the keys, and Enter
 * on anything but a choice is Cancel. Esc or a click outside is Cancel.
 */
export function choose(o: { title: string; body: string; choices: { id: string; label: string; tone: "go" | "danger" | "plain" }[] }): Promise<string> {
  return new Promise((resolve) => {
    const { overlay, panel } = dialogFrame("confirm-panel", "alertdialog", o.title);
    const title = h("div", "confirm-title", o.title);
    const body = h("div", "confirm-body", o.body);
    title.id = "ws-choose-title";
    body.id = "ws-choose-body";
    panel.setAttribute("aria-labelledby", title.id);
    panel.setAttribute("aria-describedby", body.id);
    panel.removeAttribute("aria-label");
    const foot = h("div", "confirm-foot");
    const done = (id: string) => {
      overlay.remove();
      resolve(id);
    };
    const cancel = button("confirm-cancel", t("workspace.cancel"), () => done("cancel"));
    foot.append(cancel);
    for (const c of o.choices) {
      const cls = c.tone === "go" ? "confirm-act go" : c.tone === "danger" ? "confirm-act" : "confirm-cancel";
      foot.append(button(cls, c.label, () => done(c.id)));
    }
    panel.append(title, body, foot);
    overlay.append(panel);
    document.body.append(overlay);
    overlay.addEventListener("mousedown", (e) => e.target === overlay && done("cancel"));
    overlay.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") done("cancel");
      else if (e.key === "Enter") {
        // Enter presses the button with the keys; anywhere else it cancels.
        const b = document.activeElement;
        if (b instanceof HTMLButtonElement && panel.contains(b)) b.click();
        else done("cancel");
      } else if (e.key === "Tab") trapTab(panel, e);
      else return;
      e.preventDefault();
    });
    cancel.focus();
  });
}

/** Name, projects, fleets, and a repair or remove for each missing reference. Apply changes the profile in memory. */
export function editWorkspaceDialog(state: ReturnType<typeof createWorkspaceState>, hooks: { home: () => HTMLElement | null; changed: () => void; showView: () => void; S: typeof AppState; trimPath: (path: string) => string }) {
  const { home, changed, showView, S, trimPath } = hooks;
  const workspaceName = state.workspaceName;
  const shows = state.shows;
  const clone = (p: Profile): Profile => JSON.parse(JSON.stringify(p));
  if (!state.doc || state.saving) return;
  const d = state.doc;
  const back = document.activeElement as HTMLElement | null;
  const { overlay, panel } = dialogFrame("confirm-panel ws-edit", "dialog", t("workspace.editLabel", { name: workspaceName() }));
  const title = h("div", "confirm-title", t("workspace.editTitle"));
  const body = h("div", "ws-edit-body");
  const nameLabel = h("label", "ws-field");
  const name = h("input", "prompt-input");
  name.type = "text";
  name.value = d.profile.name;
  name.placeholder = t("workspace.untitled");
  name.spellcheck = false;
  nameLabel.append(h("span", "ws-label", t("workspace.name")), name);
  body.append(nameLabel, h("div", "ws-note", t("workspace.editNote")));

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
  const explicitProjects = new Set(d.profile.projects.map(trimPath));
  const kidsOf = new Map<string, HTMLInputElement[]>();
  const loose: HTMLInputElement[] = [];
  const tree = h("div", "ws-tree");
  tree.setAttribute("role", "group");
  tree.setAttribute("aria-label", t("workspace.treeLabel"));
  const inList = (p: Project) => d.profile.projects.some((x) => trimPath(x) === trimPath(p.path));
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
    const { l, c } = box(f, d.profile.fleets.includes(f));
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
  const repairs = new Map<ReturnType<typeof state.missing>[number], HTMLSelectElement>();
  const lost = state.missing();
  if (lost.length) {
    body.append(h("div", "ws-sec", t("workspace.missing")));
    for (const m of lost) {
      const row = h("div", "ws-missing");
      const sel = h("select", "ws-repair");
      sel.setAttribute("aria-label", t("workspace.repairLabel", { ref: m.ref }));
      const opt = (value: string, text: string) => {
        const o = h("option", "", text);
        o.value = value;
        sel.append(o);
      };
      opt("keep", t("workspace.keepUnresolved"));
      opt("remove", t("workspace.removeProject"));
      if (m.kind === "project") for (const p of S.projects) opt(`use:${trimPath(p.path)}`, t("workspace.useProject", { name: p.name, path: p.path }));
      else for (const f of fleets) opt(`use:${f}`, t("workspace.useFleet", { name: f }));
      row.append(h("span", "ws-missing-ref", m.ref), h("span", "ws-missing-why", m.why), sel);
      repairs.set(m, sel);
      body.append(row);
    }
  }
  const foot = h("div", "confirm-foot");
  const cancel = button("confirm-cancel", t("workspace.cancel"), () => close());
  const apply = button("confirm-act go", t("workspace.apply"), () => submit());
  foot.append(cancel, apply);
  panel.append(title, body, foot);
  overlay.append(panel);
  document.body.append(overlay);

  function close() {
    overlay.remove();
    (back?.isConnected ? back : home())?.focus();
  }
  function submit() {
    if (state.doc !== d || state.saving) return close();
    const next = clone(d.profile);
    next.name = name.value.trim();
    const shownBefore = new Set(S.projects.filter(shows).map((p) => trimPath(p.path)));
    // Missing references keep their place unless repaired or removed.
    let projects = d.profile.projects.filter((x) => !S.projects.some((p) => trimPath(p.path) === trimPath(x)));
    let fleetsNext = d.profile.fleets.filter((f) => !fleets.includes(f));
    for (const path of projectBoxes.keys()) if (explicitProjects.has(path)) projects.push(path);
    for (const [f, c] of fleetBoxes) if (c.checked) fleetsNext.push(f);
    for (const [m, sel] of repairs) {
      if (sel.value === "keep") continue;
      const to = sel.value.startsWith("use:") ? sel.value.slice(4) : null;
      if (m.kind === "project") {
        projects = projects.filter((x) => x !== m.ref);
        if (to) projects.push(to);
        next.folded_projects = next.folded_projects.map((x) => (x === m.ref && to ? to : x)).filter((x) => x !== m.ref);
        if (next.selected === m.ref) next.selected = to;
      } else {
        fleetsNext = fleetsNext.filter((x) => x !== m.ref);
        if (to) fleetsNext.push(to);
        next.folded_fleets = next.folded_fleets.map((x) => (x === m.ref && to ? to : x)).filter((x) => x !== m.ref);
      }
    }
    next.projects = [...new Set(projects)];
    next.fleets = [...new Set(fleetsNext)];
    d.profile = next;
    // A project that left the view takes its fold and pick with it.
    const shownNow = new Set(S.projects.filter(shows).map((p) => trimPath(p.path)));
    const left = (x: string) => shownBefore.has(trimPath(x)) && !shownNow.has(trimPath(x));
    next.folded_projects = next.folded_projects.filter((x) => !left(x));
    if (next.selected && left(next.selected)) next.selected = null;
    overlay.remove();
    changed();
    showView();
  }
  overlay.addEventListener("mousedown", (e) => e.target === overlay && close());
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") close();
    else if (e.key === "Enter" && e.target === name) submit();
    else if (e.key === "Tab") trapTab(panel, e);
    else return;
    e.preventDefault();
  });
  name.focus();
  name.select();
}

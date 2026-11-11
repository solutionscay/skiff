/**
 * Settings › Schedules (#262): a Host page. The list has one row for each
 * schedule with Run now, Pause or Resume, Edit and −. Add… and Edit open the
 * edit page: Name and When, then Skipper and Prompt, in two bands. A
 * schedule in the app is When + a prompt to a Skipper, which coordinates
 * the rest. A command, a task type or a pane that `skiff schedule` gave a
 * schedule shows in a read-only row, and an edit keeps it. The prompt is a
 * file, `schedules/<uid>.md`, as a task type's instructions are: its row
 * shows the size and Edit opens it as Open with says.
 * When is one sentence: a picker is its value as text with a faint
 * underline, and a click opens the app menu. The time is a typed
 * HH:MM field. Leaving a field saves it. A new schedule saves once its name
 * and its cron are valid. Each change runs `skiff schedule` on the Host.
 */
import { S } from "../app/state";
import { accent } from "../app/stateQueries";
import { hostCan, hostInvoke } from "../app/hostCaps";
import { type MessageKey, t } from "../i18n";
import { ctxMenu } from "../ui/contextMenu";
import { confirmAction } from "../ui/confirm";
import { smallButton, textBox } from "../ui/controls";
import { h } from "../ui/dom";
import type { MenuEntry } from "../ui/menu";
import { rune } from "../ui/runes";
import { skipperWord } from "../app/skiffAgent";
import { fleetSkipperName, lastWord, targetText } from "../workspace/ledgerSchedules";
import { skipperFace } from "../workspace/skipperFace";
import { treeSections } from "../workspace/tree";
import { loadSchedules, scheduleCall, schedules, targetProject, type Schedule } from "../workspace/scheduleStore";
import { sectionHead } from "./rowTable";
import { fileSize } from "./taskTypes";
import { ALL_DAYS, every, fromCron, HOURS, hhmm, MINUTES, MON_FRI, parseTime, toCron, WEEK, weekOf, withEvery, type When } from "./whenSentence";

/** The fields of a schedule that the edit page sets. `owner`: the Skipper's owner key. */
interface Draft {
  name: string;
  cron: string;
  owner: string;
}

interface Preview {
  cron: string;
  next: string[];
  error: string | null;
}

/** What survives a redraw of the page. */
export interface SchedulesState {
  /** The schedule on the edit page: its uid, `new`, or null for the list. */
  editing: string | null;
  draft: Draft | null;
  error: string | null;
  /** The When row's own error line: a bad time or a bad cron. */
  whenError: string | null;
  preview: Preview | null;
  /** The Prompt row's one error line, such as no app for the file. */
  promptError: string | null;
  /** The edit page's change in flight: Run now waits for it. */
  saving: Promise<void> | null;
}

export const schedulesState = (): SchedulesState => ({ editing: null, draft: null, error: null, whenError: null, preview: null, promptError: null, saving: null });

export interface SchedulesCtx {
  render: () => void;
  /** The grand Skipper's name: Admiral, or the name Settings gives it. */
  mainName: () => string;
  /** Opens the schedule's prompt file as Task types › Edit opens a type's. Answers the error line, or null. */
  editPrompt: (s: Schedule) => Promise<string | null>;
}

const empty = (): Draft => ({ name: "", cron: "0 9 * * *", owner: "grand" });

/** Runs a change, then redraws. A refusal shows as the page's error line. */
async function run(st: SchedulesState, ctx: SchedulesCtx, args: string[]): Promise<boolean> {
  try {
    await scheduleCall(args);
    st.error = null;
    return true;
  } catch (e) {
    st.error = String(e);
    return false;
  } finally {
    ctx.render();
  }
}

export function schedulesPage(st: SchedulesState, ctx: SchedulesCtx): HTMLElement {
  const main = h("div", "set-main");
  const head = h("div", "set-head");
  main.append(head);
  if (!hostCan("schedules")) {
    head.append(h("div", "set-h", t("settings.tab.schedules")));
    main.append(h("div", "set-note", t("schedules.notOnHost")));
    return main;
  }
  const s = st.editing && st.editing !== "new" ? schedules.find((x) => x.uid === st.editing) : null;
  if (st.editing && (st.editing === "new" || s)) return editPage(main, head, st, ctx, s ?? null);
  st.editing = null;
  head.append(h("div", "set-h", t("settings.tab.schedules")));
  const acts = h("div", "set-head-acts");
  acts.append(smallButton(t("schedules.add"), () => {
    st.editing = "new";
    st.draft = empty();
    st.error = null;
    st.whenError = null;
    ctx.render();
  }, { go: true }));
  head.append(acts);
  if (st.error) main.append(errorLine(st.error));
  main.append(listTable(st, ctx));
  return main;
}

function errorLine(text: string): HTMLElement {
  const e = h("div", "set-error", text);
  e.setAttribute("role", "alert");
  return e;
}

/** The project's letters in its color. */
function tag(project: string | null): HTMLElement | null {
  const p = project ? S.projects.find((x) => x.name === project) : null;
  if (!p) return null;
  const el = h("span", "lg-tag", p.short);
  el.style.setProperty("--pc", accent(p));
  return el;
}

const TONE: Record<string, string> = { succeeded: "ok", running: "run", failed: "bad", pending: "none", skipped: "none" };

function listTable(st: SchedulesState, ctx: SchedulesCtx): HTMLElement {
  const table = h("div", "set-table g-sched");
  table.setAttribute("role", "table");
  const cols = h("div", "set-row set-cols");
  for (const c of ["", t("schedules.colName"), t("schedules.colWhen"), t("schedules.colTarget"), t("schedules.colNextRun"), t("schedules.colLastRun"), ""]) cols.append(h("span", "", c));
  table.append(cols);
  for (const s of schedules) {
    const row = h("div", "set-row" + (s.paused_at ? " paused" : ""));
    row.dataset.id = s.uid;
    const lead = h("span", "c-lead");
    lead.append(h("i", "sq" + (s.paused_at ? " off" : "")));
    const words = h("span", "c-words");
    words.append(h("span", "c-words-text", s.when));
    if (s.custom) words.append(h("span", "c-cron", s.cron));
    const target = h("span", "c-tgt");
    const tg = tag(targetProject(s));
    if (tg) target.append(tg);
    target.append(h("span", "", targetText(s)));
    const last = h("span", "c-last");
    if (s.last) last.append(h("i", `d ${TONE[s.last]}`), h("span", "", lastWord(s.last)));
    const end = h("span", "c-end c-acts");
    const now = smallButton(t("schedules.runNow"), () => void run(st, ctx, ["run-now", s.name]));
    now.disabled = s.open;
    end.append(
      now,
      smallButton(s.paused_at ? t("schedules.resume") : t("schedules.pause"), () => void run(st, ctx, [s.paused_at ? "resume" : "pause", s.name])),
      smallButton(t("schedules.edit"), () => {
        st.editing = s.uid;
        st.error = null;
        st.whenError = null;
        ctx.render();
      }),
    );
    const rm = smallButton(t("table.remove"), () => void remove(st, ctx, s), { icon: rune("indicators-minus"), danger: true, label: t("schedules.deleteTitle", { name: s.name }) });
    end.append(rm);
    row.append(lead, h("span", "c-name", s.name), words, target, h("span", "c-mono", s.next ?? t("schedules.paused")), last, end);
    table.append(row);
  }
  return table;
}

async function remove(st: SchedulesState, ctx: SchedulesCtx, s: Schedule) {
  const ok = await confirmAction({ title: t("schedules.deleteTitle", { name: s.name }), body: t("schedules.deleteBody"), action: t("schedules.delete") });
  if (!ok) return;
  if (await run(st, ctx, ["rm", s.name])) {
    st.editing = null;
    ctx.render();
  }
}

// ── The edit page ───────────────────────────────────────────────────────────

/** A picker in a sentence: its value as text with a faint underline. A click opens the app menu. */
function pk(label: string, shows: string, entries: () => MenuEntry[], o: { mono?: boolean } = {}): HTMLButtonElement {
  const b = h("button", "pk" + (o.mono ? " mono" : "")) as HTMLButtonElement;
  b.type = "button";
  b.textContent = shows;
  b.setAttribute("aria-label", `${label}: ${shows}`);
  b.setAttribute("aria-haspopup", "menu");
  const open = () => {
    const r = b.getBoundingClientRect();
    b.classList.add("open");
    ctxMenu.open(r.left, r.bottom, label, entries(), () => {
      b.classList.remove("open");
      b.focus();
    });
  };
  b.addEventListener("click", open);
  return b;
}

/** A menu line with a check on the value in use. */
const choice = (label: string, on: boolean, run: () => void, hint?: string): MenuEntry => ({ label, run, hint, ...(on ? { icon: "indicators-check" } : {}) });

const word = (text: string, cls = "sent-w") => h("span", cls, text);

/** One 40px row: a label, the content, and the end column. */
function row(label: string, content: Node, end?: Node, cls = ""): HTMLElement {
  const r = h("div", ("set-row " + cls).trim());
  const c = h("span", "c-field");
  c.append(content);
  const e = h("span", "c-end-note");
  if (end) e.append(end);
  r.append(h("span", "c-label", label), c, e);
  return r;
}

const EVERY_LABEL = (v: string): string => {
  const n = /^(\d+)([mh])$/.exec(v);
  if (n) return t(n[2] === "m" ? "schedules.everyMin" : "schedules.everyHour", { n: n[1] });
  return t(`schedules.every.${v}` as MessageKey);
};

/** The days menu: a click checks or clears a day, and the menu opens again in place once the Host has the change. */
function daysEntries(days: number, set: (d: number) => Promise<void>): MenuEntry[] {
  const reopen = () => document.querySelector<HTMLButtonElement>('#settings .pk[data-pick="days"]')?.click();
  const flip = (d: number) => () => {
    const next = days ^ d;
    // A week keeps one day at least.
    if (next) void set(next).then(() => requestAnimationFrame(reopen));
    else requestAnimationFrame(reopen);
  };
  return [
    ...WEEK.map(([n, name]) => choice(name, !!(days & (1 << n)), flip(1 << n))),
    { sep: true },
    choice(t("schedules.monFri"), days === MON_FRI, () => void set(MON_FRI)),
    choice(t("schedules.everyDay"), days === ALL_DAYS, () => void set(ALL_DAYS)),
  ];
}

/** The When sentence. `save` stores a new cron line. */
function whenSentence(st: SchedulesState, cron: string, save: (cron: string) => Promise<void>): HTMLElement {
  const w = fromCron(cron);
  const sent = h("div", "sent");
  const put = (next: When) => {
    st.whenError = null;
    return save(toCron(next));
  };
  const firstEntries = (): MenuEntry[] => {
    const now = every(w);
    const item = (v: string) => choice(EVERY_LABEL(v), now === v, () => put(withEvery(w, v)));
    return [
      ...["day", "weekday", "week", "month"].map(item),
      { sep: true },
      ...MINUTES.map((m) => item(`${m}m`)),
      ...HOURS.map((x) => item(`${x}h`)),
      { sep: true },
      item("custom"),
    ];
  };
  const first = pk(t("schedules.when"), EVERY_LABEL(every(w)), firstEntries);
  /** The typed time: checked when the field is left. */
  const time = (at: [number, number], set: (at: [number, number]) => When) => {
    const f = h("input", "sent-time" + (st.whenError ? " bad" : "")) as HTMLInputElement;
    f.type = "text";
    f.value = hhmm(at);
    f.spellcheck = false;
    f.setAttribute("aria-label", t("schedules.time"));
    f.addEventListener("keydown", (e) => {
      if (e.key === "Enter") f.blur();
      if (e.key === "Escape") {
        f.value = hhmm(at);
        f.blur();
      }
    });
    f.addEventListener("blur", () => {
      if (f.value.trim() === hhmm(at)) return;
      const next = parseTime(f.value);
      if (!next) {
        st.whenError = t("schedules.badTime");
        f.classList.add("bad");
        st.preview = null;
        return void sentErr(sent, st.whenError);
      }
      put(set(next));
    });
    return f;
  };
  if (w.kind === "custom") {
    sent.append(first);
    const f = h("input", "sent-cron" + (st.whenError ? " bad" : "")) as HTMLInputElement;
    f.type = "text";
    f.value = w.cron;
    f.spellcheck = false;
    f.setAttribute("aria-label", t("schedules.cron"));
    f.addEventListener("keydown", (e) => {
      if (e.key === "Enter") f.blur();
    });
    f.addEventListener("blur", () => {
      const v = f.value.trim().split(/\s+/).join(" ");
      if (v !== w.cron) void save(v);
    });
    sent.append(f);
    return sent;
  }
  sent.append(word(t("schedules.everyWord")), first);
  if (w.kind === "day" || w.kind === "weekday") sent.append(word(t("schedules.at")), time(w.at, (at) => ({ ...w, at })));
  if (w.kind === "week") {
    const days = pk(t("schedules.days"), weekWords(w.days), () => daysEntries(w.days, (d) => put(weekOf(d, w.at))));
    days.dataset.pick = "days";
    sent.append(word(t("schedules.on")), days, word(t("schedules.at")), time(w.at, (at) => ({ ...w, at })));
  }
  if (w.kind === "month") {
    const day = pk(t("schedules.dayOfMonth"), String(w.day), () => Array.from({ length: 28 }, (_, i) => choice(String(i + 1), w.day === i + 1, () => put({ ...w, day: i + 1 }))), { mono: true });
    sent.append(word(t("schedules.onDay")), day, word(t("schedules.at")), time(w.at, (at) => ({ ...w, at })));
  }
  if (w.kind === "interval") {
    const hour = (n: number) => hhmm([n, 0]);
    const all = (): MenuEntry => choice(t("schedules.allDay"), !w.window, () => put({ ...w, window: null }));
    if (!w.window) {
      sent.append(pk(t("schedules.window"), t("schedules.allDay"), () => [all(), { sep: true }, ...Array.from({ length: 24 }, (_, n) => choice(t("schedules.fromHour", { at: hour(n) }), false, () => put({ ...w, window: [n, Math.min(n + 9, 24)] })))]));
    } else {
      const [a, z] = w.window;
      sent.append(
        word(t("schedules.from")),
        pk(t("schedules.from"), hour(a), () => [all(), { sep: true }, ...Array.from({ length: z }, (_, n) => choice(hour(n), n === a, () => put({ ...w, window: [n, z] })))], { mono: true }),
        word(t("schedules.to")),
        pk(t("schedules.to"), hour(z), () => Array.from({ length: 24 - a }, (_, i) => a + 1 + i).map((n) => choice(hour(n), n === z, () => put({ ...w, window: [a, n] }))), { mono: true }),
      );
    }
    const days = pk(t("schedules.days"), weekWords(w.days), () => daysEntries(w.days, (d) => put({ ...w, days: d })));
    days.dataset.pick = "days";
    sent.append(word(t("schedules.on")), days);
  }
  return sent;
}

/** `Mon, Thu`, `Mon–Fri`, `Mon–Sun`. */
function weekWords(days: number): string {
  if (days === ALL_DAYS) return t("schedules.monSun");
  if (days === MON_FRI) return t("schedules.monFri");
  return WEEK.filter(([n]) => days & (1 << n)).map(([, s]) => s).join(", ");
}

/** Puts the When row's error line in its end column, with no redraw. */
function sentErr(sent: HTMLElement, text: string) {
  const end = sent.closest(".set-row")?.querySelector(".c-end-note");
  if (end) end.replaceChildren(h("span", "c-err", text));
}

/** The When row's end column: the next 3 runs, or the one error line. */
function whenEnd(st: SchedulesState, ctx: SchedulesCtx, cron: string): HTMLElement {
  const end = h("span", "nexts");
  if (st.whenError) {
    end.append(h("span", "c-err", st.whenError));
    return end;
  }
  const p = st.preview;
  if (!p || p.cron !== cron) {
    void hostInvoke<{ next: string[]; error: string | null }>("schedules", "cron_preview", { cron }).then((r) => {
      st.preview = { cron, next: r.next, error: r.error };
      ctx.render();
    }).catch(() => {});
    return end;
  }
  if (p.error) end.append(h("span", "c-err", p.error));
  else end.append(h("b", "", t("schedules.next")), ...p.next.map((n) => h("span", "", n)));
  return end;
}

function editPage(main: HTMLElement, head: HTMLElement, st: SchedulesState, ctx: SchedulesCtx, s: Schedule | null) {
  const d: Draft = s ? { name: s.name, cron: s.cron, owner: skipperScope(s.owner ?? "grand") } : (st.draft ??= empty());
  const back = h("button", "set-back", `‹ ${t("settings.tab.schedules")}`) as HTMLButtonElement;
  back.type = "button";
  back.addEventListener("click", () => {
    st.editing = null;
    st.draft = null;
    st.error = null;
    st.whenError = null;
    ctx.render();
  });
  head.append(back, h("div", "set-h", s ? s.name : t("schedules.new")));
  if (s) {
    const acts = h("div", "set-head-acts");
    // Run now saves the field in edit, as leaving the page does, then starts
    // the run and goes back to the list, which shows it. A refusal stays here.
    const now = smallButton(t("schedules.runNow"), async () => {
      (document.activeElement as HTMLElement | null)?.blur();
      await st.saving;
      const name = schedules.find((x) => x.uid === s.uid)?.name ?? s.name;
      if (!(await run(st, ctx, ["run-now", name]))) return;
      st.editing = null;
      st.draft = null;
      st.whenError = null;
      ctx.render();
    });
    now.disabled = s.open;
    acts.append(
      now,
      smallButton(s.paused_at ? t("schedules.resume") : t("schedules.pause"), () => void run(st, ctx, [s.paused_at ? "resume" : "pause", s.name])),
      smallButton(t("schedules.deleteDots"), () => void remove(st, ctx, s), { danger: true }),
    );
    head.append(acts);
  }

  /** One change: `edit` on a saved schedule; on a new one, `add` once Name and When are valid. */
  const save = (flags: string[], apply: (d: Draft) => void): Promise<void> => {
    const p = change(flags, apply);
    st.saving = p;
    return p.finally(() => {
      if (st.saving === p) st.saving = null;
    });
  };
  const change = async (flags: string[], apply: (d: Draft) => void) => {
    apply(d);
    if (s) {
      // A project in a fleet shows as its fleet's Skipper: the next save stores the fleet.
      const owner = s.target === "skipper" && d.owner !== s.owner && !flags.includes("--skipper") ? ["--skipper", d.owner] : [];
      await run(st, ctx, ["edit", s.name, ...flags, ...owner]);
      // A rename keeps the page on the same schedule: it goes by uid.
      return;
    }
    st.draft = d;
    if (!d.name) return ctx.render();
    const args = ["add", d.name, "--cron", d.cron, "--to", "skipper", "--skipper", d.owner];
    if (await run(st, ctx, args)) {
      st.editing = schedules.find((x) => x.name === d.name)?.uid ?? null;
      st.draft = null;
      ctx.render();
    }
  };

  if (st.error) main.append(errorLine(st.error));
  // When.
  main.append(sectionHead(t("schedules.bandWhen")));
  main.append(row(t("schedules.colName"), textBox({ label: t("schedules.colName"), value: d.name, name: true, save: (v) => void save(["--name", v], (x) => void (x.name = v)) })));
  const when = whenSentence(st, d.cron, (cron) => save(["--cron", cron], (x) => void (x.cron = cron)));
  main.append(row(t("schedules.when"), when, whenEnd(st, ctx, d.cron), "sent-row"));

  // Agent step.
  main.append(sectionHead(t("schedules.bandAgent")));
  if (s?.command) main.append(commandRow(s));
  // A task type, a pane or no agent step comes only from `skiff schedule`: it shows as it is.
  if (s && s.target !== "skipper") main.append(agentRow(s));
  else main.append(row(t("schedules.skipperScope"), skipperPicker(d, ctx, save), undefined, "sent-row"));
  if (!s || s.target) main.append(promptRow(st, ctx, s));
  return main;
}

/** A command from `skiff schedule`: read-only, with its gate and time limit, so nothing is hidden. */
function commandRow(s: Schedule): HTMLElement {
  const gate = s.target ? t(`schedules.gate.${s.gate ?? "always"}` as MessageKey) : null;
  const end = [gate, t("schedules.seconds", { n: String(s.timeout ?? 600) })].filter(Boolean).join(" · ");
  return row(t("schedules.command"), h("span", "c-ro mono", s.command ?? ""), h("span", "", end), "ro");
}

/** The prompt file, as Task types shows a type's instructions: its size or none, and Edit. */
function promptRow(st: SchedulesState, ctx: SchedulesCtx, s: Schedule | null): HTMLElement {
  const none = !s || s.prompt == null;
  const state = h("span", "c-state" + (none ? " none" : ""));
  state.append(h("span", "sq"), none ? t("tasks.none") : fileSize(s!.prompt!));
  if (s) state.append(h("span", "c-file", s.prompt_file));
  const end = h("span", "c-acts");
  if (st.promptError) end.append(h("span", "c-err", st.promptError));
  const edit = smallButton(t("skipper.edit"), () => {
    if (!s) return;
    st.promptError = null;
    void ctx.editPrompt(s).then((e) => {
      st.promptError = e;
      return loadSchedulesPage(ctx);
    });
  }, { label: t("schedules.editPrompt", { name: s?.name ?? "" }) });
  // A new schedule has no file until it saves.
  edit.disabled = !s;
  end.append(edit);
  return row(t("schedules.prompt"), state, end, "prompt-row");
}

/** An agent step from `skiff schedule` that is not a Skipper's: read-only, with its project. */
function agentRow(s: Schedule): HTMLElement {
  const c = h("span", "c-ro agent");
  const tg = tag(s.target === "type" ? s.project : null);
  if (tg) c.append(tg);
  c.append(h("span", "", targetText(s)));
  return row(t("schedules.agent"), c, undefined, "ro");
}

/** The Skipper picker: one list of every Skipper, running or not. Fire starts it. */
function skipperPicker(d: Draft, ctx: SchedulesCtx, save: (flags: string[], apply: (d: Draft) => void) => Promise<void>): HTMLElement {
  const sent = h("div", "sent");
  const o = d.owner;
  const skippers = skipperChoices(ctx);
  const shows = skippers.find((x) => x.scope === o)?.name ?? skipperName(ctx, o);
  // The face takes the check's place: the current Skipper gets the bar.
  const item = (x: SkipperChoice): MenuEntry => ({ label: x.name, current: x.scope === o, glyph: skipperFace(x.scope === "grand" ? null : x.scope, 16), run: () => void save(["--skipper", x.arg], (y) => void (y.owner = x.scope)) });
  const groups = [skippers.filter((x) => x.scope === "grand"), skippers.filter((x) => x.scope.startsWith("fleet:")), skippers.filter((x) => x.scope !== "grand" && !x.scope.startsWith("fleet:"))].filter((g) => g.length);
  sent.append(pk(t("schedules.skipperScope"), shows, () => groups.flatMap((g, i): MenuEntry[] => [...(i ? [{ sep: true as const }] : []), ...g.map(item)])));
  return sent;
}

/** A Skipper the agent step can go to. `scope`: the owner key the ledger keeps. `arg`: what `--skipper` takes. */
interface SkipperChoice {
  scope: string;
  arg: string;
  name: string;
}

/**
 * The Skippers in tree order: Admiral, each fleet's Skipper, then the
 * Skipper of each project in no fleet. A project in a fleet is not listed:
 * its fleet's Skipper takes its work.
 */
function skipperChoices(ctx: SchedulesCtx): SkipperChoice[] {
  const sections = treeSections();
  const fleets = new Set(sections.flatMap((x) => (x.fleet ? [x.fleet] : [])));
  return [
    { scope: "grand", arg: "grand", name: ctx.mainName() },
    ...sections.flatMap((x) => (x.fleet ? [{ scope: `fleet:${x.fleet}`, arg: `fleet:${x.fleet}`, name: fleetSkipperName(x.fleet) }] : [])),
    // A project that shares a fleet's name needs the prefix.
    ...(sections.find((x) => !x.fleet)?.projects ?? []).map((p) => ({ scope: p.name, arg: fleets.has(p.name) ? `project:${p.name}` : p.name, name: `${p.name} ${skipperWord()}` })),
  ];
}

/** An owner key as the Agent sentence names it. */
function skipperName(ctx: SchedulesCtx, scope: string): string {
  if (scope === "grand") return ctx.mainName();
  return scope.startsWith("fleet:") ? fleetSkipperName(scope.slice(6)) : `${scope} ${skipperWord()}`;
}

/** The owner the page shows: a project in a fleet goes to its fleet's Skipper. */
function skipperScope(owner: string): string {
  const fleet = owner ? S.projects.find((p) => p.name === owner)?.fleet : null;
  return fleet ? `fleet:${fleet}` : owner;
}

/** Reads the schedules for the page, then redraws it. */
export async function loadSchedulesPage(ctx: SchedulesCtx) {
  await loadSchedules();
  ctx.render();
}

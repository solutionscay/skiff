import { h, button } from "../ui/dom";
import { smallButton } from "../ui/controls";
import { t, type MessageKey } from "../i18n";
import { rune } from "../ui/runes";
import { createHostFixes } from "./hostFixes";
import { type Check, type CheckLine, type Fix, type HostRow, type HostsEnv, type HostsState, type Report, type RowResult, CHECK_ROWS, FIX_LABEL, alert } from "./hostShared";

/** The catalog's words for a row title from Rust. One Rust has not named here shows as it came. */
const ROW_WORDS: MessageKey[] = ["hosts.row.reach", "hosts.row.hostKey", "hosts.row.login", "hosts.row.supported", "hosts.row.skiff", "hosts.row.sessions", "hosts.row.socket", "hosts.row.identity", "hosts.row.projects", "hosts.row.linger"];
const rowTitle = (title: string) => {
  const at = CHECK_ROWS.indexOf(title);
  return at < 0 ? title : t(ROW_WORDS[at]);
};

/** A Host's check list: the run, its rows and the fix buttons. The fix dialogs live in hostFixes. */
export function createHostCheck(env: HostsEnv, st: HostsState, deps: { reload(): Promise<void> }) {
  /** Runs the read-only check list of a Host and shows it. */
  /** `result`: what a run just did, kept on its row through this check. */
  async function runCheck(id: string, result?: RowResult) {
    st.form = null;
    const c: Check = { id, report: null, error: null, result };
    st.check = c;
    env.redraw();
    try {
      c.report = await env.ask<Report>("check_host", { id });
      await env.hosts.loadHosts();
      await deps.reload();
    } catch (e) {
      c.error = String(e);
    }
    if (st.check === c) env.redraw();
  }

  /** Update Skiff… from the list: the check list first, for the version
   *  and platform the dialog names, then the dialog. `reinstall`: this
   *  app's build goes in even when the server has it. */
  async function checkThenUpdate(id: string, reinstall = false) {
    await runCheck(id);
    const x = env.hosts.knownHosts().find((k) => k.id === id);
    if (x && st.check?.id === id && st.check.report) fixes.updateSkiff(st.check, x, reinstall);
  }

  function drawCheck(c: Check): HTMLElement {
    const x = env.hosts.knownHosts().find((k) => k.id === c.id);
    const name = x?.name ?? t("hosts.thisHost");
    const main = h("div", "set-main");
    const head = h("div", "set-head");
    head.append(
      button("set-back", t("hosts.back"), () => ((st.check = null), env.redraw())),
      h("div", "set-h", t("hosts.checkTitle", { name })),
    );
    main.append(head);
    const table = h("div", "set-table g-check");
    table.setAttribute("role", "table");
    table.setAttribute("aria-label", t("hosts.checkTitle", { name }));
    const rows: CheckLine[] = c.report?.rows ?? CHECK_ROWS.map((title, i) => ({ row: String(i), title, state: "skipped", text: t(i === 0 ? "hosts.checking" : "hosts.waiting"), fix: null }));
    for (const r of rows) {
      // The row a run just changed shows what the run did, until the next check.
      const res = c.result && r.title === CHECK_ROWS[4] ? c.result : null;
      const state = res ? (res.failed ? "failed" : "passed") : c.report ? r.state : "waiting";
      const row = h("div", `set-row set-check ${state}`);
      row.setAttribute("role", "row");
      const mark = h("span", "c-lead");
      if (state === "passed") mark.append(rune("indicators-check"));
      else mark.append(h("span", "sq"));
      mark.setAttribute("aria-label", state);
      const text = h("span", "c-text", res ? res.text : r.text);
      if (res?.secs !== undefined && !res.failed) text.append(h("span", "c-secs", `${res.secs} s`));
      const fix = res ? res.fix : c.report && (r.state === "failed" || r.state === "warning") ? r.fix : null;
      if (fix && x) text.append(smallButton(res?.failed ? t("hosts.tryAgain") : t(FIX_LABEL[fix]), () => void runFix(c, x, fix), { go: true }));
      if (res?.output) {
        const out = res.output;
        text.append(smallButton(t("hosts.copyOutput"), () => void env.copyText(out), { go: true }));
      }
      row.append(mark, h("span", "c-name", rowTitle(r.title)), text);
      table.append(row);
    }
    main.append(table);
    if (c.error) main.append(alert(c.error));
    if (c.note) main.append(h("div", "set-note set-line", c.note));
    const acts = h("div", "set-form-acts");
    const running = !c.report && !c.error;
    const again = button("set-form-act", t(running ? "hosts.checking" : "hosts.checkAgain"), () => void runCheck(c.id));
    again.disabled = running;
    acts.append(again, h("span", "set-fill"));
    if (c.report?.ok && x && env.hosts.shownHost() !== x.id) acts.append(button("set-form-go", t("hosts.switchTo", { name }), () => env.hosts.pick(x.id)));
    acts.append(button("set-form-act", t("hosts.done"), () => ((st.check = null), env.redraw())));
    main.append(acts);
    return main;
  }

  /** One fix, after its confirm, then the check again. */
  async function runFix(c: Check, x: HostRow, fix: Fix) {
    const again = () => void runCheck(x.id);
    c.note = undefined;
    try {
      if (fix === "trust_key" || fix === "review_key") return await fixes.trustKey(x, again);
      if (fix === "make_key") return fixes.makeKey(x, again);
      if (fix === "unlock_key") return fixes.secretDialog(x, "unlock", again);
      if (fix === "install_key") return fixes.secretDialog(x, "install", again);
      if (fix === "fix_permissions") return fixes.secretDialog(x, "fix", again);
      if (fix === "install_skiff") return fixes.installSkiff(c, x);
      if (fix === "start_skiff") {
        c.note = t("hosts.starting", { name: x.name });
        env.redraw();
        await env.ask("start_skiff", { id: x.id });
        return again();
      }
      if (fix === "turn_on_linger") return fixes.turnOnLinger(x, again);
      if (fix === "update_skiff") return fixes.updateSkiff(c, x);
      if (fix === "copy_public_key") {
        await env.ask("show_public_key", { id: x.id });
      }
    } catch (e) {
      c.error = String(e);
      env.redraw();
    }
  }


  const fixes = createHostFixes(env, st, { runCheck });

  return { runCheck, checkThenUpdate, drawCheck };
}

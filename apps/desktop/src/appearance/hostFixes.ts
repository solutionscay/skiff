import { t, tn } from "../i18n";
import { h } from "../ui/dom";
import { confirmAction } from "../ui/confirm";
import { actionDialog } from "../ui/dialogParts";
import { type Check, type Fingerprint, type Fix, type HostKey, type HostRow, type HostsEnv, type HostsState, type RowResult, type Updated } from "./hostShared";

/** The dialogs of the check list's fixes: install, update, trust a key, make
 *  a key, a secret field. Each asks first, then checks again. */
export function createHostFixes(env: HostsEnv, st: HostsState, deps: { runCheck(id: string, result?: RowResult): Promise<void> }) {
  /** One confirm covers the install, the start and keeping it running.
   *  The dialog stays open while it runs and shows each step. */
  function installSkiff(c: Check, x: HostRow) {
    const [os] = c.report?.platform ?? ["linux", ""];
    const v = c.report?.version ?? "";
    env.runDialog<{ version: string; cli_linked: boolean }>({
      id: x.id,
      title: t("hosts.install.title", { name: x.name }),
      body:
        t(os === "macos" ? "hosts.install.bodyMac" : "hosts.install.body", { version: v, name: x.name }),
      action: t("hosts.install.action"),
      runningTitle: t("hosts.install.running", { name: x.name }),
      working: t("hosts.install.working"),
      failTitle: t("hosts.install.failed", { name: x.name }),
      steps: runSteps(x, v, t("hosts.step.startBoth")),
      tone: "go",
      run: () => env.ask("install_skiff", { id: x.id }),
      done: (r, secs, output) => {
        const cli = r.cli_linked ? "" : ` ${t("hosts.install.cliTaken")}`;
        void deps.runCheck(x.id, { row: "skiff", text: `${t("hosts.install.done", { version: r.version })}${cli}`, secs, output });
      },
      failed: (cause, output) => rowFailed(c, "install_skiff", cause, output),
    });
  }

  /** The six steps an install or update counts, before each runs. */
  const runSteps = (x: HostRow, v: string, last: string) => [t("hosts.checkTitle", { name: x.name }), t("hosts.step.get", { version: v }), t("hosts.step.upload"), t("hosts.step.unpack"), t("hosts.step.switch", { version: v }), last];

  /** Close on a failed run: the check list row keeps the cause, with Try
   *  again and Copy output. */
  function rowFailed(c: Check, fix: Fix, cause: string, output: string) {
    if (st.check !== c) return;
    c.result = { row: "skiff", text: cause, failed: true, fix, output };
    env.redraw();
  }
  /** Update Skiff…: one confirm, open while it runs with each step. When
   *  skiffd cannot keep its sessions, the update stops, and a red dialog
   *  names the sessions it would end. */
  function updateSkiff(c: Check, x: HostRow, reinstall = false) {
    updateDialog(c, x, {
      title: t("hosts.update.title", { name: x.name }),
      body: t(reinstall || c.report?.other_build ? "hosts.update.bodyBuild" : "hosts.update.body", { name: x.name, version: c.report?.version ?? "" }),
      action: t("hosts.update.action"),
      tone: "go",
      endSessions: false,
      reinstall,
    });
  }

  function updateDialog(
    c: Check,
    x: HostRow,
    o: { title: string; body: string; action: string; tone: "go" | "danger"; endSessions: boolean; reinstall: boolean },
  ) {
    const v = c.report?.version ?? "";
    env.runDialog<Updated>({
      id: x.id,
      ...o,
      runningTitle: t("hosts.update.running", { name: x.name }),
      working: t("hosts.update.working"),
      failTitle: t("hosts.update.failed", { name: x.name }),
      steps: runSteps(x, v, t("hosts.step.startHost")),
      run: () => env.ask<Updated>("update_skiff", { id: x.id, endSessions: o.endSessions, reinstall: o.reinstall }),
      done: (r, secs, output) => {
        if (r.result === "no_handover") return endsSessions(c, x, r.from, r.sessions, o.reinstall);
        const text =
          r.result === "done"
            ? `${t("hosts.update.done", { version: r.version })}${r.restarted ? ` ${t("hosts.update.restarted")}` : ""}`
            : r.result === "newer"
              ? t("hosts.update.newer", { name: x.name, version: r.from })
              : t("hosts.update.same", { version: r.version });
        void deps.runCheck(x.id, { row: "skiff", text, secs, output });
      },
      failed: (cause, output) => rowFailed(c, "update_skiff", cause, output),
    });
  }

  /** The red dialog: this update ends the sessions it names. */
  function endsSessions(c: Check, x: HostRow, from: string, sessions: string[], reinstall: boolean) {
    const n = sessions.length;
    updateDialog(c, x, {
      title: tn("hosts.update.endTitle", n, { name: x.name }),
      body: t("hosts.update.endBody", { version: from, names: sessions.join(", ") }),
      action: tn("hosts.update.endAction", n),
      tone: "danger",
      endSessions: true,
      reinstall,
    });
  }

  function turnOnLinger(x: HostRow, done: () => void) {
    actionDialog({
      title: t("hosts.linger.title", { name: x.name }),
      body: t("hosts.linger.body"),
      action: t("hosts.linger.action"),
      tone: "go",
      submit: async () => {
        await env.ask<boolean>("turn_on_linger", { id: x.id });
        done();
      },
    });
  }

  /** Trust a new host key, or replace a changed one: the fingerprint first. */
  async function trustKey(x: HostRow, done: () => void) {
    const key = await env.ask<HostKey>("host_key_scan", { id: x.id });
    const prints = (title: string, fs: Fingerprint[], old = false) => {
      const box = h("div", "confirm-body set-prints" + (old ? " old" : ""));
      box.append(h("div", "set-prints-h", title), ...fs.map((f) => h("div", "mono", `${f.kind}  ${f.sha256}`)));
      return box;
    };
    if (!key.known.length) {
      return actionDialog({
        title: t("hosts.trust.title", { name: x.name }),
        body: t("hosts.trust.body", { name: x.name }),
        content: [prints(t("hosts.trust.print"), key.fingerprints)],
        action: t("hosts.trust.action"),
        tone: "go",
        submit: async () => {
          await env.ask("host_key_trust", { token: key.token, replace: false });
          done();
        },
      });
    }
    actionDialog({
      title: t("hosts.changed.title", { name: x.name }),
      body: t("hosts.changed.body"),
      content: [prints(t("hosts.changed.known"), key.known, true), prints(t("hosts.changed.now"), key.fingerprints)],
      action: t("hosts.changed.action"),
      tone: "danger",
      submit: async () => {
        // Asked a second time, as a changed key can be an attack.
        setTimeout(async () => {
          const ok = await confirmAction({
            title: t("hosts.replace.title", { name: x.name }),
            body: t("hosts.replace.body", { file: key.file }),
            action: t("hosts.replace.action"),
          });
          if (!ok) return;
          await env.ask("host_key_trust", { token: key.token, replace: true }).catch((e) => alertCheck(e));
          done();
        });
      },
    });
  }

  function alertCheck(e: unknown) {
    if (st.check) st.check.error = String(e);
    env.redraw();
  }

  function makeKey(x: HostRow, done: () => void) {
    actionDialog({
      title: t("hosts.make.title"),
      body: t("hosts.make.body", { name: x.name }),
      action: t("hosts.make.action"),
      tone: "go",
      submit: async () => {
        await env.ask("make_key");
        // The new key goes to the server next, with one password.
        setTimeout(() => secretDialog(x, "install", done));
      },
    });
  }

  /** A dialog with one secret field. The field is cleared when it closes. */
  function secretDialog(x: HostRow, what: "install" | "unlock" | "fix", done: () => void) {
    const input = h("input", "prompt-input");
    input.type = "password";
    input.autocomplete = "off";
    input.spellcheck = false;
    const user = x.user ?? (x.destination.includes("@") ? x.destination.split("@")[0] : "");
    const who = user ? t("hosts.secret.who", { user, name: x.name }) : t("hosts.secret.yours", { name: x.name });
    const copy = {
      install: {
        title: t("hosts.secret.installTitle", { name: x.name }),
        body: t("hosts.secret.installBody", { who }),
        label: t("hosts.secret.password"),
        action: t("hosts.secret.installAction"),
        cmd: "install_key",
      },
      unlock: {
        title: t("hosts.secret.unlockTitle"),
        body: t("hosts.secret.unlockBody"),
        label: t("hosts.secret.passphrase"),
        action: t("hosts.secret.unlockAction"),
        cmd: "unlock_key",
      },
      fix: {
        title: t("hosts.secret.fixTitle", { name: x.name }),
        body: t("hosts.secret.fixBody", { name: x.name, who }),
        label: t("hosts.secret.password"),
        action: t("hosts.secret.fixAction"),
        cmd: "fix_permissions",
      },
    }[what];
    input.placeholder = copy.label;
    input.setAttribute("aria-label", copy.label);
    actionDialog({
      title: copy.title,
      body: copy.body,
      content: [input],
      action: copy.action,
      tone: what === "fix" ? "danger" : "go",
      focus: input,
      onClose: () => (input.value = ""),
      submit: async () => {
        if (!input.value) {
          input.focus();
          throw t("dialog.typeField", { field: copy.label });
        }
        const secret = input.value;
        input.value = "";
        const args = what === "unlock" ? { id: x.id, passphrase: secret } : { id: x.id, password: secret };
        try {
          await env.ask(copy.cmd, args);
        } catch (e) {
          input.focus();
          throw String(e);
        }
        done();
      },
    });
  }

  return { installSkiff, updateSkiff, turnOnLinger, trustKey, makeKey, secretDialog, alertCheck };
}

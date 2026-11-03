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
      title: `Install Skiff on ${x.name}?`,
      body:
        os === "macos"
          ? `Installs Skiff ${v} in your home folder on ${x.name}. It starts when you log in there.`
          : `Installs Skiff ${v} in your home folder on ${x.name}. It keeps running after a restart.`,
      action: "Install Skiff",
      runningTitle: `Installing Skiff on ${x.name}`,
      working: "Installing",
      failTitle: `Install failed on ${x.name}`,
      steps: runSteps(x, v, "Start skiffd and skiff-host"),
      tone: "go",
      run: () => env.ask("install_skiff", { id: x.id }),
      done: (r, secs, output) => {
        const cli = r.cli_linked ? "" : " ~/.local/bin/skiff is taken, so Skiff left it.";
        void deps.runCheck(x.id, { row: "skiff", text: `Installed Skiff ${r.version}.${cli}`, secs, output });
      },
      failed: (cause, output) => rowFailed(c, "install_skiff", cause, output),
    });
  }

  /** The six steps an install or update counts, before each runs. */
  const runSteps = (x: HostRow, v: string, last: string) => [`Check ${x.name}`, `Get Skiff ${v}`, "Upload", "Unpack", `Switch to Skiff ${v}`, last];

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
      title: `Update Skiff on ${x.name}?`,
      body: `Updates Skiff on ${x.name} to ${c.report?.version ?? ""}${reinstall || c.report?.other_build ? ", this app's build" : ""}. Its sessions keep running.`,
      action: "Update Skiff",
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
      runningTitle: `Updating Skiff on ${x.name}`,
      working: "Updating",
      failTitle: `Update failed on ${x.name}`,
      steps: runSteps(x, v, "Start skiff-host"),
      run: () => env.ask<Updated>("update_skiff", { id: x.id, endSessions: o.endSessions, reinstall: o.reinstall }),
      done: (r, secs, output) => {
        if (r.result === "no_handover") return endsSessions(c, x, r.from, r.sessions, o.reinstall);
        const text =
          r.result === "done"
            ? `Updated to Skiff ${r.version}.${r.restarted ? " skiffd restarted." : ""}`
            : r.result === "newer"
              ? `${x.name} runs a newer Skiff, ${r.from}.`
              : `Skiff ${r.version} runs already.`;
        void deps.runCheck(x.id, { row: "skiff", text, secs, output });
      },
      failed: (cause, output) => rowFailed(c, "update_skiff", cause, output),
    });
  }

  /** The red dialog: this update ends the sessions it names. */
  function endsSessions(c: Check, x: HostRow, from: string, sessions: string[], reinstall: boolean) {
    const n = sessions.length;
    updateDialog(c, x, {
      title: `Update Skiff on ${x.name} and end ${n === 1 ? "1 session" : `${n} sessions`}?`,
      body: `Skiff ${from} cannot hand its sessions over. This ends: ${sessions.join(", ")}. Save their work first.`,
      action: n === 1 ? "Update and end 1 session" : `Update and end ${n} sessions`,
      tone: "danger",
      endSessions: true,
      reinstall,
    });
  }

  function turnOnLinger(x: HostRow, done: () => void) {
    actionDialog({
      title: `Keep Skiff running on ${x.name} after you log out?`,
      body: `Turns on lingering for your account. Skiff then keeps running after you log out.`,
      action: "Turn on",
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
        title: `Trust the host key of ${x.name}?`,
        body: `First connection to ${x.name}. Compare the fingerprint with the one from your provider, if you have it.`,
        content: [prints("Fingerprint", key.fingerprints)],
        action: "Trust this key",
        tone: "go",
        submit: async () => {
          await env.ask("host_key_trust", { token: key.token, replace: false });
          done();
        },
      });
    }
    actionDialog({
      title: `The host key of ${x.name} changed`,
      body: `Normal after a server reinstall. It can also mean another machine answers here.`,
      content: [prints("Key that Skiff knows", key.known, true), prints("Key that answers now", key.fingerprints)],
      action: "Replace the key…",
      tone: "danger",
      submit: async () => {
        // Asked a second time, as a changed key can be an attack.
        setTimeout(async () => {
          const ok = await confirmAction({
            title: `Replace the key of ${x.name}?`,
            body: `Skiff replaces the old key in ${key.file}. Do this only if you know why it changed.`,
            action: "Replace the key",
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
      title: "Make an SSH key on this computer?",
      body: `Makes ~/.ssh/id_ed25519 with no passphrase, then installs it on ${x.name}.`,
      action: "Make key",
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
    const who = user ? `${user} on ${x.name}` : `your account on ${x.name}`;
    const copy = {
      install: {
        title: `Log in to ${x.name} once`,
        body: `Type the password of ${who}. Skiff uses it once and does not store it.`,
        label: "Password",
        action: "Install key",
        cmd: "install_key",
      },
      unlock: {
        title: "Unlock your SSH key",
        body: "Type the passphrase of your SSH key. Skiff does not store it.",
        label: "Passphrase",
        action: "Unlock",
        cmd: "unlock_key",
      },
      fix: {
        title: `Replace the key file on ${x.name}?`,
        body: `Another account owns your key file on ${x.name}. Skiff writes a new one with only this computer's key. The other keys stop working. Type the password of ${who}.`,
        label: "Password",
        action: "Replace the key file",
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
          throw `Type the ${copy.label.toLowerCase()}.`;
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

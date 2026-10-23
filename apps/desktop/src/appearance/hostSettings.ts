import { invoke } from "@tauri-apps/api/core";
import { h, button } from "../ui/dom";
import { rune } from "../ui/runes";
import { confirmAction, promptAction } from "../ui/confirm";
import { actionDialog } from "../ui/dialogParts";
import { copyText } from "../platform/clipboard";
import { type HostsPage, type KnownHost, knownHosts, loadHosts, localHostName, pick, shownHost } from "../app/hosts";

/**
 * Settings › Hosts: the known Hosts, the form that adds or edits one, and
 * a Host's check list. The list is this computer's own (`hosts.toml`), so
 * the tab works the same while any Host is shown. The form needs one field,
 * Server. Name, user, port and socket sit under More: ssh config gives the
 * first three, the line under Server says what `ssh -G` makes of them, with
 * no network, and the check finds the socket. Connect saves, then checks.
 * A failed row has at most one fix. Each fix asks first, names what it
 * changes, and then checks again. A password or passphrase goes to Rust,
 * which gives it to ssh through askpass only; the page keeps none.
 */

interface Resolved {
  hostname: string;
  user: string;
  port: number;
  jump: string;
}

interface Alias {
  name: string;
  goes_to: string;
}

type Draft = { destination: string; name: string; user: string; port: string; socket: string };

type Form = { id: string | null; draft: Draft; more: boolean; error: string | null; busy: boolean };

/** One row of the check list, as Rust's `check_host` answers it. */
interface CheckLine {
  row: string;
  title: string;
  state: "passed" | "failed" | "skipped" | "warning";
  text: string;
  fix: Fix | null;
}
type Fix = "trust_key" | "review_key" | "make_key" | "unlock_key" | "install_key" | "copy_public_key" | "fix_permissions" | "install_skiff" | "start_skiff" | "turn_on_linger" | "update_skiff";
const FIX_LABEL: Record<Fix, string> = {
  trust_key: "Trust this key…",
  review_key: "Review…",
  make_key: "Make key…",
  unlock_key: "Unlock key…",
  install_key: "Install key…",
  copy_public_key: "Copy public key",
  fix_permissions: "Fix permissions…",
  install_skiff: "Install Skiff…",
  start_skiff: "Start",
  turn_on_linger: "Turn on…",
  update_skiff: "Update Skiff…",
};
/** What `update_skiff` did. */
type Updated = { version: string; source: string } & (
  | { result: "done"; from: string; restarted: boolean }
  | { result: "same" }
  | { result: "newer"; version: string }
  | { result: "no_handover"; from: string; sessions: string[] }
);
interface Fingerprint {
  kind: string;
  sha256: string;
}
interface HostKey {
  token: string;
  fingerprints: Fingerprint[];
  known: Fingerprint[];
  file: string;
}
interface Report {
  rows: CheckLine[];
  ok: boolean;
  /** The server's OS and arch as bundle names write them. */
  platform: [string, string] | null;
  /** The Skiff version this app installs. */
  version: string;
}
/** The rows, in order, before an answer. */
const CHECK_ROWS = [
  "Reach the server",
  "Host key",
  "Log in without a prompt",
  "Server supported",
  "Skiff on the server",
  "Sessions service (skiffd)",
  "Socket forwarding",
  "Skiff identity and version",
  "Projects service (skiff-host)",
  "Keeps running after logout",
];

type Check = { id: string; report: Report | null; error: string | null; note?: string };

export function createHostsTab(redraw: () => void) {
  let form: Form | null = null;
  let check: Check | null = null;
  let error: string | null = null;
  let aliases: Alias[] = [];
  /** `ssh -G` per Host id, for the Server column. */
  const resolved = new Map<string, Resolved>();
  /** The form's resolve line, and the input it answers. */
  let line: { for: string; text: string; bad: boolean } = { for: "", text: "", bad: false };
  let timer: ReturnType<typeof setTimeout> | undefined;

  const server = (r: Resolved) => `${r.user ? `${r.user}@` : ""}${r.hostname}${r.port !== 22 ? `:${r.port}` : ""}`;
  const typed = (x: KnownHost) => `${x.user ? `${x.user}@` : ""}${x.destination}${x.port ? `:${x.port}` : ""}`;
  const resolve = (destination: string, user: string, port: string) =>
    invoke<Resolved>("resolve_ssh", { destination, user: user ?? "", port: port ?? "" });

  /** Reads ssh config and resolves each Host. Local only. */
  async function load() {
    aliases = await invoke<Alias[]>("ssh_aliases").catch(() => []);
    await Promise.all(
      knownHosts().map(async (x) => {
        const r = await resolve(x.destination, x.user ?? "", x.port ? String(x.port) : "").catch(() => null);
        if (r) resolved.set(x.id, r);
        else resolved.delete(x.id);
      }),
    );
  }

  function open(page: HostsPage) {
    error = null;
    form = "add" in page ? newForm(null) : null;
    check = null;
    if ("check" in page) void runCheck(page.check);
  }

  /** Runs the read-only check list of a Host and shows it. */
  async function runCheck(id: string) {
    form = null;
    const c: Check = { id, report: null, error: null };
    check = c;
    redraw();
    try {
      c.report = await invoke<Report>("check_host", { id });
      await loadHosts();
      await load();
    } catch (e) {
      c.error = String(e);
    }
    if (check === c) redraw();
  }

  function newForm(x: KnownHost | null): Form {
    const draft: Draft = x
      ? { destination: x.destination, name: x.name, user: x.user ?? "", port: x.port ? String(x.port) : "", socket: x.socket_override ? x.socket ?? "" : "" }
      : { destination: "", name: "", user: "", port: "", socket: "" };
    line = { for: "", text: "", bad: false };
    const more = !!x && (!!x.user || !!x.port || x.socket_override || x.name !== defaultName(x.destination));
    return { id: x?.id ?? null, draft, more, error: null, busy: false };
  }

  const defaultName = (d: string) => d.split("@").pop() ?? d;

  function draw(): HTMLElement {
    return form ? drawForm(form) : check ? drawCheck(check) : drawList();
  }

  function drawList(): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head set-head-row");
    const words = h("div", "set-head-words");
    const title = h("div", "set-h", "Hosts");
    title.id = "set-h-hosts";
    words.append(title, h("div", "set-sub", "Computers whose sessions Skiff can show. Pick one from the Host menu in the top bar."));
    const add = button("set-add ready", "Add SSH Host…", () => {
      form = newForm(null);
      redraw();
      focusServer();
    });
    head.append(words, add);
    main.append(head);
    const table = h("div", "set-table g-hosts");
    table.setAttribute("role", "table");
    table.setAttribute("aria-labelledby", "set-h-hosts");
    const cols = h("div", "set-row set-cols");
    cols.append(h("span", "c-lead"), h("span", "", "NAME"), h("span", "", "SERVER"), h("span", "", "STATE"), h("span", ""));
    table.append(cols, hostRow(null));
    for (const x of knownHosts()) table.append(hostRow(x));
    main.append(table);
    if (!knownHosts().length) main.append(h("div", "set-note", "Add a server to see and run its sessions here. Pick it from your SSH config, or type its name or address."));
    if (error) main.append(alert(error));
    return main;
  }

  /** One Host. `null`: this computer, which has nothing to set. */
  function hostRow(x: KnownHost | null): HTMLElement {
    const row = h("div", "set-row set-host");
    row.setAttribute("role", "row");
    const shown = x ? shownHost() === x.id : shownHost() === null;
    const lead = h("span", "c-lead");
    if (shown) lead.append(rune("indicators-check"));
    lead.setAttribute("aria-label", shown ? "Shown in this window" : "");
    const r = x && resolved.get(x.id);
    const acts = h("span", "c-end");
    if (x) {
      acts.append(
        button("set-rm", "Check", () => void runCheck(x.id)),
        button("set-rm", "Edit…", () => {
          form = newForm(x);
          redraw();
          focusServer();
        }),
        button("set-rm", "Rename…", () => rename(x)),
        button("set-rm c-remove", "Remove…", () => void remove(x)),
      );
    }
    row.append(
      lead,
      h("span", "c-name", x ? x.name : localHostName()),
      h("span", "c-server mono", x ? (r ? server(r) : typed(x)) : "On this computer"),
      stateCell(x),
      acts,
    );
    return row;
  }

  /** Dot and word, from the last check or connection in this app run. */
  function stateCell(x: KnownHost | null): HTMLElement {
    const [cls, word] = !x
      ? ["ok", "Connected"]
      : x.state === "ok"
        ? ["ok", "Connected"]
        : x.state === "attention"
          ? ["bad", "Needs attention"]
          : ["none", "Not checked"];
    const cell = h("span", `c-state ${cls}`);
    cell.append(h("span", "sq"), word);
    return cell;
  }

  function drawCheck(c: Check): HTMLElement {
    const x = knownHosts().find((k) => k.id === c.id);
    const name = x?.name ?? "this Host";
    const main = h("div", "set-main");
    const head = h("div", "set-head");
    head.append(
      button("set-back", "‹ Hosts", () => ((check = null), redraw())),
      h("div", "set-h", `Check ${name}`),
      h("div", "set-sub", `Skiff checks each step over ssh, top to bottom. It changes nothing on ${name}.`),
    );
    main.append(head);
    const table = h("div", "set-table g-check");
    table.setAttribute("role", "table");
    table.setAttribute("aria-label", `Check ${name}`);
    const rows: CheckLine[] = c.report?.rows ?? CHECK_ROWS.map((title, i) => ({ row: String(i), title, state: "skipped", text: i === 0 ? "Checking…" : "Waiting", fix: null }));
    for (const r of rows) {
      const row = h("div", `set-row set-check ${c.report ? r.state : "waiting"}`);
      row.setAttribute("role", "row");
      const mark = h("span", "c-lead");
      if (c.report && r.state === "passed") mark.append(rune("indicators-check"));
      else mark.append(h("span", "sq"));
      mark.setAttribute("aria-label", c.report ? r.state : "waiting");
      const text = h("span", "c-text", r.text);
      if (c.report && (r.state === "failed" || r.state === "warning") && r.fix && x) {
        const fix = r.fix;
        text.append(button("set-rm set-fix", FIX_LABEL[fix], () => void runFix(c, x, fix)));
      }
      row.append(mark, h("span", "c-name", r.title), text);
      table.append(row);
    }
    main.append(table);
    if (x) main.append(autoUpdate(x));
    if (c.error) main.append(alert(c.error));
    if (c.note) main.append(h("div", "set-note", c.note));
    const acts = h("div", "set-form-acts");
    const running = !c.report && !c.error;
    const again = button("set-rm", running ? "Checking…" : "Check again", () => void runCheck(c.id));
    again.disabled = running;
    acts.append(again, h("span", "set-fill"));
    if (c.report?.ok && x && shownHost() !== x.id) acts.append(button("set-add ready", `Switch to ${name}`, () => pick(x.id)));
    acts.append(button("set-rm", "Done", () => ((check = null), redraw())));
    main.append(acts);
    return main;
  }

  /** One fix, after its confirm, then the check again. */
  async function runFix(c: Check, x: KnownHost, fix: Fix) {
    const again = () => void runCheck(x.id);
    c.note = undefined;
    try {
      if (fix === "trust_key" || fix === "review_key") return await trustKey(x, again);
      if (fix === "make_key") return makeKey(x, again);
      if (fix === "unlock_key") return secretDialog(x, "unlock", again);
      if (fix === "install_key") return secretDialog(x, "install", again);
      if (fix === "fix_permissions") return secretDialog(x, "fix", again);
      if (fix === "install_skiff") return installSkiff(c, x);
      if (fix === "start_skiff") {
        c.note = `Starting Skiff on ${x.name}…`;
        redraw();
        await invoke("start_skiff", { id: x.id });
        return again();
      }
      if (fix === "turn_on_linger") return turnOnLinger(x, again);
      if (fix === "update_skiff") return updateSkiff(c, x);
      if (fix === "copy_public_key") {
        await copyText(await invoke<string>("public_key", { id: x.id }));
        c.note = `Public key copied. A public key is safe to send. It does not let anyone log in as you. When the admin of ${x.name} adds it, check again.`;
        redraw();
      }
    } catch (e) {
      c.error = String(e);
      redraw();
    }
  }

  /** One confirm covers the install, the start and keeping it running. */
  function installSkiff(c: Check, x: KnownHost) {
    const [os, arch] = c.report?.platform ?? ["linux", ""];
    const v = c.report?.version ?? "";
    actionDialog({
      title: `Install Skiff on ${x.name}?`,
      body:
        os === "macos"
          ? `Skiff ${v} for macOS ${arch}. Files go in ~/.local/share/skiff/server on ${x.name}. Two LaunchAgents in ~/Library/LaunchAgents start skiffd (sessions) and skiff-host (projects), and keep them running after you log out. After a restart they start when you log in there. Nothing outside your home folder changes.`
          : `Skiff ${v} for Linux ${arch}. Files go in ~/.local/share/skiff/server on ${x.name}. Two user services start skiffd (sessions) and skiff-host (projects), and keep them running after you log out and after a restart. Nothing outside your home folder changes.`,
      action: "Install Skiff",
      tone: "go",
      submit: async () => {
        c.note = `Installing Skiff ${v} on ${x.name}…`;
        c.error = null;
        redraw();
        try {
          const r = await invoke<{ source: string; version: string; cli_linked: boolean }>("install_skiff", { id: x.id });
          const from = r.source === "release" ? "" : ` This dev build sent its own binaries, as no release has Skiff ${r.version}.`;
          const cli = r.cli_linked ? "" : ` ~/.local/bin/skiff on ${x.name} is another file, so Skiff left it.`;
          await runCheck(x.id);
          if (check) check.note = `Skiff ${r.version} is installed on ${x.name}.${from}${cli}`;
          redraw();
        } catch (e) {
          c.note = undefined;
          c.error = String(e);
          redraw();
        }
      },
    });
  }

  /** The Host's own setting: on, an older install Skiff made is updated as
   *  the app connects, when every session survives. */
  function autoUpdate(x: KnownHost): HTMLElement {
    const row = h("label", "set-row set-open set-host-field set-auto-update");
    const box = h("input");
    box.type = "checkbox";
    box.checked = x.auto_update;
    box.addEventListener("change", async () => {
      try {
        await invoke("set_host_auto_update", { id: x.id, on: box.checked });
        await loadHosts();
      } catch (e) {
        box.checked = !box.checked;
        alertCheck(e);
      }
    });
    row.append(box, h("span", "c-text", `Update Skiff on this server automatically. Only a Skiff that Skiff installed, and only when its sessions keep running.`));
    return row;
  }

  /** Update Skiff…: one confirm. When skiffd cannot keep its sessions, the
   *  update stops, and a red dialog names the sessions it would end. */
  function updateSkiff(c: Check, x: KnownHost) {
    const v = c.report?.version ?? "";
    actionDialog({
      title: `Update Skiff on ${x.name}?`,
      body: `Skiff ${v} goes in ~/.local/share/skiff/server on ${x.name}, beside the version there now, which Skiff keeps for a rollback. skiffd moves to the new version and keeps its sessions running. If the update fails, the old version keeps running.`,
      action: "Update Skiff",
      tone: "go",
      submit: () => void runUpdate(c, x, false),
    });
  }

  async function runUpdate(c: Check, x: KnownHost, endSessions: boolean) {
    c.note = `Updating Skiff on ${x.name}…`;
    c.error = null;
    redraw();
    try {
      const r = await invoke<Updated>("update_skiff", { id: x.id, endSessions });
      if (r.result === "no_handover") return endsSessions(c, x, r.from, r.sessions);
      await runCheck(x.id);
      const dev = r.source === "this build" ? ` This dev build sent its own binaries, as no release has Skiff ${r.version}.` : "";
      const note =
        r.result === "done"
          ? `Updated Skiff on ${x.name} from ${r.from} to ${r.version}.${r.restarted ? " skiffd started again." : " Its sessions kept running."}${dev}`
          : r.result === "newer"
            ? `${x.name} runs Skiff ${r.version}, newer than this computer. Skiff does not change it.`
            : `${x.name} runs Skiff ${r.version} already.`;
      if (check) check.note = note;
      redraw();
    } catch (e) {
      c.note = undefined;
      c.error = String(e);
      redraw();
    }
  }

  /** The red dialog: this update ends the sessions it names. */
  function endsSessions(c: Check, x: KnownHost, from: string, sessions: string[]) {
    c.note = undefined;
    redraw();
    const n = sessions.length;
    actionDialog({
      title: `Update Skiff on ${x.name} and end ${n === 1 ? "1 session" : `${n} sessions`}?`,
      body: `Skiff ${from} on ${x.name} cannot hand its sessions to the new version. This update ends the ${n === 1 ? "session" : `${n} sessions`} on ${x.name}: ${sessions.join(", ")}. Save their work first. Nothing changed yet.`,
      action: n === 1 ? "Update and end 1 session" : `Update and end ${n} sessions`,
      tone: "danger",
      submit: () => void runUpdate(c, x, true),
    });
  }

  function turnOnLinger(x: KnownHost, done: () => void) {
    actionDialog({
      title: `Keep Skiff running on ${x.name} after you log out?`,
      body: `Skiff turns on lingering for your account on ${x.name}. Its user services then start when the server starts, and keep running after you log out. Nothing else changes.`,
      action: "Turn on",
      tone: "go",
      submit: async () => {
        await invoke<boolean>("turn_on_linger", { id: x.id });
        done();
      },
    });
  }

  /** Trust a new host key, or replace a changed one: the fingerprint first. */
  async function trustKey(x: KnownHost, done: () => void) {
    const key = await invoke<HostKey>("host_key_scan", { id: x.id });
    const prints = (title: string, fs: Fingerprint[], old = false) => {
      const box = h("div", "confirm-body set-prints" + (old ? " old" : ""));
      box.append(h("div", "set-prints-h", title), ...fs.map((f) => h("div", "mono", `${f.kind}  ${f.sha256}`)));
      return box;
    };
    if (!key.known.length) {
      return actionDialog({
        title: `Trust the host key of ${x.name}?`,
        body: `This is the first connection from this computer to ${x.name}. Compare the fingerprint with the one your server provider or admin gives you, if you have one. Skiff adds the key to ${key.file}. ssh in a terminal then trusts it too.`,
        content: [prints("Fingerprint", key.fingerprints)],
        action: "Trust this key",
        tone: "go",
        submit: async () => {
          await invoke("host_key_trust", { token: key.token, replace: false });
          done();
        },
      });
    }
    actionDialog({
      title: `The host key of ${x.name} changed`,
      body: `This is normal after a reinstall of the server. It can also mean that another machine answers at this address. Skiff does not connect until you decide.`,
      content: [prints("Key that Skiff knows", key.known, true), prints("Key that answers now", key.fingerprints)],
      action: "Replace the key…",
      tone: "danger",
      submit: async () => {
        // Asked a second time, as a changed key can be an attack.
        setTimeout(async () => {
          const ok = await confirmAction({
            title: `Replace the key of ${x.name}?`,
            body: `Skiff removes the old key from ${key.file} and trusts the new one. Do this only if you know why it changed, such as a reinstall of ${x.name}. If you are not sure, ask the admin of ${x.name} first.`,
            action: "Replace the key",
          });
          if (!ok) return;
          await invoke("host_key_trust", { token: key.token, replace: true }).catch((e) => alertCheck(e));
          done();
        });
      },
    });
  }

  function alertCheck(e: unknown) {
    if (check) check.error = String(e);
    redraw();
  }

  function makeKey(x: KnownHost, done: () => void) {
    actionDialog({
      title: "Make an SSH key on this computer?",
      body: `This computer has no SSH key. Skiff makes one at ~/.ssh/id_ed25519, with no passphrase, and then installs it on ${x.name}. ssh and other apps on this computer use it too.`,
      action: "Make key",
      tone: "go",
      submit: async () => {
        await invoke("make_key");
        // The new key goes to the server next, with one password.
        setTimeout(() => secretDialog(x, "install", done));
      },
    });
  }

  /** A dialog with one secret field. The field is cleared when it closes. */
  function secretDialog(x: KnownHost, what: "install" | "unlock" | "fix", done: () => void) {
    const input = h("input", "prompt-input");
    input.type = "password";
    input.autocomplete = "off";
    input.spellcheck = false;
    const user = x.user ?? (x.destination.includes("@") ? x.destination.split("@")[0] : "");
    const who = user ? `${user} on ${x.name}` : `your account on ${x.name}`;
    const copy = {
      install: {
        title: `Log in to ${x.name} once`,
        body: `Type the password of ${who}. Skiff uses it one time to add this computer's key to ${x.name}. Skiff does not store it.`,
        label: "Password",
        action: "Install key",
        cmd: "install_key",
      },
      unlock: {
        title: "Unlock your SSH key",
        body: "Your key has a passphrase and is not in the SSH agent. Type the passphrase. Skiff gives the key to the agent until you log out. Skiff does not store the passphrase.",
        label: "Passphrase",
        action: "Unlock",
        cmd: "unlock_key",
      },
      fix: {
        title: `Replace the key file on ${x.name}?`,
        body: `On ${x.name}, the file that holds your keys belongs to another account, so the SSH server ignores it. Skiff renames it and writes a new one that you own, with this computer's key in it. The other keys in the old file then stop working on ${x.name}. Type the password of ${who}. Skiff does not store it.`,
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
          await invoke(copy.cmd, args);
        } catch (e) {
          input.focus();
          throw String(e);
        }
        done();
      },
    });
  }

  function rename(x: KnownHost) {
    promptAction({
      title: `Rename ${x.name}`,
      body: "The name shows in the Host menu, in Settings and in the window title. The server stays the same.",
      placeholder: "a name",
      value: x.name,
      action: "Rename",
      submit: async (name) => {
        await invoke("rename_host", { id: x.id, name });
        await loadHosts();
        redraw();
      },
    });
  }

  /**
   * Remove forgets the Host here. When Skiff installed Skiff there, a box
   * also stops and uninstalls it. The box shows only after the server says
   * so, and the copy names the sessions that end and what stays.
   */
  function remove(x: KnownHost) {
    const kept = `Skiff removes it from this computer's Host list. Its sessions keep running on ${x.name}.`;
    const body = h("div", "confirm-body", kept);
    const extra = h("div", "set-remove-box");
    const status = h("div", "set-sub", `Skiff checks what it installed on ${x.name}…`);
    extra.append(status);
    const box = h("input") as HTMLInputElement;
    box.type = "checkbox";
    box.id = "set-uninstall";
    let ends = "";
    box.addEventListener("change", () => {
      body.textContent = box.checked
        ? `Skiff stops and disables its services on ${x.name} and deletes the files it installed there. ${ends} Your projects and all other files on ${x.name} stay. Then Skiff removes ${x.name} from this computer's Host list.`
        : kept;
    });
    actionDialog({
      title: `Remove ${x.name}?`,
      content: [body, extra],
      action: "Remove Host",
      tone: "danger",
      submit: async () => {
        error = null;
        await invoke("remove_host", { id: x.id, uninstall: box.checked });
        await loadHosts().catch(() => {});
        redraw();
      },
    });
    invoke<{ managed: boolean; sessions: string[] }>("uninstall_preview", { id: x.id }).then(
      (p) => {
        if (!p.managed) return void (status.textContent = `Skiff did not install Skiff on ${x.name}, so it leaves Skiff there as it is.`);
        const n = p.sessions.length;
        ends = n === 0 ? "No sessions run there." : `This ends its ${n === 1 ? "session" : `${n} sessions`}: ${p.sessions.join(", ")}.`;
        const label = h("label", "set-check-label");
        label.setAttribute("for", "set-uninstall");
        label.append(box, h("span", "", `Also stop and uninstall Skiff on ${x.name}. ${ends}`));
        status.replaceWith(label);
      },
      () => void (status.textContent = `Skiff cannot reach ${x.name} now, so it can only remove it from this computer.`),
    );
  }

  function drawForm(f: Form): HTMLElement {
    const main = h("div", "set-main");
    const head = h("div", "set-head");
    const back = button("set-back", "‹ Hosts", () => {
      form = null;
      redraw();
    });
    const editing = f.id ? knownHosts().find((x) => x.id === f.id) : null;
    head.append(
      back,
      h("div", "set-h", editing ? `Edit ${editing.name}` : "Add SSH Host"),
      h("div", "set-sub", "Pick a server from your SSH config, or type its name or address."),
    );
    main.append(head);

    const list = h("datalist");
    list.id = "set-ssh-aliases";
    for (const a of aliases) {
      const o = h("option");
      o.value = a.name;
      if (a.goes_to) o.label = a.goes_to;
      list.append(o);
    }
    const dest = field(f, "destination", "Server", "devbox, devbox.lan or 10.0.0.5");
    dest.setAttribute("list", list.id);
    dest.addEventListener("input", () => lookup(f));
    main.append(fieldRow("Server", dest, list));
    const said = h("div", "set-note set-resolve" + (line.bad ? " bad" : ""), line.text || "Skiff reads the user and port from your SSH config.");
    said.id = "set-resolve";
    main.append(said);

    const toggle = button("set-rm", f.more ? "Less" : "More", () => {
      f.more = !f.more;
      redraw();
    });
    toggle.setAttribute("aria-expanded", String(f.more));
    if (f.more) {
      const r = line.for === key(f) ? lastResolved : null;
      const name = field(f, "name", "Name", defaultName(f.draft.destination.trim()) || "the server's name");
      const user = field(f, "user", "User", r?.user || "from SSH config");
      const port = field(f, "port", "Port", r ? String(r.port) : "from SSH config");
      port.inputMode = "numeric";
      user.addEventListener("input", () => lookup(f));
      port.addEventListener("input", () => lookup(f));
      // The check finds the socket. A value here replaces what it finds.
      const found = editing?.socket && !editing.socket_override ? `Found: ${editing.socket}` : "Found by Skiff";
      const socket = field(f, "socket", "Socket on the server", found);
      main.append(fieldRow("Name", name), fieldRow("User", user), fieldRow("Port", port), fieldRow("Socket on the server", socket));
    }

    const acts = h("div", "set-form-acts");
    const save = button("set-add ready", f.busy ? "…" : "Connect", () => void submit(f));
    save.disabled = f.busy;
    acts.append(toggle, h("span", "set-fill"), button("set-rm", "Cancel", () => ((form = null), redraw())));
    // Only a new name needs no check.
    if (editing && onlyName(f, editing)) acts.append(button("set-rm", "Save", () => void submit(f, false)));
    acts.append(save);
    main.append(acts);
    if (f.error) main.append(alert(f.error));
    if (!line.for && f.draft.destination.trim()) lookup(f);
    return main;
  }

  let lastResolved: Resolved | null = null;
  const key = (f: Form) => `${f.draft.destination.trim()}\n${f.draft.user.trim()}\n${f.draft.port.trim()}`;

  /** The resolve line follows the fields, 250 ms after the last key. */
  function lookup(f: Form) {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const k = key(f);
      if (!f.draft.destination.trim()) {
        line = { for: "", text: "", bad: false };
        return paint();
      }
      try {
        const r = await resolve(f.draft.destination.trim(), f.draft.user.trim(), f.draft.port.trim());
        if (k !== key(f)) return;
        lastResolved = r;
        const jump = r.jump ? `, through ${r.jump}` : "";
        line = { for: k, text: `Skiff connects as ${r.user} to ${r.hostname}, port ${r.port}${jump}.`, bad: false };
      } catch (e) {
        if (k !== key(f)) return;
        line = { for: k, text: String(e), bad: true };
      }
      paint();
    }, 250);
  }

  /** The line alone: a redraw would move the caret. */
  function paint() {
    const el = document.getElementById("set-resolve");
    if (!el) return;
    el.textContent = line.text || "Skiff reads the user and port from your SSH config.";
    el.classList.toggle("bad", line.bad);
  }

  const onlyName = (f: Form, x: KnownHost) => {
    const d = f.draft;
    return d.destination.trim() === x.destination && d.user.trim() === (x.user ?? "") && d.port.trim() === (x.port ? String(x.port) : "") && d.socket.trim() === (x.socket_override ? x.socket ?? "" : "");
  };

  /** Saves the form. Connect then runs the check list: it finds the socket. */
  async function submit(f: Form, connect = true) {
    if (f.busy) return;
    f.error = null;
    f.busy = true;
    redraw();
    let id = f.id;
    // Save is for a new name only: any other change checks again.
    const before = f.id ? knownHosts().find((k) => k.id === f.id) : undefined;
    connect ||= !before || !onlyName(f, before);
    try {
      if (id) await invoke("edit_host", { id, form: f.draft });
      else id = await invoke<string>("add_host", { form: f.draft });
      await loadHosts();
      await load();
      form = null;
    } catch (e) {
      f.error = String(e);
      id = null;
    } finally {
      f.busy = false;
    }
    if (id && connect) return void runCheck(id);
    redraw();
  }

  function field(f: Form, k: keyof Draft, label: string, placeholder: string): HTMLInputElement {
    const input = h("input", "c-cmd mono");
    input.type = "text";
    input.spellcheck = false;
    input.autocomplete = "off";
    input.value = f.draft[k];
    input.placeholder = placeholder;
    input.setAttribute("aria-label", label);
    input.addEventListener("input", () => (f.draft[k] = input.value));
    input.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      e.preventDefault();
      void submit(f);
    });
    return input;
  }

  function fieldRow(label: string, ...content: HTMLElement[]): HTMLElement {
    const row = h("div", "set-row set-open set-host-field");
    row.append(h("span", "c-name", label), ...content);
    return row;
  }

  function focusServer() {
    document.querySelector<HTMLInputElement>('#settings input[aria-label="Server"]')?.focus();
  }

  function alert(text: string): HTMLElement {
    const err = h("div", "set-error", text);
    err.setAttribute("role", "alert");
    return err;
  }

  return { load, open, draw, focusServer, get editing() { return form !== null; } };
}

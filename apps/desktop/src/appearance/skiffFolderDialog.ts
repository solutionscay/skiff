import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { hostInvoke } from "../app/hostCaps";
import { isRemote } from "../app/hosts";
import { openFolderPicker } from "../workspace/folderPicker";
import { smallButton } from "../ui/controls";
import { dialogFrame } from "../ui/dialogParts";
import { h } from "../ui/dom";
import { t } from "../i18n";

/** `move_skiff_folder`: what the move did. */
interface Moved {
  from: string;
  to: string;
  warning: string | null;
}

/** Bytes, as the Rust side shows them. */
export function size(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/** `dir` with `name` under it. */
function under(dir: string, name: string): string {
  return `${dir.replace(/\/+$/, "")}/${name}`;
}

/**
 * Settings › Disk use › Folder › Move…: the person chooses a place, and the
 * agent folder moves into it under its own name. The Host refuses while
 * ledger work is open or an agent session works, or when the target is not
 * empty: the error line says why and the dialog stays. `current` is the
 * folder now. `changed` runs after a move, so Settings reads the folder
 * again. On a remote Host, Choose… opens the folder picker on that Host.
 */
export function moveSkiffFolder(o: { current: string; changed: () => void }) {
  const back = document.activeElement as HTMLElement | null;
  const remote = isRemote();
  const name = o.current.replace(/\/+$/, "").split("/").pop() || "skiff-agent";
  const { overlay, panel } = dialogFrame("confirm-panel folder-panel", "dialog", t("folder.title"));
  overlay.appendChild(panel);
  document.body.appendChild(overlay);

  let to: string | null = null;
  let busy = false;
  let open = true;

  const close = () => {
    if (!open || busy) return;
    open = false;
    overlay.remove();
    back?.focus?.();
  };

  const title = h("div", "confirm-title", t("folder.title"));
  const row = (label: string, path: string) => {
    const r = h("div", "folder-row");
    const p = h("span", "folder-row-path mono", path);
    p.title = path;
    r.append(h("span", "folder-row-name", label), p);
    return { r, p };
  };
  const from = row(t("folder.from"), o.current);
  const target = row(t("folder.to"), "");
  const choose = smallButton(t("folder.choose"), () => void pick());
  target.r.append(choose);
  const note = h("div", "folder-note", t("folder.ends"));
  const error = h("div", "prompt-error");
  error.setAttribute("role", "alert");
  const foot = h("div", "confirm-foot");
  const cancel = h("button", "confirm-cancel", t("folder.cancel"));
  cancel.type = "button";
  cancel.addEventListener("click", close);
  const go = h("button", "confirm-act go", t("folder.move"));
  go.type = "button";
  go.disabled = true;
  go.addEventListener("click", () => void run());
  foot.append(cancel, go);

  const chosen = (dir: string) => {
    to = under(dir, name);
    target.p.textContent = to;
    target.p.title = to;
    error.textContent = "";
    go.disabled = false;
    go.focus();
  };

  async function pick() {
    if (busy) return;
    if (remote) return openFolderPicker({ action: t("folder.useThis"), pick: async (dir) => chosen(dir) });
    const parent = o.current.replace(/\/[^/]+\/?$/, "") || undefined;
    const dir = await openFolder({ directory: true, title: t("folder.chooseTitle"), defaultPath: parent }).catch(() => null);
    if (typeof dir === "string") chosen(dir);
  }

  /** Runs the move. A refusal keeps the dialog with its error line. */
  async function run() {
    if (!to || busy) return;
    busy = true;
    error.textContent = "";
    for (const b of panel.querySelectorAll("button")) b.disabled = true;
    go.textContent = "…";
    try {
      const done = await hostInvoke<Moved>("skipper_folder_move", "move_skiff_folder", { path: to });
      busy = false;
      o.changed();
      finished(done);
    } catch (e) {
      busy = false;
      error.textContent = String(e);
      go.textContent = t("folder.move");
      for (const b of panel.querySelectorAll("button")) b.disabled = false;
    }
  }

  function finished(d: Moved) {
    title.textContent = t("folder.moved", { to: d.to });
    from.r.remove();
    target.r.remove();
    note.remove();
    if (d.warning) error.textContent = d.warning;
    const ok = h("button", "confirm-act go", t("folder.done"));
    ok.type = "button";
    ok.addEventListener("click", close);
    foot.replaceChildren(ok);
    ok.focus();
  }

  panel.append(title, from.r, target.r, note, error, foot);

  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") close();
    else if (e.key === "Enter") {
      const at = document.activeElement as HTMLElement | null;
      if (at instanceof HTMLButtonElement) return;
      foot.querySelector<HTMLButtonElement>(".confirm-act:not(:disabled)")?.click();
    } else if (e.key === "Tab") {
      const stops = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled)")];
      const i = stops.indexOf(document.activeElement as HTMLElement);
      stops[(i + (e.shiftKey ? -1 : 1) + stops.length) % stops.length]?.focus();
    } else return;
    e.preventDefault();
  });
  choose.focus();
}

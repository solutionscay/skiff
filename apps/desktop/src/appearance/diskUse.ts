import { invoke } from "@tauri-apps/api/core";
import { h } from "../ui/dom";
import { confirmAction } from "../ui/confirm";
import { size } from "./skiffFolderDialog";
import { t, tn, upper } from "../i18n";

/** A count of files and their bytes. */
interface Line {
  files: number;
  bytes: number;
}

/** `disk_usage`, as the backend counts the Skiff agent's folder. */
export interface DiskUsage {
  /** The folder with `~` for home. */
  shown: string;
  /** The images, pinned ones included. */
  cache: Line;
  /** The pinned images: no flush removes them. */
  pinned: Line;
  /** Null: the ledger was refused. */
  results: Line | null;
  skippers: { name: string; folder: string; line: Line }[];
  closed: Line;
  items: number;
  plans: number;
  logs: Line;
  other: Line;
  total: Line;
  unreadable: number;
  ledger_error: string | null;
  daemon: boolean;
  /** What Flush done work removes. The flush sends it back. */
  removable: string[];
  /** Auto flush's age in days. 0: off. */
  cache_days: number;
}

/** The page's state. It survives every redraw of Settings. */
export interface DiskState {
  usage: DiskUsage | null;
  counting: boolean;
  /** "Removed N files, X." after a flush. */
  removed: string | null;
  error: string | null;
}

export const diskState = (): DiskState => ({ usage: null, counting: false, removed: null, error: null });


/** Counts the folder. The cells show "…" until the answer comes. */
export async function countDisk(s: DiskState, redraw: () => void) {
  s.counting = true;
  redraw();
  try {
    s.usage = await invoke<DiskUsage>("disk_usage");
    s.error = null;
  } catch (e) {
    s.error = String(e);
  } finally {
    s.counting = false;
    redraw();
  }
}

async function flush(s: DiskState, redraw: () => void, run: () => Promise<{ files: number; bytes: number }>) {
  try {
    const out = await run();
    s.removed = t("disk.removed", { files: tn("disk.files", out.files), size: size(out.bytes) });
    s.error = null;
  } catch (e) {
    s.error = String(e);
  }
  await countDisk(s, redraw);
}

/** Flush done work…: the counts first, then the flush of what the dialog showed. */
async function flushDone(s: DiskState, u: DiskUsage, redraw: () => void) {
  const lines: string[] = [];
  if (u.closed.files || u.plans)
    lines.push(t("disk.flushClosed", { files: tn("disk.resultFiles", u.closed.files), size: size(u.closed.bytes), items: tn("disk.closedItems", u.items), plans: tn("disk.plans", u.plans) }));
  const loose = { files: u.cache.files - u.pinned.files, bytes: u.cache.bytes - u.pinned.bytes };
  if (loose.files) lines.push(t("disk.flushImages", { images: tn("disk.images", loose.files), size: size(loose.bytes) }));
  lines.push(t("disk.flushStays"));
  const ok = await confirmAction({ title: t("disk.flushTitle"), body: lines.join("\n"), action: t("disk.flush") });
  if (ok) await flush(s, redraw, () => invoke("flush_done", { expected: u.removable }));
}

/**
 * Settings › Disk use: what the Skiff agent's folder holds, Flush images,
 * Flush done work… and the auto flush switch. `setDays` saves
 * `[skiff_agent] cache_days`.
 */
export function diskUseSection(s: DiskState, redraw: () => void, setDays: (days: number) => void): HTMLElement[] {
  const u = s.usage;
  const head = h("div", "set-head");
  const title = h("div", "set-h", t("settings.tab.disk"));
  title.id = "set-h-disk";
  head.append(title, h("div", "set-sub mono", u?.shown ?? ""));

  const table = h("div", "set-disk");
  table.setAttribute("role", "table");
  table.setAttribute("aria-labelledby", title.id);
  const cols = h("div", "set-row set-cols");
  cols.setAttribute("role", "row");
  cols.append(h("span", "", upper(t("disk.colType"))), h("span", "c-num", upper(t("disk.colFiles"))), h("span", "c-num", upper(t("disk.colSize"))), h("span", "", upper(t("disk.colFolder"))), h("span"));
  table.append(cols);

  const counted = (l: Line | null | undefined, known = true) =>
    s.counting || !u ? ["…", "…"] : !known || !l ? ["—", "—"] : [String(l.files), size(l.bytes)];
  const row = (name: string, l: Line | null | undefined, opts: { sub?: boolean; total?: boolean; folder?: string; plain?: boolean; act?: HTMLElement; known?: boolean } = {}) => {
    const r = h("div", "set-row set-agent on" + (opts.sub ? " sub" : "") + (opts.total ? " total" : ""));
    r.setAttribute("role", "row");
    const [files, bytes] = counted(l, opts.known ?? true);
    r.append(
      h("span", "c-name", name),
      h("span", "c-num", files),
      h("span", "c-num", bytes),
      h("span", "c-where" + (opts.plain ? "" : " mono"), opts.folder ?? ""),
      opts.act ?? h("span"),
    );
    return r;
  };
  const button = (label: string, off: boolean, click: () => void) => {
    const cell = h("span", "c-act");
    const b = h("button", "set-rm", label);
    b.type = "button";
    b.disabled = off;
    b.addEventListener("click", click);
    cell.append(b);
    return cell;
  };

  const known = !!u && u.results !== null;
  const loose = u ? u.cache.files - u.pinned.files : 0;
  const doneOff = !u || !known || !u.daemon || (!u.closed.files && !u.plans && !loose);
  table.append(
    row(t("disk.rowImages"), u?.cache, {
      folder: "cache/",
      act: button(t("disk.flushImagesButton"), !loose, () => void flush(s, redraw, () => invoke("flush_cache"))),
    }),
    row(t("disk.rowPinned"), u?.pinned, { sub: true }),
    row(t("disk.rowResults"), u?.results, { known }),
    ...(u?.skippers ?? []).map((k) => row(k.name, k.line, { sub: true, folder: k.folder, known })),
    row(t("disk.rowClosed"), u?.closed, {
      sub: true,
      known,
      plain: true,
      folder: u && known && !s.counting && (u.items || u.plans) ? `${tn("disk.items", u.items)} · ${tn("disk.plans", u.plans)}` : "",
      act: button(t("disk.flushDoneButton"), doneOff, () => u && void flushDone(s, u, redraw)),
    }),
    row(t("disk.rowLogs"), u?.logs),
    row(t("disk.rowOther"), u?.other),
    row(t("disk.rowTotal"), u?.total, { total: true }),
  );

  const status = h("div", "set-hint", s.removed ?? "");
  status.setAttribute("role", "status");
  if (s.removed) status.dataset.kind = "added";
  const errors = [
    s.error,
    u?.ledger_error,
    u && !u.daemon ? t("disk.noDaemon") : null,
    u?.unreadable ? t("disk.unreadable", { files: tn("disk.files", u.unreadable) }) : null,
  ]
    .filter((e): e is string => !!e)
    .map((e) => {
      const el = h("div", "set-error", e);
      el.setAttribute("role", "alert");
      return el;
    });

  const days = u?.cache_days ?? 0;
  const on = days > 0;
  const auto = h("div", "set-row set-open set-agent set-disk-auto" + (on ? " on" : ""));
  const sw = h("button", "c-mode set-switch");
  sw.type = "button";
  sw.disabled = !u;
  sw.setAttribute("role", "switch");
  sw.setAttribute("aria-checked", String(on));
  sw.setAttribute("aria-label", t(on ? "disk.autoOn" : "disk.autoOff"));
  sw.appendChild(h("span", "switch"));
  sw.addEventListener("click", () => setDays(on ? 0 : 30));
  auto.append(h("span", "c-name", t("disk.auto")), h("span", "c-cmd", on ? tn("disk.days", days) : ""), sw);

  return [head, table, status, ...errors, auto];
}

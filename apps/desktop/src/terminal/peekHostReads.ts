/** Host file and diff reads for the peek. The controller supplies its gate and actions. */
import { hostInvoke } from "../app/hostCaps";
import { hostName } from "../app/hosts";
import { t } from "../i18n";
import type { HostDiff, HostFile } from "../platform/types";
import type { Preview } from "../workspace/rowActs";
import { allChanges, cardFrame } from "./peekCard";
import { diffBody } from "./diffView";
import { hostFileFrame, renderHostFile, showHostView, type Gate } from "./peekHostView";

/** Shows a remote diff, guarded by the preview and data connection. */
export function showDiff(gate: Gate, close: () => void, p: Extract<Preview, { kind: "host-diff" }>) {
  const el = cardFrame("diff", `${p.wt}/${p.file ?? allChanges(p.base)}`, close, "peek diff-view");
  showHostView(
    gate,
    el,
    t("peek.loadingDiff", { host: hostName() }),
    () => hostInvoke<HostDiff>("git_diff", "git_diff", { path: p.wt, file: p.file ?? null }, p.hostId),
    (d) => diffBody(d, p.file, p.base),
    t("peek.noDiff"),
  );
}

/** Shows a bounded Host file, with an action to open a local copy when allowed. */
export function showFile(gate: Gate, close: () => void, p: Extract<Preview, { kind: "host-file" }>, openCopy: (wt: string, file: string, hostId?: string) => void) {
  const copy = p.copy ? () => openCopy(p.wt, p.file, p.hostId) : null;
  showRead(gate, close, p.path, () => hostInvoke<HostFile>("files", "read_file", { worktree: p.wt, file: p.file }, p.hostId), copy);
}

/** Shows a bounded Host read in the file viewer. */
export function showRead(gate: Gate, close: () => void, title: string, read: () => Promise<HostFile>, copy: (() => void) | null) {
  showHostView(gate, hostFileFrame(title, close, copy), t("peek.loadingFile", { host: hostName() }), read, renderHostFile, t("peek.noPreview"));
}

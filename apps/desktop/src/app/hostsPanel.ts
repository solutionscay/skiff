import { button, h } from "../ui/dom";
import { t, tn } from "../i18n";
import { copyText } from "../platform/clipboard";
import { showError } from "../ui/alerts";
import { canDisconnect, connectEndpoint, disconnect, orderedConnections, retry } from "./connections";
import { hostLabel, knownHosts, type HostsPage } from "./hosts";
import { pageRoute } from "../platform/invoke";
import { shownIds } from "./stateQueries";
import { isSlot } from "../workspace/layoutSlots";

export function hostIcon(remote: boolean) {
  const el = h("span", "host-icon");
  el.innerHTML = remote
    ? '<svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="7"/><rect x="3" y="13" width="18" height="7"/><path d="M7 7.5h1M7 16.5h1"/></svg>'
    : '<svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="12"/><path d="M8 20h8M12 16v4"/></svg>';
  return el;
}

let closePanel: (() => void) | undefined;
export function openHostsPanel(anchor: HTMLElement, manage: (p: HostsPage) => void) {
  if (closePanel) {
    closePanel();
    return;
  }
  const panel = h("section", "hosts-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", t("settings.tab.hosts"));
  const rect = anchor.getBoundingClientRect();
  panel.style.top = rect.bottom + "px";
  panel.style.right = Math.max(0, innerWidth - rect.right) + "px";
  document.body.append(panel);
  anchor.setAttribute("aria-expanded", "true");
  function close() {
    clearInterval(timer);
    panel.remove();
    closePanel = undefined;
    anchor.setAttribute("aria-expanded", "false");
    window.removeEventListener("skiff-host-panel", draw);
    document.removeEventListener("pointerdown", outside, true);
    document.removeEventListener("keydown", key, true);
    anchor.focus();
  }
  const outside = (e: PointerEvent) => {
    if (!panel.contains(e.target as Node) && !anchor.contains(e.target as Node)) close();
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopImmediatePropagation();
      close();
    }
  };
  const settings = (p: HostsPage) => {
    close();
    manage(p);
  };
  let signature = "";
  function draw() {
    const hosts = orderedConnections();
    const ids = [null, ...knownHosts().map((h) => h.id)];
    const stamp = JSON.stringify([
      ids.map(hostLabel),
      hosts.map((c) => [c.entry, c.phase, c.lines, !!c.next, c.phase === "up" ? 0 : Math.floor(Date.now() / 1000)]),
      shownIds(),
    ]);
    if (stamp === signature) return;
    signature = stamp;
    const focused = panel.contains(document.activeElement) ? (document.activeElement as HTMLElement).dataset.action : null;
    panel.replaceChildren(h("div", "hp-title", t("settings.tab.hosts")));
    ids.forEach((id, i) => {
      const c = hosts.find((it) => it.endpoint === id);
      const name = hostLabel(id);
      const row = h("div", "hp-row");
      const state = c
        ? c.phase === "up"
          ? t("hosts.connected")
          : c.phase === "failed"
            ? t("hosts.needsAttention")
            : t("connect.connecting", { host: name })
        : t("hosts.disconnected");
      const action = button("hp-act", t(!c ? "hosts.connect" : c.phase === "up" ? "hosts.disconnect" : "dialog.cancel"), () => {
        void (c ? disconnect(c) : connectEndpoint(id)).catch(showError);
      });
      action.dataset.action = String(i);
      action.disabled = !!c && !canDisconnect(c);
      row.append(
        h("span", "host-square " + (c?.phase ?? "off")),
        hostIcon(id !== null),
        h("span", "hp-name", name),
        h("span", "hp-state", state),
        action,
      );
      panel.append(row);
      if (!c) return;
      if (c.phase === "up") {
        const note = h("div", "hp-note", t("hosts.removeKept", { name }));
        const n = pageRoute()?.entry === c.entry ? shownIds().filter((s) => !isSlot(s)).length : 0;
        if (n) note.append(h("div", "", tn("hosts.panesClose", n)));
        panel.append(note);
        return;
      }
      const lines = h("div", "hp-lines");
      for (const line of c.phase === "failed" ? c.lines : c.lines.slice(-8)) lines.append(h("div", "", line));
      panel.append(lines);
      if (c.phase === "failed") {
        panel.append(
          h("div", "hp-error", t("connect.cannot", { host: name })),
          h("div", "hp-time", t("connect.nextTry", { n: Math.max(0, Math.ceil((c.next - Date.now()) / 1000)) })),
        );
        const acts = h("div", "cp-acts");
        if (id) acts.append(button("cp-btn", t("connect.check"), () => settings({ check: id })));
        acts.append(
          button("cp-btn", t("connect.tryAgain"), () => retry(c)),
          button("cp-btn", t("connect.copyOutput"), () => void copyText(c.lines.join("\n"))),
        );
        panel.append(acts);
      } else panel.append(h("div", "hp-time", t("connect.elapsed", { n: Math.floor((Date.now() - c.since) / 1000) })));
    });
    panel.append(
      button("hp-foot", t("hosts.addSsh"), () => settings({ add: true })),
      button("hp-foot", t("hosts.manage"), () => settings({})),
    );
    if (focused) panel.querySelector<HTMLElement>('[data-action="' + focused + '"]')?.focus();
  }
  closePanel = close;
  document.addEventListener("pointerdown", outside, true);
  document.addEventListener("keydown", key, true);
  window.addEventListener("skiff-host-panel", draw);
  const timer = window.setInterval(draw, 1000);
  draw();
  panel.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
}

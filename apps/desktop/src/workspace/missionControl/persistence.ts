/** The panel state on this computer, per Host. */
export function createPersistence(state: { panelOpen: boolean; keptFor: string | null | undefined }, shownHost: () => string | null) {
  const key = () => `skiff.mission.${shownHost() ?? "local"}`;
  function saveKept() {
    try {
      localStorage.setItem(key(), JSON.stringify({ panel: state.panelOpen }));
    } catch {
      /* The panel state lasts for this run. */
    }
  }
  function loadKept() {
    const host = shownHost();
    if (state.keptFor === host) return;
    state.keptFor = host;
    let kept: { panel?: boolean } = {};
    try {
      const value: unknown = JSON.parse(localStorage.getItem(key()) ?? "{}");
      if (value && typeof value === "object") kept = value as typeof kept;
    } catch {
      /* Start as laid out. */
    }
    state.panelOpen = kept.panel !== false;
    if ("offs" in kept || "order" in kept || "folded" in kept || "opened" in kept) saveKept();
  }
  return { loadKept, saveKept };
}

import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real pane menu. The child pane of a Skipper split offers no new panes.
const stub = new Proxy({}, { get: (_, k) => (k === "__esModule" ? false : () => undefined) });
const withStubs = (imports: Record<string, unknown>) => new Proxy(imports, { get: (t, k: string) => t[k] ?? stub });

function labels(child: string | null) {
  const s = { id: "kid", title: "kid", cwd: "/harbor/api" };
  let items: any[] = [];
  const m: any = moduleFromSource(
    "../src/workspace/menus/pane.ts",
    withStubs({
      "../../app/state": { S: { focused: "kid", maximized: null, focusMode: false }, sessions: new Map([["kid", s]]) },
      "../../app/stateQueries": withStubs({ shownIds: () => ["sk", "kid"], splitFull: () => false, skipperSplit: () => (child ? { skipper: "sk", child, layout: null } : null) }),
      "../../app/skiffAgent": withStubs({ isSkiffAgent: () => false }),
      "../../app/hostCaps": { hostCan: () => true },
      "../../app/keys": { keyLabel: () => "" },
      "../model": withStubs({ isProjectSkipper: () => false }),
      "../../terminal/resume": withStubs({ resumeOffer: () => null }),
      "./end": { endEntry: () => ({ label: "End session…" }), sessionLabel: () => "kid", themeEntry: () => ({ label: "Session theme…" }) },
      "../../ui/contextMenu": { ctxMenu: { open: (_x: number, _y: number, _t: string, list: any[]) => (items = list) } },
    }),
    { console },
    "",
  );
  m.paneMenu("kid", 0, 0);
  return items.map((i) => i.label);
}

test("a Skipper split's child pane has no add items; a group pane keeps them", () => {
  const companion = labels("kid");
  assert.ok(!companion.includes("Add pane right…") && !companion.includes("Add pane below…"), companion.join(", "));
  assert.ok(companion.includes("End session…"));
  const group = labels(null);
  assert.ok(group.includes("Add pane right…") && group.includes("Add pane below…"), group.join(", "));
});

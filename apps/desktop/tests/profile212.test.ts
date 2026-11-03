import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, workspaceHarness } from "./profile212Harness.ts";

test("a save conflict keeps edits and offers Save As", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw");
  await h.profile.openWorkspace("/work.skiffw");
  h.profile.editWorkspace();
  h.elements().find((e) => e.type === "text")!.value = "Edited";
  h.button("Apply").click();
  h.calls.conflict = true;
  h.profile.saveWorkspace();
  await flush();
  assert.equal(h.profile.workspaceEdited(), true);
  assert.equal(h.profile.workspaceNotice().disk, "changed");
  assert.deepEqual(h.calls.expects, ["read"]);
});

test("Save As checks the destination revision and preserves edits on conflict", async () => {
  const h = workspaceHarness();
  h.fixture("/work.skiffw");
  h.fixture("/other.skiffw");
  await h.profile.openWorkspace("/work.skiffw");
  h.profile.editWorkspace();
  h.elements().find((e) => e.type === "text")!.value = "Edited";
  h.button("Apply").click();
  h.calls.picked = "/other.skiffw";
  h.calls.conflict = true;
  h.profile.saveWorkspaceAs();
  await flush();
  assert.deepEqual(h.calls.expects, ["read"]);
  assert.equal(h.profile.workspaceEdited(), true);
  assert.equal(h.files.get("/other.skiffw").profile.name, "/other.skiffw");
});

for (const disk of ["changed", "missing"] as const) {
  test(`a ${disk} workspace file is reported on focus`, async () => {
    const h = workspaceHarness();
    h.fixture("/work.skiffw");
    await h.profile.openWorkspace("/work.skiffw");
    await h.profile.startWorkspaces();
    if (disk === "changed") h.files.get("/work.skiffw").revision = "new";
    else h.files.delete("/work.skiffw");
    h.events.get("focus")!();
    await flush();
    assert.equal(h.profile.workspaceNotice().disk, disk);
  });
}

test("closing the workspace status menu returns focus to its button", () => {
  const h = workspaceHarness();
  const button = h.h("button");
  h.document.body.append(button);
  h.profile.review.workspaceMenu(button);
  h.menus.at(-1)!();
  assert.equal(h.document.activeElement, button);
});

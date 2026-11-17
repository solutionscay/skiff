import { test } from "node:test";
import assert from "node:assert/strict";
import { AVATAR_SWATCHES, avatarColor } from "../src/appearance/skipperAvatar.ts";

// The face's color: a picked color wins; else a swatch, never the pale theme color that came out grey.
test("a picked color wins; with none, a swatch near the theme's hue; a grey theme takes any swatch", () => {
  assert.equal(avatarColor("tidepool-web", "#12AB34", "#e0ccff"), "#12ab34");
  const near = (c: string) => [-1, 0, 1].map((d) => AVATAR_SWATCHES[(AVATAR_SWATCHES.indexOf(c) + d + 12) % 12]);
  // foot's pale lavender: a violet family swatch, not the lavender itself.
  const purple = near("#b35cff");
  for (const name of ["tidepool-web", "lighthouse-docs", "quill-notes", "fleet:Harbor", "Admiral"]) {
    assert.ok(purple.includes(avatarColor(name, null, "#e0ccff")), name);
    assert.ok(AVATAR_SWATCHES.includes(avatarColor(name, "not a color", "#c2c2c2")), name);
  }
});

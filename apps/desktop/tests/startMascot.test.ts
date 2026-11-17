import { test } from "node:test";
import assert from "node:assert/strict";
import { mascotSources } from "../src/app/startMascot.ts";

/** The start screen plays one ordinary clip: H.264 first, which WKWebView and
 *  WebView2 play, then VP9 for a WebKitGTK without H.264. Reduced motion keeps the still. */

test("the clip tries H.264, then VP9", () => {
  assert.deepEqual(mascotSources(false).map((s) => s.type), ["video/mp4", "video/webm"]);
  assert.match(mascotSources(false)[0].src, /start-mascot\.mp4$/);
  assert.match(mascotSources(false)[1].src, /start-mascot\.webm$/);
});

test("reduced motion keeps the still", () => {
  assert.deepEqual(mascotSources(true), []);
});

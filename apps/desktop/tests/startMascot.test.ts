import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/** The start screen plays one ordinary clip from index.html, without the app's code:
 *  H.264 first, which WKWebView and WebView2 play, then VP9 for a WebKitGTK without
 *  H.264. The still is the poster, and reduced motion keeps the still. */

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const start = html.slice(html.indexOf('<div id="start"'), html.indexOf('<div id="app"'));

test("the clip is in index.html and tries H.264, then VP9", () => {
  const sources = [...start.matchAll(/<source src="([^"]+)" type="([^"]+)"/g)].map((m) => [m[1], m[2]]);
  assert.deepEqual(sources, [
    ["./src/assets/start-mascot.mp4", "video/mp4"],
    ["./src/assets/start-mascot.webm", "video/webm"],
  ]);
  const video = start.match(/<video [^>]*>/)?.[0] ?? "";
  for (const attr of ["muted", "loop", "autoplay", "playsinline"]) assert.match(video, new RegExp(` ${attr}[ >]`));
  assert.doesNotMatch(video, / controls/);
});

test("the poster is the still", () => {
  assert.match(start, /<video [^>]*poster="\.\/src\/assets\/start-mascot\.webp"/);
});

test("reduced motion keeps the still", () => {
  assert.match(start, /matchMedia\("\(prefers-reduced-motion: reduce\)"\)\.matches\) return; v\.pause\(\);.*i\.src = v\.poster;.*v\.replaceWith\(i\)/);
});

test("the app's code does not swap the mascot", () => {
  assert.doesNotMatch(readFileSync(new URL("../src/main.ts", import.meta.url), "utf8"), /startMascot/);
});

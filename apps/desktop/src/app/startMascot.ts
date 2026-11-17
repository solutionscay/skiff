/**
 * The start screen's mascot. index.html shows the still (the clip's first
 * frame). The clip takes its place, with the still as its poster until the
 * first frame and when the clip does not play.
 */

/** H.264 plays in WKWebView and WebView2. WebKitGTK without an H.264 decoder plays the VP9. */
const SOURCES = [
  { src: new URL("../assets/start-mascot.mp4", import.meta.url).href, type: "video/mp4" },
  { src: new URL("../assets/start-mascot.webm", import.meta.url).href, type: "video/webm" },
];

/** The clip's sources in the order the webview tries them; none under reduced motion. */
export function mascotSources(reducedMotion: boolean) {
  return reducedMotion ? [] : SOURCES;
}

/** Put the clip in place of the still. */
export function startMascot(start: HTMLElement) {
  const img = start.querySelector("img");
  const sources = mascotSources(matchMedia("(prefers-reduced-motion: reduce)").matches);
  if (!img || !sources.length) return;
  const video = document.createElement("video");
  video.muted = true;
  video.loop = true;
  video.autoplay = true;
  video.playsInline = true;
  video.disablePictureInPicture = true;
  video.poster = img.src;
  for (const { src, type } of sources) video.append(Object.assign(document.createElement("source"), { src, type }));
  img.replaceWith(video);
}

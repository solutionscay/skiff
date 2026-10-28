/** What the page says about an app update. Kept apart from Tauri for the page tests. */

const RELEASES = "https://github.com/solutionscay/skiff/releases";

/** The release that offers the update, for an install that cannot run here. */
export function releasePage(version: string): string {
  return `${RELEASES}/tag/v${version.replace(/^v/, "")}`;
}

/** One error line for a step that failed: what failed, then the updater's reason. */
export function failLine(step: "check" | "download" | "install", e: unknown): string {
  const reason = String(e instanceof Error ? e.message : e).trim().replace(/\.$/, "");
  const what = { check: "The update check failed", download: "The update did not download", install: "The update did not install" }[step];
  return reason ? `${what}: ${reason}.` : `${what}.`;
}

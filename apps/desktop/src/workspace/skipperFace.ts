/**
 * A Skipper's face: the generated avatar chosen in its own folder
 * (`avatar.toml` in the Skiff agent's folder, `fleets/<fleet>/` or
 * `projects/<project>/`). Skiff writes a random one there the first time it
 * reads a Skipper's folder, so each face stays. Until then, and on a remote
 * Host, the face is generated from the name, and the Admiral's is the gold
 * bicorne. The face sets --pc to the project color, so the body follows a
 * color or theme change.
 *
 * The choices are this Mac's, as project icons are: a remote Host shows the
 * generated faces.
 */
import { invoke } from "@tauri-apps/api/core";
import { ADMIRAL_AVATAR, skipperAvatar } from "../appearance/skipperAvatar";
import { accent } from "../app/stateQueries";
import { S } from "../app/state";
import { render } from "../app/renderRequest";
import { dialogFrame, dialogHeader } from "../ui/dialogParts";
import { button, h } from "../ui/dom";

let chosen = new Map<string, number>();

/** Scopes already read, so a face with no choice asks once. */
const asked = new Set<string>();
let reading = false;

/** Every Skipper with a folder on this Mac: "" for the Admiral, then fleets and projects. */
export const avatarScopes = () => [...chosen.keys()];

/** Reads the choices again, then draws. Called with every projects read. */
export async function loadSkipperAvatars() {
  const list = await invoke<{ scope: string; seed: number }[]>("skipper_avatars").catch(() => []);
  const next = new Map(list.map((a) => [a.scope, a.seed]));
  if (JSON.stringify([...next]) === JSON.stringify([...chosen])) return;
  chosen = next;
  render("skipper-avatars");
}

/** What a face shows, for the row signatures. */
export const faceSig = (scope: string | null) => [chosen.get(scope ?? "") ?? null, scope === null ? null : accent(projectOf(scope))];

const projectOf = (scope: string) => S.projects.find((p) => p.name === scope) ?? null;

/** `scope`: a project's name, `fleet:<name>`, or null for the Admiral. `px` at zoom 1. `seed`: a face to show in place of the chosen one. */
export function skipperFace(scope: string | null, px: number, seed?: number): HTMLElement {
  const face = h("span", "skipper-face");
  face.style.setProperty("--face", String(px));
  // The Admiral's generated faces are gold, as its own is.
  face.style.setProperty("--pc", scope === null ? "var(--rank)" : accent(projectOf(scope)));
  const key = scope ?? "";
  const n = seed ?? chosen.get(key);
  // A new Skipper's folder gets its random face on the next read.
  if (seed === undefined && n === undefined && !asked.has(key)) {
    asked.add(key);
    if (!reading) {
      reading = true;
      queueMicrotask(() => void loadSkipperAvatars().finally(() => (reading = false)));
    }
  }
  face.innerHTML = scope === null && !n ? ADMIRAL_AVATAR : skipperAvatar(scope ?? "Admiral", n ?? 0);
  return face;
}

/**
 * Avatar…: twelve generated faces. A click picks one and closes; Shuffle
 * draws twelve more. Esc or a click outside closes with no change. The keys
 * go back to what held them.
 */
export function setAvatarDialog(scope: string | null, title: string, done: () => void) {
  const back = document.activeElement as HTMLElement | null;
  const { overlay, panel } = dialogFrame("confirm-panel av-panel", "dialog", title);
  const close = () => {
    overlay.remove();
    back?.focus?.();
  };
  const now = chosen.get(scope ?? "") ?? 1;
  let base = Math.floor((now - 1) / 12) * 12;
  const grid = h("div", "av-grid");
  const error = h("div", "prompt-error");
  error.setAttribute("role", "alert");
  const draw = () => {
    grid.replaceChildren(
      ...Array.from({ length: 12 }, (_, i) => {
        const seed = base + i + 1;
        const b = button("av-opt", "", async () => {
          try {
            await invoke("set_skipper_avatar", { scope: scope ?? "", seed });
            await loadSkipperAvatars();
            close();
            done();
          } catch (e) {
            error.textContent = String(e);
          }
        });
        b.setAttribute("aria-label", `Avatar ${seed}`);
        b.setAttribute("aria-pressed", String(seed === now));
        b.appendChild(skipperFace(scope, 32, seed));
        return b;
      }),
    );
  };
  const shuffle = button("av-act", "Shuffle", () => {
    base += 12;
    draw();
    grid.querySelector<HTMLButtonElement>(".av-opt")?.focus();
  });
  panel.append(dialogHeader(title, close), grid, shuffle, error);
  overlay.appendChild(panel);
  overlay.addEventListener("mousedown", (e) => {
    if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  });
  document.body.appendChild(overlay);
  draw();
  (grid.querySelector<HTMLButtonElement>('[aria-pressed="true"]') ?? grid.querySelector<HTMLButtonElement>(".av-opt"))?.focus();
}

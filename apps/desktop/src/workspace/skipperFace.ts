/**
 * A Skipper's face: the avatar chosen in its own folder (`avatar.toml` in
 * the Skiff agent's folder, `fleets/<fleet>/` or `projects/<project>/`),
 * else the one generated from its project's or fleet's name. The Admiral's
 * own is the gold bicorne. The face sets --pc to the project color, so the
 * body follows a color or theme change.
 *
 * The choices are this Mac's, as project icons are: a remote Host shows the
 * generated faces.
 */
import { invoke } from "@tauri-apps/api/core";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { ADMIRAL_AVATAR, skipperAvatar } from "../appearance/skipperAvatar";
import { accent } from "../app/stateQueries";
import { S } from "../app/state";
import { render } from "../app/renderRequest";
import { actionDialog } from "../ui/dialogParts";
import { button, h } from "../ui/dom";

type Chosen = { seed: number | null; image: string | null };

let chosen = new Map<string, Chosen>();

/** Reads the choices again, then draws. Called with every projects read. */
export async function loadSkipperAvatars() {
  const list = await invoke<({ scope: string } & Chosen)[]>("skipper_avatars").catch(() => []);
  const next = new Map(list.map((a) => [a.scope, { seed: a.seed, image: a.image }]));
  if (JSON.stringify([...next]) === JSON.stringify([...chosen])) return;
  chosen = next;
  render("skipper-avatars");
}

/** What a face shows, for the row signatures. */
export const faceSig = (scope: string | null) => [chosen.get(scope ?? "") ?? null, scope === null ? null : accent(projectOf(scope))];

const projectOf = (scope: string) => S.projects.find((p) => p.name === scope) ?? null;

/** `scope`: a project's name, `fleet:<name>`, or null for the Admiral. `px` at zoom 1. */
export function skipperFace(scope: string | null, px: number, pick?: Chosen): HTMLElement {
  const face = h("span", "skipper-face");
  face.style.setProperty("--face", String(px));
  // The Admiral's generated faces are gold, as its own is.
  face.style.setProperty("--pc", scope === null ? "var(--rank)" : accent(projectOf(scope)));
  const c = pick ?? chosen.get(scope ?? "");
  if (c?.image) {
    const img = h("img");
    img.src = c.image;
    img.alt = "";
    face.appendChild(img);
  } else if (scope === null && !c?.seed) face.innerHTML = ADMIRAL_AVATAR;
  else face.innerHTML = skipperAvatar(scope ?? "Admiral", c?.seed ?? 0);
  return face;
}

async function save(scope: string | null, avatar: string | null) {
  await invoke("set_skipper_avatar", { scope: scope ?? "", avatar });
  await loadSkipperAvatars();
}

/**
 * Set avatar…: the preview at each size and six generated faces. Shuffle
 * draws six more, From name goes back to the name's own. Image file… sets
 * the picture at once, as the project icon's Choose file… does.
 */
export function setAvatarDialog(scope: string | null, title: string, defaultPath?: string) {
  const now = chosen.get(scope ?? "");
  let seed = now?.seed ?? 0;
  let base = seed - (seed % 6);
  const preview = h("div", "av-preview");
  const grid = h("div", "av-grid");
  const draw = () => {
    const at = (px: number, label: string) => {
      const row = h("div", "av-at");
      row.append(skipperFace(scope, px, { seed, image: null }), h("span", "", label), h("span", "av-px", String(px)));
      return row;
    };
    const big = h("div", "av-big");
    big.appendChild(skipperFace(scope, 80, { seed, image: null }));
    const sizes = h("div", "av-sizes");
    sizes.append(at(26, "Skippers list"), at(16, "Tree, pane, card"));
    preview.replaceChildren(big, sizes);
    grid.replaceChildren(
      ...Array.from({ length: 6 }, (_, i) => {
        const s = base + i;
        const b = button("av-opt", "", () => {
          seed = s;
          draw();
          grid.querySelectorAll<HTMLButtonElement>(".av-opt")[i].focus();
        });
        b.setAttribute("aria-label", s ? `Avatar ${s}` : "From name");
        b.setAttribute("aria-pressed", String(s === seed));
        b.appendChild(skipperFace(scope, 32, { seed: s, image: null }));
        return b;
      }),
    );
  };
  const acts = h("div", "av-acts");
  const dlg = actionDialog({
    title,
    content: [preview, grid, acts],
    action: "Set avatar",
    tone: "go",
    submit: () => save(scope, seed ? `gen:${seed}` : null),
  });
  acts.append(
    button("av-act", "Shuffle", () => {
      base += 6;
      seed = base;
      draw();
    }),
    button("av-act", "Image file…", async () => {
      const file = await openFile({
        title,
        defaultPath,
        filters: [{ name: "Images", extensions: ["png", "svg", "jpg", "jpeg", "webp", "gif"] }],
      }).catch(() => null);
      if (typeof file !== "string") return;
      try {
        await save(scope, file);
        dlg.close();
      } catch (e) {
        dlg.error.textContent = String(e);
      }
    }),
    button("av-act", "From name", () => {
      base = seed = 0;
      draw();
    }),
  );
  draw();
}

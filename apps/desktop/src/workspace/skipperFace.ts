/**
 * A Skipper's face: the avatar its project chose in projects.toml
 * (`skipper_avatar`), else the one generated from its project's or fleet's
 * name. The Admiral has one fixed face. The face sets --pc to the project
 * color, so the body follows a color or theme change.
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
import type { Project } from "../platform/types";

type Chosen = { seed: number | null; image: string | null };

let chosen = new Map<string, Chosen>();

/** Reads the choices again, then draws. Called with every projects read. */
export async function loadSkipperAvatars() {
  const list = await invoke<({ project: string } & Chosen)[]>("skipper_avatars").catch(() => []);
  const next = new Map(list.map((a) => [a.project, { seed: a.seed, image: a.image }]));
  if (JSON.stringify([...next]) === JSON.stringify([...chosen])) return;
  chosen = next;
  render("skipper-avatars");
}

/** What a face shows, for the row signatures. */
export const faceSig = (scope: string | null) => (scope === null ? null : [chosen.get(scope) ?? null, accent(projectOf(scope))]);

const projectOf = (scope: string) => S.projects.find((p) => p.name === scope) ?? null;

/** `scope`: a project's name, `fleet:<name>`, or null for the Admiral. `px` at zoom 1. */
export function skipperFace(scope: string | null, px: number, pick?: Chosen): HTMLElement {
  const face = h("span", "skipper-face");
  face.style.setProperty("--face", String(px));
  if (scope === null) {
    face.innerHTML = ADMIRAL_AVATAR;
    return face;
  }
  face.style.setProperty("--pc", accent(projectOf(scope)));
  const c = pick ?? chosen.get(scope);
  if (c?.image) {
    const img = h("img");
    img.src = c.image;
    img.alt = "";
    face.appendChild(img);
  } else face.innerHTML = skipperAvatar(scope, c?.seed ?? 0);
  return face;
}

async function save(p: Project, avatar: string | null) {
  await invoke("set_skipper_avatar", { project: p.name, avatar });
  await loadSkipperAvatars();
}

/**
 * Set avatar…: the preview at each size and six generated faces. Shuffle
 * draws six more, From name goes back to the name's own. Image file… sets
 * the picture at once, as the project icon's Choose file… does.
 */
export function setAvatarDialog(p: Project) {
  const now = chosen.get(p.name);
  let seed = now?.seed ?? 0;
  let base = seed - (seed % 6);
  const preview = h("div", "av-preview");
  const grid = h("div", "av-grid");
  const draw = () => {
    const at = (px: number, label: string) => {
      const row = h("div", "av-at");
      row.append(skipperFace(p.name, px, { seed, image: null }), h("span", "", label), h("span", "av-px", String(px)));
      return row;
    };
    const big = h("div", "av-big");
    big.appendChild(skipperFace(p.name, 80, { seed, image: null }));
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
        b.appendChild(skipperFace(p.name, 32, { seed: s, image: null }));
        return b;
      }),
    );
  };
  const acts = h("div", "av-acts");
  const dlg = actionDialog({
    title: `Skipper avatar: ${p.name}`,
    content: [preview, grid, acts],
    action: "Set avatar",
    tone: "go",
    submit: () => save(p, seed ? `gen:${seed}` : null),
  });
  acts.append(
    button("av-act", "Shuffle", () => {
      base += 6;
      seed = base;
      draw();
    }),
    button("av-act", "Image file…", async () => {
      const file = await openFile({
        title: `Avatar for ${p.name} Skipper`,
        defaultPath: p.path,
        filters: [{ name: "Images", extensions: ["png", "svg", "jpg", "jpeg", "webp", "gif"] }],
      }).catch(() => null);
      if (typeof file !== "string") return;
      try {
        await save(p, file);
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

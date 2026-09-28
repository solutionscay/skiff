import { invoke } from "@tauri-apps/api/core";
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import type { FolderInfo, Project } from "./types";

const PALETTE = ["#b69cff", "#f28fd0", "#7ee0cb", "#e0c07e", "#9ec1ff", "#ff9e7a", "#c3e88d", "#d0c2ff"];

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

function shortName(name: string): string {
  return name.replace(/[^A-Za-z0-9]/g, "").slice(0, 3).toLowerCase();
}

function tilde(p: string, home: string | null): string {
  return home && p.startsWith(home + "/") ? "~" + p.slice(home.length) : p;
}

/**
 * Modal dialog: pick a folder, check it with the daemon, choose name, rail
 * label and color, then write it to projects.toml.
 */
export function createAddProject(onAdded: (p: Project) => void, onClose: () => void) {
  const overlay = el("div");
  overlay.id = "add-project";
  overlay.hidden = true;
  overlay.innerHTML = `
    <div class="ap-panel" role="dialog" aria-modal="true" aria-labelledby="ap-title">
      <div class="ap-head"><div id="ap-title">Add project</div><kbd>Esc</kbd></div>
      <div class="ap-row">
        <label for="ap-folder" class="ap-label">FOLDER</label>
        <input id="ap-folder" class="ap-input mono" readonly placeholder="No folder chosen" />
        <button type="button" class="ap-side-btn" data-act="browse">Browse…</button>
      </div>
      <div class="ap-status" role="status"><span class="ap-dot"></span><span class="ap-status-text"></span></div>
      <div class="ap-fields">
        <div class="ap-row">
          <label for="ap-name" class="ap-label">NAME</label>
          <input id="ap-name" class="ap-input" spellcheck="false" autocomplete="off" />
          <label for="ap-short" class="ap-label ap-label-mid">RAIL</label>
          <input id="ap-short" class="ap-input ap-short mono" maxlength="3" spellcheck="false" autocomplete="off" />
        </div>
        <div class="ap-row">
          <div class="ap-label" id="ap-color-label">COLOR</div>
          <div class="ap-swatches" role="radiogroup" aria-labelledby="ap-color-label"></div>
        </div>
        <div class="ap-row">
          <div class="ap-label">ICON</div>
          <div class="ap-icon-cur"><span class="ap-icon-box"></span><span class="ap-icon-text mono"></span></div>
          <button type="button" class="ap-mid-btn" data-act="icon-toggle"></button>
          <button type="button" class="ap-side-btn" data-act="icon-browse">Browse…</button>
        </div>
        <div class="ap-row ap-preview">
          <div class="ap-label">PREVIEW</div>
          <div class="ap-chip mono"></div>
          <div class="ap-card"><div class="ap-card-name"></div><div class="ap-card-path mono"></div></div>
        </div>
      </div>
      <div class="ap-error" role="alert" hidden></div>
      <div class="ap-foot">
        <div class="ap-note"></div>
        <button type="button" class="ap-cancel" data-act="cancel">Cancel</button>
        <button type="button" class="ap-submit" data-act="submit">Add project</button>
      </div>
    </div>`;
  document.body.appendChild(overlay);

  const q = <T extends Element>(s: string) => overlay.querySelector(s) as T;
  const folderInput = q<HTMLInputElement>("#ap-folder");
  const nameInput = q<HTMLInputElement>("#ap-name");
  const shortInput = q<HTMLInputElement>("#ap-short");
  const status = q<HTMLDivElement>(".ap-status");
  const statusText = q<HTMLSpanElement>(".ap-status-text");
  const fields = q<HTMLDivElement>(".ap-fields");
  const swatches = q<HTMLDivElement>(".ap-swatches");
  const chip = q<HTMLDivElement>(".ap-chip");
  const cardName = q<HTMLDivElement>(".ap-card-name");
  const cardPath = q<HTMLDivElement>(".ap-card-path");
  const errorBox = q<HTMLDivElement>(".ap-error");
  const submitBtn = q<HTMLButtonElement>(".ap-submit");

  let folder: FolderInfo | null = null;
  let color = PALETTE[0];
  let shortEdited = false;
  /** detect: the repo's own icon (or letters if none); none: letters; custom: a chosen file. */
  let icon: { mode: "detect" | "none" | "custom"; path?: string; url?: string | null } = { mode: "detect" };
  const iconBox = q<HTMLSpanElement>(".ap-icon-box");
  const iconText = q<HTMLSpanElement>(".ap-icon-text");
  const iconToggle = q<HTMLButtonElement>("[data-act=icon-toggle]");
  const iconUrl = () => (icon.mode === "custom" ? icon.url ?? null : icon.mode === "detect" ? folder?.icon ?? null : null);
  let busy = false;
  let checking = false;
  let home: string | null = null;

  /** Why Add project cannot run yet, or null when it can. */
  function blocker(): string | null {
    if (checking) return "Still checking the folder…";
    if (!folder) return "Pick a folder first: click Browse…";
    if (!folder.root || folder.error) return folder.error ?? "Pick a folder in a git repository.";
    if (nameInput.value.trim() === "") return "Type a project name.";
    return null;
  }

  function render() {
    const ok = !!folder?.root && !folder.error;
    // Only a problem is worth a line: not a repo, or already added.
    status.hidden = checking || !folder || ok;
    status.classList.add("bad");
    statusText.textContent = folder?.error ?? "";
    fields.hidden = !ok;

    const url = iconUrl();
    iconBox.replaceChildren();
    if (url) {
      const img = el("img");
      img.src = url;
      img.alt = "";
      iconBox.appendChild(img);
    }
    iconText.textContent =
      icon.mode === "custom" ? icon.path ?? "" : icon.mode === "none" ? "letters" : folder?.icon_path ?? "none found, letters";
    iconToggle.textContent = icon.mode === "none" ? "Detect" : "None";
    iconToggle.hidden = icon.mode === "detect" && !folder?.icon;

    swatches.replaceChildren(
      ...PALETTE.map((hex) => {
        const b = el("button", "ap-swatch");
        b.type = "button";
        b.style.setProperty("--sw", hex);
        b.setAttribute("role", "radio");
        b.setAttribute("aria-checked", String(hex === color));
        b.setAttribute("aria-label", hex);
        b.title = hex;
        b.addEventListener("click", () => {
          color = hex;
          render();
        });
        return b;
      }),
    );

    overlay.style.setProperty("--pc", color);
    chip.replaceChildren();
    if (url) {
      const img = el("img");
      img.src = url;
      img.alt = "";
      chip.appendChild(img);
    } else {
      chip.textContent = shortInput.value;
    }
    cardName.textContent = nameInput.value;
    cardPath.textContent = folder?.root ? tilde(folder.root, home) : "";
    submitBtn.disabled = busy;
    submitBtn.classList.toggle("blocked", blocker() !== null);
    submitBtn.textContent = busy ? "Adding…" : "Add project";
  }

  async function browse() {
    const dir = await openFolder({ directory: true, title: "Pick a folder in a git repository" }).catch(() => null);
    if (typeof dir !== "string") return;
    folderInput.value = dir;
    errorBox.hidden = true;
    checking = true;
    folder = null;
    try {
      folder = await invoke<FolderInfo>("inspect_folder", { path: dir });
    } catch (e) {
      folder = { root: null, name: "", short: "", color, branch: null, worktrees: 0, icon: null, icon_path: null, error: String(e) };
    } finally {
      checking = false;
    }
    if (folder.root && !folder.error) {
      nameInput.value = folder.name;
      shortInput.value = folder.short;
      shortEdited = false;
      color = folder.color;
      icon = { mode: "detect" };
      folderInput.value = folder.root;
    }
    render();
    if (folder.root && !folder.error) nameInput.focus();
  }

  async function browseIcon() {
    const root = folder?.root;
    if (!root) return;
    const file = await openFolder({
      title: "Project icon",
      defaultPath: root,
      filters: [{ name: "Images", extensions: ["png", "svg", "ico", "jpg", "jpeg", "webp", "gif"] }],
    }).catch(() => null);
    if (typeof file !== "string") return;
    const url = await invoke<string | null>("read_icon", { path: file }).catch(() => null);
    if (!url) {
      errorBox.textContent = "That image cannot be used: it must be png, svg, ico, jpg, webp or gif, under 512 KB.";
      errorBox.hidden = false;
      return;
    }
    errorBox.hidden = true;
    icon = { mode: "custom", path: file.startsWith(root + "/") ? file.slice(root.length + 1) : file, url };
    render();
  }

  async function submit() {
    if (busy) return;
    const why = blocker();
    if (why || !folder?.root) {
      errorBox.textContent = why ?? "Pick a folder first.";
      errorBox.hidden = false;
      return;
    }
    busy = true;
    errorBox.hidden = true;
    render();
    try {
      const p = await invoke<Project>("add_project", {
        path: folder.root,
        name: nameInput.value.trim(),
        short: shortInput.value.trim() || null,
        color,
        icon: icon.mode === "detect" ? null : icon.mode === "none" ? "" : icon.path,
      });
      close();
      onAdded(p);
    } catch (e) {
      errorBox.textContent = String(e);
      errorBox.hidden = false;
    } finally {
      busy = false;
      render();
    }
  }

  function close() {
    overlay.hidden = true;
    onClose();
  }

  nameInput.addEventListener("input", () => {
    if (!shortEdited) shortInput.value = shortName(nameInput.value);
    render();
  });
  shortInput.addEventListener("input", () => {
    shortEdited = true;
    render();
  });
  overlay.addEventListener("click", (e) => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]")?.dataset.act;
    if (act === "browse") void browse();
    else if (act === "icon-browse") void browseIcon();
    else if (act === "icon-toggle") {
      icon = icon.mode === "none" ? { mode: "detect" } : { mode: "none" };
      render();
    } else if (act === "cancel") close();
    else if (act === "submit") void submit();
    else if (e.target === overlay) close();
  });
  overlay.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      void submit();
    } else if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const f = [...overlay.querySelectorAll<HTMLElement>("button:not([disabled]), input")].filter((x) => x.offsetParent);
      const i = f.indexOf(document.activeElement as HTMLElement);
      const next = f[(i + (e.shiftKey ? -1 : 1) + f.length) % f.length];
      if (next) {
        e.preventDefault();
        next.focus();
      }
    }
  });

  return {
    async open() {
      folder = null;
      icon = { mode: "detect" };
      busy = false;
      folderInput.value = "";
      nameInput.value = "";
      shortInput.value = "";
      errorBox.hidden = true;
      overlay.hidden = false;
      render();
      q<HTMLButtonElement>("[data-act=browse]").focus();
      const h = await import("@tauri-apps/api/path").then((m) => m.homeDir()).catch(() => null);
      home = h ? h.replace(/\/+$/, "") : null;
      render();
    },
    get isOpen() {
      return !overlay.hidden;
    },
  };
}

/**
 * The keymap. The terminal owns Ctrl+letter; Skiff uses Ctrl+Shift+letter,
 * like foot and GNOME Terminal. `[keys]` in projects.toml overrides any
 * action: `palette = "ctrl+shift+p"`.
 */

export type Action =
  | "palette" | "new-session" | "new-worktree" | "add-project"
  | "split-right" | "split-down" | "close-pane" | "next-waiting" | "back" | "rename"
  | "copy" | "paste" | "find" | "font-bigger" | "font-smaller" | "font-reset" | "settings" | "quit"
  | "focus-left" | "focus-right" | "focus-up" | "focus-down"
  | "region-next" | "region-prev" | "focus-list" | "session-next" | "session-prev" | "shortcuts";

export const DEFAULTS: Record<Action, string> = {
  palette: "ctrl+shift+p",
  "new-session": "ctrl+shift+t",
  "new-worktree": "ctrl+shift+n",
  "add-project": "ctrl+shift+o",
  "split-right": "ctrl+shift+d",
  "split-down": "ctrl+shift+s",
  "close-pane": "ctrl+shift+w",
  "next-waiting": "ctrl+shift+j",
  back: "ctrl+shift+b",
  rename: "ctrl+shift+r",
  copy: "ctrl+shift+c",
  paste: "ctrl+shift+v",
  find: "ctrl+shift+f",
  "font-bigger": "ctrl+=",
  "font-smaller": "ctrl+-",
  "font-reset": "ctrl+0",
  settings: "ctrl+,",
  quit: "ctrl+shift+q",
  "focus-left": "ctrl+alt+left",
  "focus-right": "ctrl+alt+right",
  "focus-up": "ctrl+alt+up",
  "focus-down": "ctrl+alt+down",
  "region-next": "f6",
  "region-prev": "shift+f6",
  "focus-list": "ctrl+shift+l",
  "session-next": "ctrl+shift+down",
  "session-prev": "ctrl+shift+up",
  shortcuts: "f1",
};

interface Combo { ctrl: boolean; shift: boolean; alt: boolean; key: string }

const ALIASES: Record<string, string> = {
  left: "arrowleft", right: "arrowright", up: "arrowup", down: "arrowdown",
  plus: "=", "+": "=", minus: "-", comma: ",", esc: "escape", space: " ",
};

function parse(spec: string): Combo | null {
  const parts = spec.toLowerCase().split("+").map((p) => p.trim());
  // "ctrl++" splits into an empty last part: the key is "+".
  if (parts.at(-1) === "" && parts.length > 1) parts.splice(-2, 2, "+");
  const key = parts.pop();
  if (!key) return null;
  return {
    ctrl: parts.includes("ctrl"),
    shift: parts.includes("shift"),
    alt: parts.includes("alt"),
    key: ALIASES[key] ?? key,
  };
}

/** The key a keydown means, independent of Shift and keyboard layout for letters and digits. */
function eventKey(e: KeyboardEvent): string {
  const m = /^(Key|Digit)(.)$/.exec(e.code);
  if (m) return m[2].toLowerCase();
  if (e.code === "Equal") return "=";
  if (e.code === "Minus") return "-";
  if (e.code === "Comma") return ",";
  return e.key.toLowerCase();
}

let table: [Action, Combo][] = [];
let labels: Record<string, string> = {};
let combos: Record<string, Combo> = {};

/** Builds the keymap from the defaults and `[keys]` overrides. */
export function setKeymap(overrides: Record<string, string>) {
  const specs: Record<string, string> = { ...DEFAULTS };
  for (const [k, v] of Object.entries(overrides)) if (k in DEFAULTS) specs[k] = v;
  table = [];
  labels = {};
  combos = {};
  for (const [action, spec] of Object.entries(specs) as [Action, string][]) {
    const c = parse(spec);
    if (!c) continue;
    table.push([action, c]);
    labels[action] = label(c);
    combos[action] = c;
  }
}
setKeymap({});

/** The action a keydown triggers, if any. */
export function actionFor(e: KeyboardEvent): Action | null {
  if (e.metaKey) return null;
  const key = eventKey(e);
  for (const [action, c] of table) {
    // Ctrl+= and Ctrl+- also work with Shift held (Ctrl++ on most layouts).
    const anyShift = c.key === "=" || c.key === "-";
    if (c.ctrl === e.ctrlKey && (anyShift || c.shift === e.shiftKey) && c.alt === e.altKey && c.key === key) return action;
  }
  return null;
}

function label(c: Combo): string {
  const k = c.key.startsWith("arrow") ? c.key.slice(5)[0].toUpperCase() + c.key.slice(6) : c.key.length === 1 ? c.key.toUpperCase() : c.key.toUpperCase();
  return [c.ctrl && "Ctrl", c.alt && "Alt", c.shift && "Shift", k].filter(Boolean).join("+");
}

/** "Ctrl+Shift+T", for menus and the palette. */
export function keyLabel(action: Action): string {
  return labels[action] ?? "";
}

const MENU_KEYS: Record<string, string> = { "=": "Equal", "-": "Minus", ",": "Comma", " ": "Space", escape: "Escape" };

/** The action's key as the menu bar spells it ("Ctrl+Shift+T"), or "" for none. */
export function menuAccel(action: Action): string {
  const c = combos[action];
  if (!c) return "";
  const k = c.key.startsWith("arrow") ? "Arrow" + c.key[5].toUpperCase() + c.key.slice(6) : MENU_KEYS[c.key] ?? c.key.toUpperCase();
  return [c.ctrl && "Ctrl", c.alt && "Alt", c.shift && "Shift", k].filter(Boolean).join("+");
}

/** What each action does, for the shortcut sheet. Grouped as the menu bar is. */
export const DESCRIBE: [string, Action, string][] = [
  ["File", "new-session", "New session in the focused worktree"],
  ["File", "new-worktree", "New worktree"],
  ["File", "add-project", "Add project"],
  ["File", "settings", "Settings"],
  ["File", "quit", "Quit Skiff. Sessions keep running in skiffd"],
  ["Edit", "copy", "Copy the terminal selection"],
  ["Edit", "paste", "Paste into the terminal"],
  ["Edit", "find", "Find in the terminal"],
  ["Edit", "rename", "Rename the session"],
  ["View", "palette", "Command palette"],
  ["View", "split-right", "Add pane right"],
  ["View", "split-down", "Add pane below"],
  ["View", "close-pane", "Close pane (the session keeps running)"],
  ["View", "next-waiting", "Next waiting session"],
  ["View", "back", "Back to the last session"],
  ["View", "focus-left", "Focus the pane to the left"],
  ["View", "focus-right", "Focus the pane to the right"],
  ["View", "focus-up", "Focus the pane above"],
  ["View", "focus-down", "Focus the pane below"],
  ["View", "font-bigger", "Bigger text"],
  ["View", "font-smaller", "Smaller text"],
  ["View", "font-reset", "Reset text size"],
  ["Help", "region-next", "Next region: rail, new worktree, session list, terminal"],
  ["Help", "region-prev", "Previous region"],
  ["Help", "shortcuts", "This sheet"],
];

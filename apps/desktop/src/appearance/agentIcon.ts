import type { AgentInfo, SessionInfo } from "../platform/types";
import { ink } from "./appTheme";
import { ownTheme, signatureColor } from "./themes";
import { basename } from "../workspace/model";

export type AgentKind = "claude" | "codex" | "gemini" | "grok" | "opencode" | "shell" | "other";

const BY_PROGRAM: Record<string, AgentKind> = {
  claude: "claude",
  codex: "codex",
  gemini: "gemini",
  agy: "gemini",
  grok: "grok",
  opencode: "opencode",
};
const SHELLS = new Set(["bash", "zsh", "fish", "sh", "dash", "nu", "elvish", "xonsh", "ksh", "tcsh"]);

function knownProgram(value: string | null | undefined): AgentKind | undefined {
  if (!value) return undefined;
  // Agent titles can start with a status glyph, for example "✳ Claude Code".
  // Find a known program word instead of assuming the first title word is it.
  for (const word of value.toLowerCase().split(/[^a-z0-9]+/)) {
    const kind = BY_PROGRAM[word];
    if (kind) return kind;
  }
}

/**
 * Which icon a session gets: the active program's terminal title first, then
 * the + menu's agent id and original command. An interrupted agent leaves its
 * launch label behind when the fallback shell starts a different agent.
 */
export function agentKind(s: SessionInfo): AgentKind {
  const titled = knownProgram(s.title);
  if (titled) return titled;
  const labeled = knownProgram(s.label);
  if (labeled) return labeled;
  const prog = basename(s.command).toLowerCase();
  if (BY_PROGRAM[prog]) return BY_PROGRAM[prog];
  if (s.role === "shell" || SHELLS.has(prog)) return "shell";
  return "other";
}

// Plain glyphs in a 16px box. Not vendor logos: a shape and a color per agent.
const GLYPHS: Record<AgentKind, string> = {
  claude:
    '<g stroke="#d97757" stroke-width="2" stroke-linecap="round"><path d="M8 1.5v13M1.5 8h13M3.4 3.4l9.2 9.2M12.6 3.4l-9.2 9.2"/></g>',
  codex:
    '<path d="M8 1.5l5.6 3.25v6.5L8 14.5l-5.6-3.25v-6.5z" fill="none" stroke="#10a37f" stroke-width="1.8" stroke-linejoin="round"/><circle cx="8" cy="8" r="1.8" fill="#10a37f"/>',
  gemini:
    '<path d="M8 1c.6 4.4 2.6 6.4 7 7-4.4.6-6.4 2.6-7 7-.6-4.4-2.6-6.4-7-7 4.4-.6 6.4-2.6 7-7z" fill="#8ab4f8"/>',
  grok:
    '<circle cx="8" cy="8" r="5.8" fill="none" stroke="#e6e8eb" stroke-width="1.8"/><path d="M3.2 12.8L12.8 3.2" stroke="#e6e8eb" stroke-width="1.8" stroke-linecap="round"/>',
  opencode:
    '<path d="M5.5 2.5H2.5v11h3M10.5 2.5h3v11h-3" fill="none" stroke="#e0c07e" stroke-width="1.8" stroke-linecap="square"/>',
  shell:
    '<path d="M2.5 4l4 4-4 4M8 12.5h5.5" fill="none" stroke="#9aa3ae" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>',
  other:
    '<rect x="2" y="3" width="12" height="10" fill="none" stroke="#9aa3ae" stroke-width="1.6"/><path d="M4.5 6.5l2 1.5-2 1.5" fill="none" stroke="#9aa3ae" stroke-width="1.4"/>',
};

/**
 * A session with a terminal theme of its own draws its glyph in the theme's
 * signature color. It follows the session, so it holds inside any group.
 */
export function agentIcon(s: SessionInfo, size = 14): HTMLSpanElement {
  const span = iconOf(agentKind(s), size);
  const t = ownTheme(s);
  if (t) {
    span.title = `Terminal theme: ${t.name}`;
    const c = ink(signatureColor(t));
    span.innerHTML = span.innerHTML.replace(/#[0-9a-f]{6}/gi, c);
  }
  return span;
}

/** The icon a new session of this agent gets. `null` is a plain shell. */
export function launchIcon(a: AgentInfo | null, size = 14): HTMLSpanElement {
  if (!a) return iconOf("shell", size);
  const prog = basename(a.command.trim().split(/\s+/)[0] ?? "");
  return iconOf(BY_PROGRAM[a.id] ?? BY_PROGRAM[prog] ?? "other", size);
}

function iconOf(kind: AgentKind, size: number): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = `agent-icon ${kind}`;
  span.setAttribute("aria-hidden", "true");
  span.innerHTML = `<svg width="${size}" height="${size}" viewBox="0 0 16 16">${GLYPHS[kind].replace(/#[0-9a-f]{6}/gi, ink)}</svg>`;
  return span;
}

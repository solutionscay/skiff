# AGENTS.md

Instructions for coding agents (and any other contributor) working in this repository.

## Terminology

Read the [wiki Glossary](https://github.com/solutionscay/skiff/wiki/Glossary) before
changing anything under `apps/desktop/src` that touches the sidebar, the session tree, or
keyboard navigation — `sidebar.ts`, `keyboard.ts`, `keys.ts`, `view.ts`, `layout.ts`,
`terminal.ts`. It defines the terms this codebase uses for its own concepts (rail,
sidebar/tree, pane, split, group, region, roving item, and the current keybinding scheme),
so new code and comments stay consistent with the existing ones instead of inventing
competing names for the same thing.

## Design

Read the [wiki Design page](https://github.com/solutionscay/skiff/wiki/Design) before
changing anything the user sees or presses: styles, menus, dialogs, keys, labels or
themes. Claude Code and Codex load the same rules as the `skiff-design` skill
(`.agents/skills/skiff-design`).

## Keys

Before you add or change a keyboard shortcut, load the `skiff-keys` skill
(`.agents/skills/skiff-keys`). It lists where each key lives and what to keep in step.

## Development

See the README's [Development](README.md#development) section for build and run
instructions, and its [Layout](README.md#layout) section for what lives where.

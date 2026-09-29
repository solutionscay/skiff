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

## Development

See the README's [Development](README.md#development) section for build and run
instructions, and its [Layout](README.md#layout) section for what lives where.

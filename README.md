<p align="center"><img src=".github/skiff.png" width="160" alt="Skiff mascot: a small boat with a robot at the helm"></p>

<h1 align="center">Skiff</h1>

<p align="center">
  <a href="https://github.com/solutionscay/skiff/releases/latest"><img src="https://img.shields.io/github/v/release/solutionscay/skiff?include_prereleases&label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/solutionscay/skiff" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS-lightgrey" alt="Platforms: Linux and macOS">
</p>

Skiff is a desktop app for running many coding-agent sessions at once, on Linux and
macOS. It keeps them in order across projects and Git worktrees. It shows where each
session is and what it needs: a bell when an agent asks for approval, a flag when it
finished a turn out of sight.

<p align="center"><img src=".github/screenshot.webp" alt="Skiff on Linux: the Skiff agent, Admiral, at the top of the side panel, the skiff project with its worktrees below it, and four agent sessions that Admiral started, shown side by side in one group"></p>

## Philosophy

Skiff manages sessions. The apps you already have do everything else. Skiff does not
edit files: it opens them in the editor, diff tool or file manager you set in
Settings › Open with.

Sessions outlive the window. `skiffd`, a daemon, owns every terminal. Close the window
and the agents keep running.

Skiff stays out of your agents' setup. It installs no hooks or plugins and runs no
automations. It reads a session's state from its screen and from the program in front
of the terminal. All settings live in one file,
`projects.toml`.

## Install

Download the latest [release](https://github.com/solutionscay/skiff/releases/latest):

- Linux (x86_64): `.deb`, `.rpm` or AppImage
- macOS (Apple Silicon): `.dmg`, signed and notarized

The `skiff` command ships with the app. It drives sessions from a shell or from an agent.
See the [CLI](https://github.com/solutionscay/skiff/wiki/CLI) page.

## Start

1. Add a project: a Git repository or a plain folder.
2. Make worktrees in it, and start agent or shell sessions in each one.
3. Put related sessions side by side in a named group.
4. Click the top row, Admiral, to start the Skiff agent. It hands tasks to your sessions
   and tracks them in a ledger.

Press F1 for every key. The [wiki](https://github.com/solutionscay/skiff/wiki) has the
details: [Skiff agent](https://github.com/solutionscay/skiff/wiki/Skiff-agent),
[Open with](https://github.com/solutionscay/skiff/wiki/Open-with),
[skiffd](https://github.com/solutionscay/skiff/wiki/skiffd).

## Layout

```
crates/skiff-core    session model, wire protocol, projects.toml config, git, themes
crates/skiffd        daemon: owns every PTY, serves clients over a Unix socket
crates/skiff-client  async client for the socket; Skiff agent setup shared by the app and the CLI
crates/skiff-cli     the `skiff` command: sessions, groups, the Skiff agent's ledger and watcher
apps/desktop         Tauri v2 app, vanilla TypeScript + xterm.js
  src-tauri          Rust side: config, git, open with, the skiffd connection, menus
  src/app            state, actions, keyboard, startup connections
  src/ui             DOM builders, dialogs, menus
  src/workspace      session tree, fleets, layouts, files, project actions
  src/terminal       xterm runtime, panes, search, clipboard, font size
  src/appearance     themes, colors, icons, settings
  src/platform       wire types, IPC bytes, file actions
  src/diagnostics    latency traces and memory counts
  src/styles         styles by owner, imported in cascade order
```

## Development

```
cd apps/desktop && pnpm install && pnpm tauri dev
CARGO_BUILD_JOBS=8 cargo test -p CRATE TEST_NAME
cd apps/desktop && pnpm bundle   # packages for this OS, with skiffd inside
```

During a task, run only the tests for the code you changed. See
[AGENTS.md](AGENTS.md#testing). The [Glossary](https://github.com/solutionscay/skiff/wiki/Glossary)
names the parts of the screen and the session tree.

A rebuild does not replace the running daemon. Run `skiff daemon reload` after
`cargo build -p skiffd`. When the daemon changes, raise the skiffd version in
`crates/skiffd/Cargo.toml` and `crates/skiff-core/Cargo.toml`. When the reload handover
metadata changes, raise `HANDOVER` in `crates/skiffd/src/session/reload.rs`. Set
`SKIFF_SOCKET` to another path for a test daemon with its own state. See
[skiffd › Development](https://github.com/solutionscay/skiff/wiki/skiffd#development).

A `v*` tag builds every package in CI and attaches them to a draft release.

## License

MIT

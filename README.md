<p align="center"><img src=".github/skiff.png" width="160" alt="Skiff mascot: a small boat with a robot at the helm"></p>

<h1 align="center">Skiff</h1>

<p align="center">
  <a href="https://github.com/solutionscay/skiff/releases/latest"><img src="https://img.shields.io/github/v/release/solutionscay/skiff?include_prereleases&label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/solutionscay/skiff" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS-lightgrey" alt="Platforms: Linux and macOS">
</p>

A minimalist desktop workspace for coding agents on Linux and macOS.

Skiff keeps sessions organized across projects and Git worktrees. Editing and
previews stay in the apps you already use.

<p align="center"><img src=".github/screenshot.webp" alt="Skiff on Linux: a project with its session group, changed files and file tree in the sidebar, and three agent sessions side by side, with an unread flag on the session that finished"></p>

## Philosophy

Skiff has a deliberately small scope: reduce the mental effort of managing
multiple sessions, and use existing apps for the rest.

Project and terminal themes give you a visual cue about where you are. Named
groups and terminal layouts keep related sessions together, so you can arrange
them to suit the work.

## How it works

You add a project (a Git repository), make worktrees in it, and start agent or shell sessions in each worktree. Sessions can sit side by side in a group. Each group has a name and a terminal theme, and so does each project.

Skiff does not edit or preview files. Each worktree lists its changed files, and a double-click shows the diff. Files open in the apps you choose under Settings › Open with (see the [wiki](https://github.com/solutionscay/skiff/wiki/Open-with)). There are no plugins, no hooks into agent configs, and no automations. Settings live in one file, `projects.toml`.

Sessions run in `skiffd`, a daemon that owns every PTY. You can close the window and the agents keep running. When the window opens again, each terminal shows its last screen.

Each session row shows what needs you. A bell means an agent asks for approval. A flag means an agent finished its turn, or a shell command ended, while you were in another pane. `skiffd` reads this from the screen and from the program in front of the terminal, not from hooks. When you come back to a pane, a line marks where you stopped reading.

## Install

Download the latest [release](https://github.com/solutionscay/skiff/releases/latest):

- Linux (x86_64): `.deb`, `.rpm` or AppImage
- macOS (Apple Silicon): `.dmg`, signed and notarized

The `skiff` command drives sessions and groups from a shell or an agent. It ships with the app and updates with it. The `.deb` and `.rpm` install it to `/usr/bin`. The AppImage copies it to `~/.local/bin` when it starts. The macOS app links it into `/usr/local/bin`, or `/opt/homebrew/bin` when it can write there. Otherwise run Help › Install skiff command… once. See the [wiki CLI page](https://github.com/solutionscay/skiff/wiki/CLI).

## Themes

Themes apply to the app, a project, or a session. A session theme replaces the
project theme. A project theme replaces the app theme.

The 34 built-in themes use the operator's Foot palette. Their
Skiff TOML files live in `crates/skiff-core/themes`. Ultraviolet is the default.
Each file declares `color_family` and `main_color`. The picker uses these
values for its color filter and swatch. Themes without a declared color group
show as Unclassified.

Use the theme picker to search by name or filter by main color.
Select **Import themes…** to add multiple files. Skiff supports its own TOML,
Superset JSON, Foot INI, Alacritty TOML, Ghostty, and base16 or base24 YAML.
Each file needs a foreground, a background, and all 16 ANSI colors. Import
keeps existing files with the same name and reports files it cannot read.

**Open theme folder** opens the `themes` folder beside `projects.toml`.
You can copy files there. The list refreshes when you return to Skiff. There is no theme count
limit. Theme files contain color values, so a large collection needs little space.

Light terminal themes use contrast correction for text. The waterline uses
each pane's background. Programs can supply their own text and background
colors, so a program's dark blocks can still appear inside a light terminal.

## Layout

```
crates/skiff-core    session model, wire protocol, projects.toml config
crates/skiffd        daemon: owns every PTY, serves clients over a Unix socket
crates/skiff-client  async client for the socket
crates/skiff-cli     the `skiff` command: sessions and groups from the shell
apps/desktop         Tauri v2 app, vanilla TypeScript + xterm.js
  src/app            state, actions, keyboard, startup connections
  src/ui             DOM builders, dialogs, menus
  src/workspace      rail, session tree, layouts, files, project actions
  src/terminal       xterm runtime, panes, search, clipboard, font size
  src/appearance     themes, colors, icons, settings
  src/platform       wire types, IPC bytes, file actions
  src/diagnostics    latency traces and memory counts
  src/styles         styles by owner, imported in cascade order
```

## Development

See the [wiki Glossary](https://github.com/solutionscay/skiff/wiki/Glossary) for the terms
this codebase uses for its own screen areas, session-tree contents, and keyboard navigation.

```
cd apps/desktop && pnpm install && pnpm tauri dev
cargo test --workspace
```

`pnpm tauri dev` builds `skiffd` first, then the app, and puts both in `target/debug`.

The app starts `skiffd` on its own when the socket is not there. It looks for the binary
next to its own executable, then on `PATH`. `SKIFF_DAEMON` overrides. `SKIFF_SOCKET`
overrides the socket path, `SKIFF_CONFIG` the config file.

### Changes to skiffd

The daemon outlives the app. A rebuild does not replace the daemon that is running,
so a change to `crates/skiffd` has no effect until you restart it:

```
cargo build -p skiffd      # pnpm tauri dev also does this
pkill -x skiffd            # ends every open session
```

Then reload the window (Ctrl+R) or restart the app. The app starts the new binary
on its next call.

skiffd has its own version (`crates/skiffd` and `crates/skiff-core`), apart from the
app version. At launch the app compares the running daemon's version and protocol
(`skiff_core::PROTOCOL`) with the skiffd it ships. It replaces an older or incompatible daemon without asking when no
session is live. When sessions are live, it shows a "Restart skiffd" button instead.

Debug builds use `skiffd-dev.sock` and `workspace-dev.json`, so `pnpm tauri dev` runs
its own daemon beside the installed app. The daemon writes its log next to the socket
(`skiffd.log` or `skiffd-dev.log`), which is `$XDG_RUNTIME_DIR/skiff/` on Linux.

## Packages

```
cd apps/desktop && pnpm bundle   # packages for the host OS, with skiffd inside
```

On Linux this makes `.deb`, `.rpm` and AppImage. On macOS it makes `Skiff.app` and a `.dmg`.
Packages land in `target/release/bundle/`.

A `v*` tag builds every package in CI and attaches them to a draft release. CI signs and
notarizes the `.dmg` with the `APPLE_*` repository secrets.

## License

MIT

## Social preview

The GitHub social preview image is [`.github/social-preview.png`](.github/social-preview.png).

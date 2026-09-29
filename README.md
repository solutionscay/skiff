<p align="center"><img src=".github/skiff.png" width="160" alt="Skiff mascot: a small boat with a robot at the helm"></p>

<h1 align="center">Skiff</h1>

A desktop app for running many coding agents side by side, on Linux and macOS.

<p align="center"><img src=".github/screenshot.webp" alt="Skiff on Linux: a project with two worktrees, and three agent sessions in a one-left, two-right layout over a project background"></p>

You add a project (a Git repository), make worktrees in it, and start agent or shell sessions in each worktree. Sessions can sit side by side in a group. Each group has a name and a terminal theme, and so does each project.

The first version stops there. It has no plugins, no hooks into agent configs, no Git client beyond worktrees, no editor or file previews, and no automations. A project can show a Files tree under each worktree (`files = true`), but it only opens files in their default app. Settings live in one file, `projects.toml`.

Sessions run in `skiffd`, a daemon that owns every PTY. You can close the window and the agents keep running. When the window opens again, each terminal shows its last screen.

## Platforms

Skiff runs on Linux and macOS. The app and the daemon talk over a Unix socket.

## Layout

```
crates/skiff-core    session model, wire protocol, projects.toml config
crates/skiffd        daemon: owns every PTY, serves clients over a Unix socket
crates/skiff-client  async client for the socket
apps/desktop         Tauri v2 app, vanilla TypeScript + xterm.js
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

If a change bumps the wire protocol (`skiff_core::PROTOCOL`), the app replaces an old
daemon without asking when no session is live. When sessions are live, it shows a
"Restart skiffd" button instead.

The daemon writes its log to `skiffd.log` next to the socket, which is
`$XDG_RUNTIME_DIR/skiff/` on Linux.

## Packages

```
cd apps/desktop && pnpm bundle   # packages for the host OS, with skiffd inside
```

On Linux this makes `.deb`, `.rpm` and AppImage. On macOS it makes `Skiff.app` and a `.dmg`.
Packages land in `target/release/bundle/`.

The macOS build is not signed. Another Mac blocks it at first open: right-click the app
and choose Open, or run `xattr -dr com.apple.quarantine /Applications/Skiff.app`.

A `v*` tag builds the Linux packages in CI and attaches them to a draft release.

## License

MIT

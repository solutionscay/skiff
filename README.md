# Skiff

A desktop app for running many coding agents side by side, on Linux.

You add a project (a Git repository), make worktrees in it, and start agent or shell sessions in each worktree. Sessions can sit side by side in a group. Each group has a name and a terminal theme, and so does each project.

The first version stops there. It has no plugins, no hooks into agent configs, no Git client beyond worktrees, no file browser, and no automations. Settings live in one file, `projects.toml`.

Sessions run in `skiffd`, a daemon that owns every PTY. You can close the window and the agents keep running. When the window opens again, each terminal shows its last screen.

## Platforms

Linux is the target for now. The app and the daemon talk over a Unix socket, which also works on macOS, so a macOS build may come later. Windows is not planned: it would need a different transport.

## Layout

```
crates/skiff-core    session model, wire protocol, projects.toml config
crates/skiffd        daemon: owns every PTY, serves clients over a Unix socket
crates/skiff-client  async client for the socket
apps/desktop         Tauri v2 app, vanilla TypeScript + xterm.js
```

## Development

```
cargo build --workspace          # builds skiffd and the app backend
cargo test --workspace
cd apps/desktop && pnpm install && pnpm tauri dev
```

The app starts `skiffd` on its own when the socket is not there. It looks for the binary
next to its own executable, then on `PATH`. `SKIFF_DAEMON` overrides. `SKIFF_SOCKET`
overrides the socket path, `SKIFF_CONFIG` the config file.

## Packages

```
cd apps/desktop && pnpm bundle   # .deb, .rpm and AppImage, with skiffd inside
```

A `v*` tag builds the same packages in CI and attaches them to a draft release.

## License

MIT

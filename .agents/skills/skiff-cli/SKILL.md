---
name: skiff-cli
description: Drive Skiff from the shell with the `skiff` CLI. Use when the user asks to start, list, name, group, split, message, watch or stop Skiff sessions or agents, for example "open a codex next to me", "start two claude sessions in a group", "send this to the other pane", "wait for it to finish", "what is the other agent doing", or anything else about Skiff panes and groups.
---

# Skiff CLI

`skiff` talks to `skiffd`, the daemon that owns every Skiff terminal. The app and the CLI see the same sessions and groups. A change from one shows in the other at once.

Full reference: https://github.com/solutionscay/skiff/wiki/CLI

## Boot sequence

Do these steps before the first command in a task.

1. Run `skiff status`. If the command is not found, stop. Tell the user to install it: `cargo install --path crates/skiff-cli` from a Skiff checkout. If it says skiffd is not running, stop and tell the user to open Skiff.
2. Check `$SKIFF_SESSION`. If it is set, you run inside a Skiff pane, and `.` means your own session. `--group .` means the group your pane is in.
3. Run `skiff ls --json` and `skiff group ls --json` to see what exists. Read JSON, not the tables.

## Names

- A session is a full id, a unique id prefix, or the name the sidebar shows. A group is an id, a prefix or a name.
- An argument that matches more than one session is an error that lists them. Pick the id from the list and run again.
- Use ids in scripts. Names can change.

## Commands

| Task | Command |
|---|---|
| List sessions | `skiff ls [--json]` |
| Start a shell | `skiff new [--name N] [--cwd DIR]` |
| Start an agent | `skiff new --agent claude [--name N]` (`skiff agents` lists ids) |
| Start a command | `skiff new -- npm run dev` |
| Start next to me | `skiff new --agent codex --group . --split right` |
| Type into a session | `skiff send S "text" --enter` |
| Send a file or long prompt | `skiff send S - --enter < prompt.md` |
| Read the screen | `skiff peek S [-n 40]` |
| Wait for it to stop | `skiff wait S --for idle [--timeout 600]` |
| Wait for an approval prompt | `skiff wait S --for waiting` |
| Rename | `skiff rename S "name"` |
| Mark seen | `skiff seen S` |
| Stop | `skiff kill S` |
| Groups | `skiff g ls`, `g new [--name N] S1 S2 [--dir row\|col]`, `g add G S [--split right\|down\|left\|up] [--of S]`, `g rm G S`, `g rename G N`, `g delete G` |

`skiff new` prints the new session id on stdout. `--json` prints the full session.

`wait` exits 0 when every session reaches the state, 1 on timeout, and 2 when a session closes or exits first. States: `working`, `waiting`, `idle`, `done`.

## Recipes

Hand a task to a new agent and collect the result:

```sh
id=$(skiff new --agent claude --name reviewer --group . --split down)
skiff wait "$id" --for idle --timeout 60     # let it boot
skiff send "$id" "Review the diff on this branch. List bugs only." --enter
skiff wait "$id" --for idle --timeout 1800
skiff peek "$id" -n 80
```

Put two agents side by side in a new group:

```sh
a=$(skiff new --agent claude) ; b=$(skiff new --agent codex)
skiff g new --name pair "$a" "$b" --dir row
```

## Rules

- Never run `skiff attach`. It takes over the terminal and needs a person at the keys.
- Do not `kill` a session, or remove a session from its group, unless the user asked for that session by name or you started it in this task.
- Do not `send` into a session you did not start unless the user asked you to.
- A group holds 4 panes at most. A session is in one group at most. Adding it to a group takes it out of its old one.
- `idle` means no output for a while. It does not prove the agent is done. Read the screen with `peek` before you report a result.
- An agent that exits leaves a shell in its pane. `done` means the shell exited too.

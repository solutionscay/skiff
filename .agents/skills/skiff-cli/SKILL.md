---
name: skiff-cli
description: Drive Skiff from the shell with the `skiff` CLI. Use when the user asks to start, list, name, group, split, message, watch or stop Skiff sessions or agents, for example "open a codex next to me", "start two claude sessions in a group", "send this to the other pane", "wait for it to finish", "what is the other agent doing", or anything else about Skiff panes and groups.
---

# Skiff CLI

`skiff` talks to `skiffd`, the daemon that owns every Skiff terminal. The app and the CLI see the same sessions and groups. A change from one shows in the other at once.

Full reference: https://github.com/solutionscay/skiff/wiki/CLI

## Boot sequence

Do these steps before the first command in a task.

1. Run `skiff status`. If the command is not found, stop. Tell the user to run Help › Install skiff command… in Skiff. The app ships the CLI and keeps it up to date. If it says skiffd is not running, stop and tell the user to open Skiff.
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
| Type into an idle session | `skiff send S "text" --enter` |
| Type a command into an idle shell | `skiff send S "make test" --enter --shell` |
| Send a file or long prompt | `skiff send S - --enter < prompt.md` |
| Read the screen | `skiff peek S [-n 40]` |
| Wait for it to stop | `skiff wait S --for idle [--timeout 600]` |
| Wait for an approval prompt | `skiff wait S --for waiting` |
| Rename | `skiff rename S "name"` |
| Mark seen | `skiff seen S` |
| Stop | `skiff kill S` |
| Themes | `skiff theme ls`, `theme session set S ID`, `theme project set NAME ID`, `theme app set ID`, and `clear` in place of `set ID` |
| Open with | `skiff open-with ls`, `open-with set KEY "CMD"`, `open-with clear KEY`. Keys: `diff`, `text`, `markdown`, `html` |
| Groups | `skiff g ls`, `g new [--name N] S1 S2 [--dir row\|col]`, `g add G S [--split right\|down\|left\|up] [--of S]`, `g rm G S`, `g rename G N`, `g delete G` |

`skiff new` prints the new session id on stdout. `--json` prints the full session.

States: `working`, `waiting`, `idle`, `done`. `waiting` means an approval prompt or a bell. Only a person answers it.

`send` checks the session before it types any bytes. It refuses in this order:

| Exit | Condition | What to do |
|---|---|---|
| 7 | The session is your own (`$SKIFF_SESSION`), by any name, id, prefix or `.` | Send to another session. |
| 3 | The session is `waiting` | Tell the user which pane needs them. End the turn. |
| 4 | The session is `working` | Do not wait for it and do not retry. See the rules. |
| 6 | The session is `done` | Its program exited. Start a new session. |
| 5 | A shell, or a program Skiff cannot identify, is in front | Add `--shell` only if you mean to type a shell command. |
| 0 | Sent | |
| 1 | Error, for example no such session | |

`--shell` relaxes only the exit 5 check. No flag overrides a waiting, working, done or own-session refusal. `--raw` and `--shell` do not either.

After the bytes go in, one more check applies:

| Exit | Condition | What to do |
|---|---|---|
| 8 | `--enter` went to a known agent (Claude Code, Codex, Gemini, Grok, opencode), but the session stayed `idle` for 5 seconds after Enter | Run `skiff peek S`. If the text is in the input box, tell the user which pane. Do not send it again. |

`send` never presses Enter a second time. It skips this check without `--enter`, with `--raw`, and when the program in front is not a known agent. A turn that starts and ends in under 5 seconds with no output can also give exit 8.

`wait` exit codes:

| Exit | Meaning |
|---|---|
| 0 | Every session reached the state. |
| 1 | Timeout. |
| 2 | A session closed or exited before it reached the state. |
| 3 | A session is `waiting`. Check its pane. |

With `--for idle`, `done` or `any-not-working`, `wait` stops at once with exit 3 when any session is `waiting`, at the start or later. `--for waiting` does not change.

## Recipes

Hand a task to a new agent and collect the result:

```sh
id=$(skiff new --agent claude --name reviewer --group . --split down)
skiff wait "$id" --for idle --timeout 60     # let it boot
skiff send "$id" "Review the diff on this branch. List bugs only." --enter
skiff wait "$id" --for idle --timeout 540    # shorter than the shell tool's limit
skiff peek "$id" -n 80
```

If a `wait` times out and the child still works, run `wait` again. If it exits 3, stop. See the rules.

Put two agents side by side in a new group:

```sh
a=$(skiff new --agent claude) ; b=$(skiff new --agent codex)
skiff g new --name pair "$a" "$b" --dir row
```

## Rules

- Never run `skiff attach`. It takes over the terminal and needs a person at the keys.
- Do not `kill` a session, or remove a session from its group, unless the user asked for that session by name or you started it in this task.
- Do not `send` into a session you did not start unless the user asked you to.
- On exit 8 from `send`, run `skiff peek S`. If the text is in the input box, tell the user which pane holds it, with its name and id. Do not send the text again.
- Only a person answers an approval. On exit 3 from `send` or `wait`, tell the user which pane needs them, with its name and id. End the turn. Do not answer the prompt. Do not retry until the user says to continue.
- Give each `wait` a `--timeout` shorter than your shell tool's time limit.
- A child reports to its parent when it ends its turn with the result on screen or in a file. The parent waits, then reads the result with `peek`.
- Never wait for the session that started you. A send to a busy parent is refused with exit 4. Then leave the result on screen or in a file and end your turn. Do not retry. A send to an idle parent is permitted.
- If a child's report names another pane that needs a person, pass that report to the user and stop.
- Change a project or app theme, or an Open with command, only when the user asks. Those edit `projects.toml` and apply to every pane.
- A group holds 4 panes at most. A session is in one group at most. Adding it to a group takes it out of its old one.
- `idle` means no output for a while. It does not prove the agent is done. After each successful `wait`, read the screen with `peek` before you report a result.
- An agent that exits leaves a shell in its pane. `done` means the shell exited too.

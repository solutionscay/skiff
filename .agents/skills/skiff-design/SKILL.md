---
name: skiff-design
description: Skiff's design rules for UI, interaction and copy. Use before you add or change anything under apps/desktop/src that the user sees or presses (CSS, menus, dialogs, keys, labels, themes), and when you review such a change.
---

# Skiff design

The published copy is the wiki [Design](https://github.com/solutionscay/skiff/wiki/Design) page. Change both together.

Where the rules live in code: tokens in `apps/desktop/src/styles/tokens.css`, keys in `app/keys.ts` (`keyLabel`, `DESCRIBE`), actions in `app/actions.ts`, menus in `ui/menu.ts` and `workspace/menus.ts`, theme pickers in `appearance/themes.ts`.

How Skiff looks and behaves. Read this before you change the UI. Terms follow the [Glossary](https://github.com/solutionscay/skiff/wiki/Glossary).

## The essence

Skiff is a control surface for many long-running agent sessions. It shows where each session is and what it needs. The apps the user already has do everything else.

Test every change with three questions:

1. Does it make many sessions easier to manage?
2. Does the user always know where the keys are and what an action changes?
3. Does Skiff stay small?

## Product rules

- **Panes are for sessions.** Files, diffs and viewers open in another app's window or a terminal window (Settings › Open with). A new "open X" feature launches a window. It never adds a pane.
- **Sessions outlive the window.** `skiffd` owns every PTY. Closing a pane, a group or the window does not end a session. Only End or Kill does, and both ask first.
- **Hands off the agent setup.** Skiff installs no hooks or plugins, injects no flags, and writes nothing into an agent's config or data folder. An integration, if one exists, is a separate package that the user installs.
- **Restore as shells.** After a restart, each pane comes back as a shell in its folder. Skiff does not guess agent state.
- **One settings file.** `projects.toml` holds the settings. Customization is declarative: TOML and themes. Skiff has no plugins and no automations.

## Visual rules

- **Square and flush.** `border-radius: 0` everywhere. Only dots are round. Panes touch, with no gaps and no floating cards.
- **One rule weight.** Lines are 1px `--rule`. Edges of floating surfaces use `--rule-strong`. Do not stack accent lines.
- **One row grid.** Rows, menu items, headers and bar buttons are `--row` high (40px at zoom 1). Side padding is 16px. Lines align across the rail, the sidebar and the terminal area.
- **Everything zooms.** Write lengths as `calc(N * var(--u))` and font sizes as `calc(Npx * var(--zoom, 1))`. Do not use raw px for layout.
- **Tokens only.** Colors come from `styles/tokens.css`. Components contain no hex values. A theme changes the tokens.
- **Two typefaces.** IBM Plex Sans for the UI: 13px body, 11 to 12px for meta text and hints, weights 500 to 700 for emphasis. JetBrains Mono for terminals and code.
- **Each color has one job.**
  - Project color (`--pc`) shows location: the rail chip, the 3px project bar, the current row and the lit menu item.
  - State colors show session state: amber for waiting, blue for working, grey for done and idle. Project colors stay away from state colors.
  - Red shows errors and destructive items only. A destructive item stays red when lit.
- **Highlight, do not outline.** The current row and the lit menu item get a 22% project-color tint and a 2px inset bar on the left. A focus ring shows only after keyboard use (`body.kbd`), as a quiet 1px line.
- **Shadows on floating surfaces only.** Menus and dialogs have shadows. Menus are solid, never translucent.
- **Little motion.** Layout transitions are 120ms or less. Only the working skiff and the mascot loop, and they use `steps()`. Bells and flags stay still. Smooth loops cost frames on WebKit and in power-saver mode.
- **Icons.** The Rune Icons outline set, stroke `currentColor`, 14px in menus. Add only the icons that the code uses.

## Theme scope

Themes cascade: app, then project, then group or session. A lower level replaces the one above it.

- Each picker names its scope in the title: "Project theme: skiff", "Session theme: Sea Dog Mermaid".
- A line under the title says what the theme covers.
- The first card is the level above: "Same as app", "Same as project".

Themes are location cues. One look tells you which project or session you are in.

## Interaction rules

- **Keyboard and mouse are equal.** An action has one implementation. Its key, its menu item and its palette entry all call it.
- **Ctrl+Shift for the app.** App keys are Ctrl+Shift+letter (⌘+Shift on macOS). Bare Ctrl belongs to the terminal. Users override keys in `[keys]`, and F1 shows the live keymap. Never hardcode a key label: use `keyLabel(action)`.
- **Keys act on what is highlighted.** On the rail, a key acts on the project. On a session row, it acts on that session. On a group row, it acts on the group. On a project, worktree, Changes or Files row, it acts on the project. With no row, it acts on the focused pane.
- **Menus show their keys, and the keys work.** An item's hint key picks that item while the menu is open. Enter always picks the lit item.
- **One highlight.** The list has one current row at most. The focused pane's frame hides when the keys are elsewhere, such as on the rail or on a group row.
- **Focus goes back.** A menu or dialog gives the keys back to the rail, row or terminal that opened it. It does not change the selection.
- **Three regions.** The rail, the list and the terminal have one tab stop each. F6 steps through them.
- **Ask before you destroy.** Destructive actions ask, and they say what keeps running. Other actions do not ask.

## Words

- Use the Glossary terms. Do not invent synonyms.
- Write in Simplified Technical English: short sentences, active voice, one idea per sentence.
- Use sentence case. An item that opens a dialog or more choices ends in "…".
- Name the scope: "Session theme", "Project theme", "Close the pane. The session keeps running."
- Keep the tone plain and dry. Leave out hype.

## Before you ship a UI change

- Lines align with the row grid and the 1px rules.
- Colors and sizes come from tokens and zoom units.
- The action has a key, a menu item and a palette entry where it fits, and all three call the same code.
- The key acts on the highlighted target, and the title or label names that target.
- Focus and selection are the same after the menu or dialog closes.
- Nothing new opens in a pane, and nothing writes into an agent's setup.

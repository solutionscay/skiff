# Motion inventory

Every animation in `apps/desktop/src`. Add a line when you add an animation. `tests/motionRules.test.ts` guards the rules.

The rules:

1. Never animate terminals: no opacity, transform or size animation on a pane or its container.
2. Animate only light layers, with opacity and transform only.
3. At most 120ms, from the tokens in `styles/tokens.css`. `ui/motion.ts` reads them for animations, and holds the reduced-motion check.
4. No terminal work during a transition. Panes paint and fit before the board goes; WebGL attaches before a pane shows.
5. When in doubt, cut the animation. Reduced motion stops all of it.

## One-off

| Animation | Duration | Where |
|---|---|---|
| Mission control content fades in on open | `--motion-fade` | `workspace/missionMotion.ts` |
| Ledger row moves to its new place | `--motion-move` | `workspace/ledgerMotion.ts` |
| Ledger row comes or goes | `--motion-fade` | `workspace/ledgerMotion.ts` |
| Ledger row flash, when the count is pressed | `--motion-move` | `styles/ledger.css` |

## Loops

Each loop uses `steps()`. Reduced motion stops each loop.

| Animation | Timing | Where |
|---|---|---|
| Working skiff | water 0.6s, rock 1.2s | `styles/tree.css`, `styles/mission.css` |
| Idle watcher icon | 8s | `styles/tree-project.css` |
| Skipper face | blink 5s, head turn 42s | `styles/skippers.css` |
| Diagnostics progress sweep | 1.2s | `styles/diagnostics-alerts.css` |

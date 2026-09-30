/** Pane actions are connected at startup, before the first render. */
import type * as View from "../workspace/view";
let actions: Pick<typeof View, "focusPane" | "closePane" | "dragSessions">;
export const configurePaneActions = (next: typeof actions) => { actions = next; };
export const focusPane: typeof View.focusPane = (...args) => actions.focusPane(...args);
export const closePane: typeof View.closePane = (...args) => actions.closePane(...args);
export const dragSessions: typeof View.dragSessions = (...args) => actions.dragSessions(...args);

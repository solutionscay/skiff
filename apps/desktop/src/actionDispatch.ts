import type { Action } from "./keys";
let dispatch: (action: Action) => void;
export const configureActionDispatch = (next: typeof dispatch) => { dispatch = next; };
export const runAction = (action: Action) => dispatch(action);

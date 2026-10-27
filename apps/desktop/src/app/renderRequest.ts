/** Render callbacks are connected before application events can run. */
let draw: (cause?: string) => void;
export const configureRender = (next: (cause?: string) => void) => { draw = next; };
/** `cause` names what asked, for the latency trace. */
export const render = (cause?: string) => draw(cause);

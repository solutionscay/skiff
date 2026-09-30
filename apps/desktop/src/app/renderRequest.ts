/** Render callbacks are connected before application events can run. */
let draw: () => void;
export const configureRender = (next: () => void) => { draw = next; };
export const render = () => draw();

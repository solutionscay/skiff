import type { createSwitcher } from "../ui/switcher";
export let switcher: ReturnType<typeof createSwitcher>;
export const configureCommandUi = (next: typeof switcher) => { switcher = next; };

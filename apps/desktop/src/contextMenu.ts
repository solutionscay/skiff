import { createMenu } from "./menu";
import { refocusTerminal } from "./view";

export const ctxMenu = createMenu(() => refocusTerminal());

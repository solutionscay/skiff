import { hostStore } from "../platform/invoke";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import type { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
export interface Pane {
  hostId?: string;
  el: HTMLDivElement;
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  /** The WebGL renderer, while this pane keeps one. */
  webgl?: WebglAddon;
  /** The theme id this pane last drew with. */
  theme?: string;
  /** Out of the layout: no output stream, so it costs nothing while hidden. */
  parked: boolean;
  /** Counts subscriptions. A chunk from an older one is dropped. */
  stream: number;
  /** The subscription whose output the terminal last drew. Until it equals `stream`, the rows are from before the snapshot. */
  wrote: number;
  /** Bytes xterm parsed that Rust has not heard about yet. */
  unacked: number;
  /** An ack_output call is in flight. */
  acking: boolean;
  /** Subscribe and unsubscribe calls, one after another, so they land in order. */
  sub: Promise<void>;
}

export let panes: Map<string, Pane>;

/** Opens in flight, so focus calls share one terminal and subscription. */
export let opening: Map<string, Promise<Pane>>;

hostStore(() => ({ panes: new Map<string, Pane>(), opening: new Map<string, Promise<Pane>>() }),
  (store) => { ({ panes, opening } = store); });

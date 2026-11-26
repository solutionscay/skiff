/** A route names one Settings entry before Hello and its durable Host after Hello. */
export interface HostRoute {
  entry: number;
  endpoint: string | null;
  host: string | null;
  generation: number;
}

export function sameRoute(a: HostRoute | null, b: HostRoute | null): boolean {
  return !!a && !!b && a.entry === b.entry && a.endpoint === b.endpoint && a.host === b.host && a.generation === b.generation;
}

/** A Host switch reloads the document. Only a reconnect can advance this page's route. */
export function advances(current: HostRoute, next: HostRoute): boolean {
  return current.entry === next.entry && current.endpoint === next.endpoint && next.generation > current.generation;
}

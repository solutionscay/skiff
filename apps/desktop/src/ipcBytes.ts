/** Byte payloads from a Tauri channel. */
export function toBytes(m: unknown): Uint8Array {
  if (m instanceof ArrayBuffer) return new Uint8Array(m);
  if (m instanceof Uint8Array) return m;
  if (Array.isArray(m)) return Uint8Array.from(m as number[]);
  return new TextEncoder().encode(String(m));
}

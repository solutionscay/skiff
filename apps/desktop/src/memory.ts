/** The status bar's memory reading: the app with WebKit, plus skiffd. */
import { invoke } from "@tauri-apps/api/core";
import { $ } from "./dom";
import { rune } from "./runes";

const mb = (b: number) => (b >= 1 << 30 ? `${(b / (1 << 30)).toFixed(1)} GB` : `${Math.round(b / (1 << 20))} MB`);

async function update() {
  const el = $("memory");
  let m: [number, number] | null;
  try {
    m = await invoke<[number, number] | null>("memory_usage");
  } catch (e) {
    el.textContent = "mem ?";
    el.title = String(e);
    return;
  }
  if (!m) {
    el.textContent = "";
    return;
  }
  const [app, daemon] = m;
  el.replaceChildren(rune("devices-memory", 13), mb(app + daemon));
  el.title = `App ${mb(app)} · skiffd ${mb(daemon)}\nSessions not counted`;
}

/** Reads now, then every 5 seconds. A read is one pass over /proc. */
export function startMemory() {
  void update();
  window.setInterval(update, 5000);
}

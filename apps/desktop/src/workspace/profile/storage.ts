/** The workspace file on disk (read, write, revision) and the recent list in localStorage. */
import { invoke } from "@tauri-apps/api/core";
import type { Profile } from "./state";
import { basename } from "../paths";
import { shownHost } from "../../app/hosts";

const LAST_KEY = "skiff.workspace.last";
const RECENT_KEY = "skiff.workspace.recent";
const RECENT_MAX = 8;

export interface Loaded {
  path: string;
  profile: Profile;
  revision: string;
}

function storedList(key: string): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

function store(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* it lasts for this run */
  }
}

/** Workspace files are local, but their project paths belong to one Host. */
const hostKey = (key: string) => {
  const host = shownHost();
  return host ? `${key}.${host}` : key;
};

export const recents = () => storedList(hostKey(RECENT_KEY));

export function remember(path: string) {
  store(hostKey(RECENT_KEY), JSON.stringify([path, ...recents().filter((x) => x !== path)].slice(0, RECENT_MAX)));
  store(hostKey(LAST_KEY), path);
}

export function forget(path: string, render: () => void) {
  store(hostKey(RECENT_KEY), JSON.stringify(recents().filter((x) => x !== path)));
  render();
}

export const fileName = basename;

export async function read(path: string): Promise<Loaded> {
  return invoke<Loaded>("workspace_read", { path });
}

export async function revision(path: string) {
  return invoke<string | null>("workspace_revision", { path });
}

export async function write(path: string, profile: Profile, expect: string | null) {
  return invoke<string>("workspace_write", { path, profile, expect });
}

export const last = () => localStorage.getItem(hostKey(LAST_KEY));
export const forgetLast = () => store(hostKey(LAST_KEY), null);

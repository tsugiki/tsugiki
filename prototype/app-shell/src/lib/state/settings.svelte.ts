// PROTOTYPE — throwaway. Settings state: the values, keybindings and Repository settings, in memory.
// A Repository's pins and Muted authors stay in its user state (app.svelte.ts), so pinning or muting in
// the app shows up in config.toml and the other way round. Nothing is written to disk.
import {
  DEFAULTS, DEFAULT_KEYS, configToml, localToml, parseToml, applyToml,
  type Config, type RepoConfig, type Value,
} from "$lib/core/settings";
import { snapshots } from "$lib/data/source";
import { REPOS, userOf, say } from "./app.svelte";

type Own = Omit<RepoConfig, "pinned" | "mutedAuthors">;
const cfg = $state<{ values: Record<string, Value>; keys: Record<string, string>; repos: Own[] }>({
  values: { ...DEFAULTS },
  keys: { ...DEFAULT_KEYS },
  repos: snapshots.map((s) => ({
    name: s.repo.name, conventions: "auto", firstSync: "auto",
    filters: s.filters.map((f) => ({ name: f.name, q: f.q })),
    clone: s.repo.name === "python/cpython" ? "X:\\Work\\cpython\\main" : "",
  })),
});
/** The token typed in for "A personal access token": the OS keychain, in the real app. */
export const secret = $state({ token: "" });

/** Settings UI: open or not, which variant (?variant=), and the section to land on. */
export const settingsUi = $state<{ open: boolean; variant: string; section: string | null }>({ open: false, variant: "A", section: null });
export function openSettings(section: string | null = null) { settingsUi.open = true; settingsUi.section = section; }

/** The whole configuration as the files would hold it. */
export function config(): Config {
  return {
    values: cfg.values, keys: cfg.keys,
    repos: cfg.repos.map((r) => ({ ...r, pinned: [...userOf(r.name).pins].sort((a, b) => a - b), mutedAuthors: [...userOf(r.name).mutedAuthors].sort() })),
  };
}
export const value = (key: string) => cfg.values[key];
export const isDefault = (key: string) => cfg.values[key] === DEFAULTS[key];
export function setValue(key: string, v: Value) { cfg.values[key] = v; }
export function resetValue(key: string) { cfg.values[key] = DEFAULTS[key]; }
export const keyOf = (id: string) => cfg.keys[id];
export function setKey(id: string, k: string) { cfg.keys[id] = k; }
export function resetKey(id: string) { cfg.keys[id] = DEFAULT_KEYS[id]; }

export const repoConfig = (name: string) => config().repos.find((r) => r.name === name)!;
export function setRepo(name: string, patch: Partial<Own>) { Object.assign(cfg.repos.find((r) => r.name === name)!, patch); }
export function unpin(name: string, n: number) { userOf(name).pins.delete(n); }
export function unmuteAuthorIn(name: string, login: string) { userOf(name).mutedAuthors.delete(login); }
export function removeRepo(name: string) { say(`Would remove ${name} from config.toml and delete its mirror (prototype: kept)`); }

export const configText = () => configToml(config());
export const localText = () => localToml(config(), !!secret.token);

/** Variant C: the user edited a file. Valid parts apply; the errors come back with line numbers. */
export function applyFile(which: "config" | "local", text: string): string[] {
  const { data, errors } = parseToml(text);
  const parsed = errors.map((e) => `line ${e.line}: ${e.message}`);
  const { config: next, errors: invalid } = applyToml(config(), data, which === "config" ? "synced" : "machine");
  cfg.values = next.values;
  cfg.keys = next.keys;
  cfg.repos = cfg.repos.map((r) => {
    const n = next.repos.find((x) => x.name === r.name);
    if (!n) return r;
    if (which === "config") {
      const u = userOf(r.name);
      u.pins.clear(); n.pinned.forEach((p) => u.pins.add(p));
      u.mutedAuthors.clear(); n.mutedAuthors.forEach((a) => u.mutedAuthors.add(a));
    }
    return { name: r.name, conventions: n.conventions, firstSync: n.firstSync, filters: n.filters, clone: n.clone };
  });
  const added = next.repos.filter((r) => !REPOS.includes(r.name)).map((r) => `${r.name}: would be added and synced (prototype: only the snapshot's Repositories exist)`);
  return [...parsed, ...invalid, ...added];
}

/** B's Apply / Cancel: a copy to edit, then commit or drop. */
export function snapshotSettings() { return JSON.parse(JSON.stringify({ cfg, secret, pins: REPOS.map((r) => [...userOf(r).pins]), muted: REPOS.map((r) => [...userOf(r).mutedAuthors]) })); }
export function restoreSettings(s: ReturnType<typeof snapshotSettings>) {
  cfg.values = s.cfg.values; cfg.keys = s.cfg.keys; cfg.repos = s.cfg.repos; secret.token = s.secret.token;
  REPOS.forEach((r, i) => {
    const u = userOf(r);
    u.pins.clear(); s.pins[i].forEach((n: number) => u.pins.add(n));
    u.mutedAuthors.clear(); s.muted[i].forEach((a: string) => u.mutedAuthors.add(a));
  });
}

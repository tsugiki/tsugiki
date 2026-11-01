// PROTOTYPE — throwaway. Core: what gh-rev's settings are, where each one lives, and the two files they
// make. `config.toml` travels with the user (they sync it themselves: dotfiles, Dropbox); `local.toml`
// belongs to this machine (paths, the token source). A token typed in is kept in the OS keychain.
//
// The TOML here is the small subset the files use: [tables], [[array tables]], strings, numbers,
// booleans and one-line arrays. The real app would use a real parser in Rust.

export type Where = "synced" | "machine" | "keychain";
export type Value = string | number | boolean;
export type SettingType =
  | { kind: "bool" }
  | { kind: "number"; min: number; max: number; unit?: string }
  | { kind: "enum"; options: [string, string][] }
  | { kind: "text"; mono?: boolean }
  | { kind: "secret" };

export interface SettingDef {
  /** `table.key`, as written in its file. */
  key: string;
  section: string;
  label: string;
  help: string;
  type: SettingType;
  default: Value;
  where: Where;
}

export const SECTIONS = ["Account", "Sync", "Review", "Appearance", "Status bar", "Storage"] as const;
/** The status bar's switchable parts, in the order of its right-click menu. */
export const STATUS_PARTS = ["status_bar.sync", "status_bar.access", "status_bar.rate_limit", "status_bar.pending_reviews", "status_bar.chord", "status_bar.jobs"];

export const SETTINGS: SettingDef[] = [
  { key: "auth.source", section: "Account", label: "GitHub token", where: "machine", default: "gh",
    help: "Where gh-rev gets its token. The gh CLI's token is reused as is; gh-rev registers no OAuth app of its own.",
    type: { kind: "enum", options: [["gh", "From the gh CLI (gh auth token)"], ["token", "A personal access token"]] } },
  { key: "auth.token", section: "Account", label: "Personal access token", where: "keychain", default: "",
    help: "Used only when the token source is a personal access token. Kept in the OS keychain, never in a file.",
    type: { kind: "secret" } },
  // GitHub does not tell apps what a fine-grained token allows, so the user says; a refused write corrects it.
  { key: "auth.fine_allows", section: "Account", label: "This token allows", where: "machine", default: "unsure",
    help: "Only for a fine-grained token (github_pat_…): GitHub doesn’t tell apps what it allows. Not sure: gh-rev finds out on the first write, and keeps what you were sending.",
    type: { kind: "enum", options: [["read", "Reading only"], ["write", "Reviews and comments on one owner’s repositories"], ["unsure", "Not sure"]] } },
  { key: "auth.fine_owner", section: "Account", label: "Owner", where: "machine", default: "",
    help: "The token’s resource owner: the user or organization its writes are limited to.", type: { kind: "text", mono: true } },

  { key: "sync.workbench_minutes", section: "Sync", label: "Workbench refresh", where: "synced", default: 5,
    help: "How often the open Repository's Workbench is refreshed. Each poll fetches only what changed since the last one.",
    type: { kind: "number", min: 1, max: 60, unit: "minutes" } },
  { key: "sync.open_item_seconds", section: "Sync", label: "Open Issue or PR refresh", where: "synced", default: 60,
    help: "How often the Issue or PR on screen is refreshed while it stays open. It is always refreshed when it is opened.",
    type: { kind: "number", min: 15, max: 600, unit: "seconds" } },
  { key: "sync.small_repository", section: "Sync", label: "Small Repository", where: "synced", default: 300,
    help: "A Repository with at most this many open Issues and PRs loads everything open on its first sync.",
    type: { kind: "number", min: 0, max: 5000, unit: "open Issues and PRs" } },
  { key: "sync.first_sync_days", section: "Sync", label: "First sync of a large Repository", where: "synced", default: 30,
    help: "A larger Repository first loads this many days of updates, plus whatever the Filters, pins and Reviews reach. Older Issues load on demand.",
    type: { kind: "number", min: 1, max: 365, unit: "days" } },

  { key: "review.diff_view", section: "Review", label: "Diff view", where: "synced", default: "unified",
    help: "How a PR's diff opens. Ctrl+Alt+U switches while reviewing.",
    type: { kind: "enum", options: [["unified", "Unified"], ["split", "Side by side"]] } },
  { key: "review.collapse_generated", section: "Review", label: "Collapse Generated files", where: "synced", default: true,
    help: "Generated files (from .gitattributes on the Target branch) start collapsed, with their line counts.",
    type: { kind: "bool" } },
  { key: "review.editor_command", section: "Review", label: "Open in editor", where: "machine", default: "code -g {file}:{line}",
    help: "Run by Ctrl+Shift+E in the Review checkout. {file} and {line} are filled in.",
    type: { kind: "text", mono: true } },

  { key: "appearance.theme", section: "Appearance", label: "Theme", where: "synced", default: "system",
    help: "Follows the system's light or dark mode unless set.",
    type: { kind: "enum", options: [["system", "System"], ["light", "Light"], ["dark", "Dark"]] } },
  { key: "appearance.code_font_size", section: "Appearance", label: "Code font size", where: "synced", default: 13,
    help: "For diffs and code blocks.",
    type: { kind: "number", min: 9, max: 22, unit: "px" } },

  { key: "appearance.list_key_hints", section: "Appearance", label: "Key hints under the Issue list", where: "synced", default: true,
    help: "The legend and the list's keys (↑ ↓ move, E archive, M mute…). ? always lists every key.",
    type: { kind: "bool" } },

  // The status bar's parts. Offline and an expired token show whatever these say: nothing else would tell.
  { key: "status_bar.visible", section: "Status bar", label: "Show the status bar", where: "synced", default: true,
    help: "Off hides it, except that being offline or an expired token still shows, since nothing else would tell you.",
    type: { kind: "bool" } },
  { key: "status_bar.sync", section: "Status bar", label: "Sync status", where: "synced", default: true,
    help: "“✓ Synced 2 min ago” and “Syncing…”; click to refresh.", type: { kind: "bool" } },
  { key: "status_bar.access", section: "Status bar", label: "Read-only", where: "synced", default: true,
    help: "“Read-only” when your token can’t write to this Repository; click for the Account page.", type: { kind: "bool" } },
  { key: "status_bar.rate_limit", section: "Status bar", label: "Rate limit warning", where: "synced", default: true,
    help: "When few GitHub API calls are left and gh-rev refreshes less often.", type: { kind: "bool" } },
  { key: "status_bar.pending_reviews", section: "Status bar", label: "Pending reviews", where: "synced", default: true,
    help: "Draft comments not yet submitted, in any Repository.", type: { kind: "bool" } },
  { key: "status_bar.chord", section: "Status bar", label: "Chord prompt", where: "synced", default: true,
    help: "“(Ctrl+K) was pressed. Waiting for the second key…”", type: { kind: "bool" } },
  { key: "status_bar.jobs", section: "Status bar", label: "Background work", where: "synced", default: true,
    help: "Mirror fetches, first syncs and patch-id checks, with progress.", type: { kind: "bool" } },

  { key: "storage.config_file", section: "Storage", label: "Config file", where: "machine", default: "%APPDATA%\\gh-rev\\config.toml",
    help: "Where config.toml lives. Point it into a folder you sync to share Repositories, Filters, pins and keybindings between machines.",
    type: { kind: "text", mono: true } },
  { key: "storage.data_dir", section: "Storage", label: "Data folder", where: "machine", default: "%LOCALAPPDATA%\\gh-rev",
    help: "The git mirrors, the Review checkout and the local database (Local marks, Last seen, Archived and Muted items). Never synced.",
    type: { kind: "text", mono: true } },
];
export const DEFAULTS: Record<string, Value> = Object.fromEntries(SETTINGS.map((s) => [s.key, s.default]));
export const settingDef = (key: string) => SETTINGS.find((s) => s.key === key);

/** Actions with a key, VS Code defaults. Only remapped ones are written to config.toml. */
export const ACTIONS: { id: string; label: string; key: string }[] = [
  { id: "palette", label: "Command palette", key: "ctrl+shift+p" },
  { id: "quick_open", label: "Quick open", key: "ctrl+p" },
  { id: "settings", label: "Settings", key: "ctrl+," },
  { id: "switch_repository", label: "Switch Repository", key: "ctrl+r" },
  { id: "definition", label: "Go to definition", key: "f12" },
  { id: "peek_definition", label: "Peek definition", key: "alt+f12" },
  { id: "next_change", label: "Next change", key: "f7" },
  { id: "prev_change", label: "Previous change", key: "shift+f7" },
  { id: "next_thread", label: "Next comment thread", key: "f8" },
  { id: "prev_thread", label: "Previous comment thread", key: "shift+f8" },
  { id: "find", label: "Find in diff", key: "ctrl+f" },
  { id: "sidebar", label: "Toggle sidebar", key: "ctrl+b" },
  { id: "diff_view", label: "Unified ↔ side-by-side", key: "ctrl+alt+u" },
  { id: "send", label: "Send comment", key: "ctrl+enter" },
  { id: "editor", label: "Open in editor", key: "ctrl+shift+e" },
  { id: "local_mark", label: "Local mark", key: "ctrl+k m" },
  { id: "pin", label: "Pin or unpin", key: "ctrl+k p" },
  { id: "back", label: "Back", key: "alt+arrowleft" },
  { id: "forward", label: "Forward", key: "alt+arrowright" },
  { id: "github", label: "Open on GitHub", key: "ctrl+shift+g" },
  { id: "archive", label: "Archive the selected item", key: "e" },
  { id: "mute", label: "Mute the selected item", key: "m" },
];
export const DEFAULT_KEYS: Record<string, string> = Object.fromEntries(ACTIONS.map((a) => [a.id, a.key]));
/** Pairs of actions bound to the same key. */
export function keyConflicts(keys: Record<string, string>): [string, string][] {
  const out: [string, string][] = [];
  const ids = Object.keys(keys);
  for (let i = 0; i < ids.length; i++) for (let j = i + 1; j < ids.length; j++) if (keys[ids[i]] && keys[ids[i]] === keys[ids[j]]) out.push([ids[i], ids[j]]);
  return out;
}

export interface FilterConfig { name: string; q: string }
/** A Repository's settings. `clone` is a path, so it lives on this machine; the rest is synced. */
export interface RepoConfig {
  name: string;
  conventions: "auto" | "cpython" | "github";
  firstSync: "auto" | "everything" | "recent";
  filters: FilterConfig[];
  pinned: number[];
  mutedAuthors: string[];
  clone: string;
}
export const CONVENTIONS: [RepoConfig["conventions"], string][] = [["auto", "Detect"], ["cpython", "CPython: gh-N titles, Backports"], ["github", "GitHub: “Fixes #N” links"]];
export const FIRST_SYNC: [RepoConfig["firstSync"], string][] = [["auto", "By size"], ["everything", "Everything open"], ["recent", "Recent updates, older on demand"]];

export interface Config { values: Record<string, Value>; keys: Record<string, string>; repos: RepoConfig[] }

// --- writing ----------------------------------------------------------------------------------------
const q = (v: Value) => (typeof v === "string" ? (v.includes("\\") && !v.includes("'") ? `'${v}'` : JSON.stringify(v)) : String(v));
const arr = (xs: (string | number)[]) => `[${xs.map((x) => q(x)).join(", ")}]`;

function tables(c: Config, where: Where): string[] {
  const out: string[] = [];
  let table = "";
  for (const s of SETTINGS.filter((s) => s.where === where)) {
    const [t, k] = s.key.split(".");
    if (t !== table) { out.push("", `[${t}]`); table = t; }
    out.push(`${k} = ${q(c.values[s.key] ?? s.default)}`);
  }
  return out;
}

/** The synced file: settings, remapped keys, and each Repository with its Filters, pins and Muted authors. */
export function configToml(c: Config): string {
  const out = ["# gh-rev: what travels with you. Sync this file yourself (dotfiles, Dropbox, …).", ...tables(c, "synced")];
  const changed = ACTIONS.filter((a) => c.keys[a.id] !== a.key);
  out.push("", "[keys]", "# Only remapped keys; the rest are the VS Code defaults.");
  for (const a of changed) out.push(`${a.id} = ${q(c.keys[a.id])}`);
  for (const r of c.repos) {
    out.push("", "[[repository]]", `name = ${q(r.name)}`, `conventions = ${q(r.conventions)}`, `first_sync = ${q(r.firstSync)}`,
      `pinned = ${arr(r.pinned)}`, `muted_authors = ${arr(r.mutedAuthors)}`);
    for (const f of r.filters) out.push("", "[[repository.filter]]", `name = ${q(f.name)}`, `q = ${q(f.q)}`);
  }
  return out.join("\n") + "\n";
}

/** This machine's file: paths and the token source. The token itself is in the keychain. */
export function localToml(c: Config, tokenSet: boolean): string {
  const out = ["# gh-rev: this machine only. Not synced.", ...tables(c, "machine")];
  if (tokenSet) out.splice(out.indexOf("[auth]") + 2, 0, "# token: in the OS keychain");
  for (const r of c.repos) if (r.clone) out.push("", `[repository.${q(r.name)}]`, `# Borrow objects from this clone, read-only (--reference).`, `clone = ${q(r.clone)}`);
  return out.join("\n") + "\n";
}

// --- reading ----------------------------------------------------------------------------------------
export interface TomlError { line: number; message: string }
type Table = Record<string, unknown>;

function parseValue(s: string): unknown {
  s = s.trim();
  if (s === "true" || s === "false") return s === "true";
  if (/^-?\d+(\.\d+)?$/.test(s)) return +s;
  if (s.startsWith('"')) return JSON.parse(s);
  if (s.startsWith("'") && s.endsWith("'") && s.length > 1) return s.slice(1, -1);
  if (s.startsWith("[") && s.endsWith("]")) {
    const inner = s.slice(1, -1).trim();
    return inner ? (inner.match(/"(?:[^"\\]|\\.)*"|'[^']*'|[^,\s][^,]*/g) ?? []).map(parseValue) : [];
  }
  throw new Error(`not a value: ${s}`);
}
const stripComment = (line: string) => {
  let inS: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (inS) { if (c === "\\" && inS === '"') i++; else if (c === inS) inS = null; }
    else if (c === '"' || c === "'") inS = c;
    else if (c === "#") return line.slice(0, i);
  }
  return line;
};
const splitKey = (k: string) => (k.match(/"[^"]*"|'[^']*'|[^.\s]+/g) ?? []).map((p) => p.replace(/^["']|["']$/g, ""));

/** The TOML subset above, into nested tables. Errors carry 1-based line numbers. */
export function parseToml(text: string): { data: Table; errors: TomlError[] } {
  const data: Table = {};
  const errors: TomlError[] = [];
  let cur: Table = data;
  text.split(/\r?\n/).forEach((raw, i) => {
    const line = stripComment(raw).trim();
    if (!line) return;
    try {
      let m;
      if ((m = /^\[\[(.+)\]\]$/.exec(line))) {
        const path = splitKey(m[1]);
        let t: Table = data;
        for (const p of path.slice(0, -1)) { const v = t[p]; t = (Array.isArray(v) ? v.at(-1) : (t[p] ??= {})) as Table; }
        const last = path.at(-1)!;
        const list = (t[last] ??= []) as Table[];
        if (!Array.isArray(list)) throw new Error(`${m[1]} is not a list of tables`);
        list.push((cur = {}));
      } else if ((m = /^\[(.+)\]$/.exec(line))) {
        let t: Table = data;
        for (const p of splitKey(m[1])) { const v = t[p]; t = (Array.isArray(v) ? v.at(-1) : (t[p] ??= {})) as Table; }
        cur = t;
      } else if ((m = /^([\w"'.-]+)\s*=\s*(.+)$/.exec(line))) {
        const key = splitKey(m[1]).join(".");
        if (key in cur) throw new Error(`${key} is set twice`);
        cur[key] = parseValue(m[2]);
      } else throw new Error("expected key = value or [table]");
    } catch (e) {
      errors.push({ line: i + 1, message: (e as Error).message });
    }
  });
  return { data, errors };
}

/** Checks a parsed file against the settings and applies what is valid onto `base`. */
export function applyToml(base: Config, data: Table, where: "synced" | "machine"): { config: Config; errors: string[] } {
  const errors: string[] = [];
  const values = { ...base.values };
  const known = new Set(["keys", "repository"]);
  for (const s of SETTINGS.filter((s) => s.where === where)) {
    const [t, k] = s.key.split(".");
    known.add(t);
    const v = (data[t] as Table | undefined)?.[k];
    if (v === undefined) { values[s.key] = s.default; continue; }
    const err = checkValue(s, v);
    if (err) errors.push(`${s.key}: ${err}`); else values[s.key] = v as Value;
  }
  for (const [t, v] of Object.entries(data)) {
    if (!known.has(t)) { errors.push(`[${t}]: unknown table`); continue; }
    if (t === "keys" || t === "repository" || typeof v !== "object") continue;
    for (const k of Object.keys(v as Table)) if (!settingDef(`${t}.${k}`)) errors.push(`${t}.${k}: unknown setting`);
    for (const k of Object.keys(v as Table)) { const d = settingDef(`${t}.${k}`); if (d && d.where !== where) errors.push(`${t}.${k}: belongs in ${d.where === "synced" ? "config.toml" : d.where === "machine" ? "local.toml" : "the OS keychain"}`); }
  }
  let keys = base.keys;
  let repos = base.repos;
  if (where === "synced") {
    keys = { ...DEFAULT_KEYS };
    for (const [id, k] of Object.entries((data.keys as Table | undefined) ?? {})) {
      if (!(id in DEFAULT_KEYS)) errors.push(`keys.${id}: unknown action`);
      else if (typeof k !== "string") errors.push(`keys.${id}: expected a key like "ctrl+shift+p"`);
      else keys[id] = k;
    }
    const list = (data.repository as Table[] | undefined) ?? [];
    if (!Array.isArray(list)) errors.push("repository: use [[repository]]");
    else repos = list.map((r, i) => {
      const old = base.repos.find((x) => x.name === r.name);
      if (typeof r.name !== "string") errors.push(`repository #${i + 1}: needs name = "owner/name"`);
      const pick = <T extends string>(v: unknown, ok: [T, string][], what: string, def: T): T =>
        v === undefined ? def : ok.some(([o]) => o === v) ? (v as T) : (errors.push(`${r.name}: ${what} must be ${ok.map(([o]) => `"${o}"`).join(", ")}`), def);
      return {
        name: String(r.name ?? "?"),
        conventions: pick(r.conventions, CONVENTIONS, "conventions", "auto"),
        firstSync: pick(r.first_sync, FIRST_SYNC, "first_sync", "auto"),
        pinned: Array.isArray(r.pinned) ? r.pinned.filter((n): n is number => typeof n === "number") : [],
        mutedAuthors: Array.isArray(r.muted_authors) ? r.muted_authors.map(String) : [],
        filters: Array.isArray(r.filter) ? (r.filter as Table[]).map((f) => ({ name: String(f.name ?? ""), q: String(f.q ?? "") })) : [],
        clone: old?.clone ?? "",
      };
    });
  } else {
    const local = (data.repository as Table | undefined) ?? {};
    repos = base.repos.map((r) => ({ ...r, clone: String(((local[r.name] as Table | undefined)?.clone) ?? "") }));
    for (const n of Object.keys(local)) if (!base.repos.some((r) => r.name === n)) errors.push(`repository."${n}": not in config.toml`);
  }
  return { config: { values, keys, repos }, errors };
}

export function checkValue(s: SettingDef, v: unknown): string | null {
  const t = s.type;
  if (t.kind === "bool") return typeof v === "boolean" ? null : "expected true or false";
  if (t.kind === "number") return typeof v !== "number" ? "expected a number" : v < t.min || v > t.max ? `between ${t.min} and ${t.max}` : null;
  if (t.kind === "enum") return t.options.some(([o]) => o === v) ? null : `one of ${t.options.map(([o]) => `"${o}"`).join(", ")}`;
  return typeof v === "string" ? null : "expected a string";
}

/** Settings whose label, help or key match every word, for a search box. */
export function searchSettings(q: string): SettingDef[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  return SETTINGS.filter((s) => words.every((w) => `${s.label} ${s.help} ${s.key} ${s.section}`.toLowerCase().includes(w)));
}

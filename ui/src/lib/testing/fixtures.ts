// What the core answers, built the way the tests need it. It imports no Vitest, so the unit tests and the
// e2e specs share it, and it imports nothing at run time from the app, which Playwright can't resolve.
// They build the `_Serialize` shapes, which fill every field and so fit the `_Deserialize` ones too.
import type {
  DiffFile_Serialize as DiffFile,
  DiffThread_Serialize as DiffThread,
  HourPoints_Serialize as HourPoints,
  LanguageServerSettings_Serialize as LanguageServerSettings,
  ListedItem_Serialize as ListedItem,
  LoadedSettings_Serialize as LoadedSettings,
  PrFiles_Serialize as PrFiles,
  RepositoryCheck_Serialize as RepositoryCheck,
  Settings_Serialize as Settings,
  Themes,
  ThemeView,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../data/bindings";

/** A command's answer of `data`. */
export const ok = <T>(data: T) => ({ status: "ok" as const, data });

/** The answer of a command that only says it's done. */
export const done = ok(null);

/** A command's answer that it failed, saying `message`. */
export const refused = (message: string) => ({
  status: "error" as const,
  error: { kind: "internal" as const, message },
});

/** The Workbench's answer, listing `items`, counting those that Need attention, with no folded Muted item,
 * no Review request, no Stale review and no Filter, unless `more` says otherwise. The items stay as loosely
 * typed as the caller made them: a mocked command checks nothing, and a typed one, as in the e2e specs,
 * checks them there. */
export const workbenchPage = <Item, Filter = never>(
  items: Item[],
  more: {
    muted?: Item[];
    count?: number;
    review_requests?: number;
    stale_reviews?: number;
    filters?: Filter[];
  } = {},
) =>
  ok({
    items,
    muted: more.muted ?? [],
    count: more.count ?? items.filter((item) => (item as { attention?: unknown }).attention != null).length,
    review_requests: more.review_requests ?? 0,
    stale_reviews: more.stale_reviews ?? 0,
    filters: more.filters ?? ([] as Filter[]),
  });

/** PR `number`'s row, open and unreviewed, its CI passed, seen, with what `overrides` says instead. */
export const workbenchRow = (
  number: number,
  overrides: Omit<Partial<WorkbenchRow>, "number"> = {},
): WorkbenchRow => ({
  number,
  title: "gh-7: Fix it",
  author: "pr-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: { kind: "unreviewed" },
  seen: { mark: { numbers: [number], up_to: null, findings: [] }, state: "seen" },
  mute_author: { kind: "mute", login: "pr-author" },
  ...overrides,
});

/** Issue `number` as a list holds it: open, with no PRs, seen, nothing that Needs attention, and not
 * hidden, with what `overrides` says instead. */
export const listedIssue = (
  number: number,
  overrides: Omit<Partial<ListedItem & { kind: "issue" }>, "kind" | "number"> = {},
): ListedItem & { kind: "issue" } => ({
  kind: "issue",
  number,
  title: `Issue ${String(number)}`,
  state: "OPEN",
  people: [],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [],
  seen: { mark: { numbers: [number], up_to: null, findings: [] }, state: "seen" },
  mute_author: { kind: "mute", login: "issue-author" },
  attention: null,
  key: { kind: "issue", number },
  hidden: null,
  ...overrides,
});

/** A file of the diff at `path` whose header is row `row`, adding a line, with what `more` says instead. */
export const diffFile = (path: string, row: number, more: Partial<DiffFile> = {}): DiffFile => ({
  path,
  old_path: null,
  change: { kind: "lines", added: 1, removed: 0 },
  row,
  columns: 4,
  generated: false,
  gaps: [],
  rest: null,
  pairs: [],
  ...more,
});

/** The `commands` of a mocked `$lib/data/bindings` that a view in the Detail pane tells the core through
 * what the user did with its item, each answering that it did. A `vi.mock` factory takes this with
 * `await import`, as it does `prFilesEvents`. */
export const itemMoves = () => {
  const answered = async () => done;
  return { hideItem: answered, showItem: answered, closeItem: answered };
};

/** The events the Detail pane listens to while it shows a PR: its view's, and its Files tab's. */
const PR_FILES_EVENTS = [
  "changed",
  "readFailed",
  "headsFetched",
  "headsFetchFailed",
  "cloningChanged",
  "mirrorStatesChanged",
  "mirrorsHeldChanged",
] as const;

/** An event the Detail pane listens to while it shows a PR. */
export type PrFilesEvent = (typeof PR_FILES_EVENTS)[number];

/** The `events` of a mocked `$lib/data/bindings` that a shown PR listens to, each listening with what
 * `listen` gives for it, so an event the Detail pane starts to listen to is added here once. A `vi.mock`
 * factory is hoisted above the imports, so it takes this with `await import`. */
export const prFilesEvents = <Listen>(listen: (event: PrFilesEvent) => Listen) =>
  Object.fromEntries(PR_FILES_EVENTS.map((event) => [event, { listen: listen(event) }])) as Record<
    PrFilesEvent,
    { listen: Listen }
  >;

/** The Files tab's answer once the diff is ready. */
export type ReadyFiles = Extract<PrFiles, { state: "ready" }>;

/** A ready diff of `rows` rows from `base` to `abc`, with no file, NEWS entry or thread, unless `more`
 * says otherwise. */
export const readyFiles = (rows: number, more: Partial<Omit<ReadyFiles, "state">> = {}): ReadyFiles => ({
  state: "ready",
  head: "abc",
  base: "base",
  files: [],
  rows,
  news: [],
  threads: { kind: "shown", threads: [] },
  ...more,
});

/** Review thread `id` under row `at` of the diff, or at `at` when it isn't a number, with `comments` comments
 * by some-reviewer. */
export const diffThread = (
  id: string,
  at: number | DiffThread["at"],
  resolution: "open" | "resolved" = "open",
  comments = 1,
): DiffThread => ({
  id,
  at: typeof at === "number" ? { kind: "row", row: at } : at,
  resolution,
  comments: Array.from({ length: comments }, () => reviewComment()),
  later_comments: 0,
});

/** A submitted comment of a review thread, by some-reviewer, with what `more` says instead. */
export const reviewComment = (
  more: Partial<DiffThread["comments"][number]> = {},
): DiffThread["comments"][number] => ({
  id: null,
  author: "some-reviewer",
  created_at: "2026-11-04T10:00:00Z",
  body_html: "<p>Why?</p>",
  hidden: null,
  submission: "submitted",
  ...more,
});

/** The core's answer to a search of the diff, which found `matches`, each a row and where in it the text
 * starts and ends. */
export const foundText = (matches: [number, number, number][], more = false) =>
  ok({ matches: matches.map(([row, start, end]) => ({ row, start, end })), more });

/** The core's read of lines `start` to `end`, the end left out, of a file of `count` lines, line N of which
 * reads `line N`, as it reads a hit's file. */
export const fileLines =
  (count: number) =>
  ({ start, end }: { start: number; end: number }) => {
    const lines = [];
    for (let line = start; line < Math.min(end, count + 1); line++) lines.push(`line ${String(line)}`);
    return Promise.resolve(ok({ count, lines }));
  };

/** `machine.toml`'s language servers when it names none. */
export const NO_SERVERS: LanguageServerSettings = {
  ty: null,
  "rust-analyzer": null,
  clangd: null,
  idle_minutes: 30,
};

/** The settings of a `settings.toml` that sets nothing, with `more` instead. */
export const defaultSettings = (more: Partial<Settings> = {}): Settings => ({
  status_bar: { visible: true },
  releases: { check: true },
  appearance: { theme: "system", light_theme: "VS Code Light Modern", dark_theme: "VS Code Dark Modern" },
  diff: { files: "all", peek: "inline" },
  keys: {},
  repositories: [],
  ...more,
});

/** The settings the core read: the defaults with `more` instead, no language server and no problem. */
export const loadedSettings = (more: Partial<Settings> = {}): LoadedSettings => ({
  settings: defaultSettings(more),
  language_servers: NO_SERVERS,
  problems: [],
});

/** The core's check of a small Repository named `name`, as Add Repository shows it. */
export const repositoryCheck = (name = "some-maintainer/gcmon"): RepositoryCheck => ({
  repository: { name, conventions: "github", filters: [] },
  open_issues: 10,
  open_prs: 3,
  size: "small",
  window_days: 30,
  first_sync_items: 48,
  first_sync_seconds: 5,
  first_sync_points: 12,
  mirror: { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 },
});

/** A theme named `name` with no tokens. */
export const themeView = (name: string, kind: "light" | "dark"): ThemeView => ({ name, kind, tokens: [] });

/** VS Code's and Gruvbox's themes of each side, with VS Code Light Modern and `dark` chosen and the system
 * deciding the side. */
export const builtInThemes = (dark: string): Themes => ({
  theme: { kind: "system" },
  light: {
    chosen: "VS Code Light Modern",
    all: [themeView("VS Code Light Modern", "light"), themeView("Gruvbox Light Hard", "light")],
    failed: [],
  },
  dark: {
    chosen: dark,
    all: [themeView("VS Code Dark Modern", "dark"), themeView("Gruvbox Dark Hard", "dark")],
    failed: [],
  },
  problems: [],
});

/** The token's points this hour, of which Tsugiki spent 3,610 and other tools 150. */
export const HOUR_POINTS: HourPoints = {
  limit: 5000,
  used: 3760,
  remaining: 1240,
  resets_at: "2026-11-17T14:32:00Z",
  spent: 3610,
  since: null,
};

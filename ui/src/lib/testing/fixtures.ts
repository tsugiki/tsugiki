// What the core answers, built the way the tests need it. It imports no Vitest, so the unit tests and the
// e2e specs share it, and it imports nothing at run time from the app, which Playwright can't resolve.
// They build the `_Serialize` shapes, which fill every field and so fit the `_Deserialize` ones too.
import type {
  Account,
  DiffFile_Serialize as DiffFile,
  Error_Serialize as CoreError,
  DiffThread_Serialize as DiffThread,
  Home_Serialize as Home,
  HourPoints_Serialize as HourPoints,
  InboxCursor_Serialize as InboxCursor,
  LanguageServerSettings_Serialize as LanguageServerSettings,
  ListedItem_Serialize as ListedItem,
  LoadedSettings_Serialize as LoadedSettings,
  Opened_Serialize as Opened,
  PrFiles_Serialize as PrFiles,
  RepositoryCheck_Serialize as RepositoryCheck,
  RepositorySettings_Serialize as RepositorySettings,
  Settings_Serialize as Settings,
  ShowMuted,
  Status_Serialize as Status,
  Themes,
  ThemeView,
  TreeRow,
  Truncated,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../data/bindings";

/** The app-wide status that says `account`, with `problem` beside it, or nothing. */
export const statusOf = (account: Account, problem: CoreError | null = null): Status => ({
  account,
  problem,
});

/** The status while octocat is signed in, with no problem. */
export const OCTOCAT_ONLINE = statusOf({ state: "online", login: "octocat" });

/** A command's answer of `data`. */
export const ok = <T>(data: T) => ({ status: "ok" as const, data });

/** The answer of a command that only says it's done. */
export const done = ok(null);

/** A command's answer that it failed, saying `message`. */
export const refused = (message: string) => ({
  status: "error" as const,
  error: { kind: "internal" as const, message },
});

/** A page of the Inbox holding `items`, whose next older page starts at `older`, with "Show muted" as
 * `show_muted` says, `needs_github` for a search the cache doesn't read, `truncated` when a search past
 * GitHub's 1,000 results stopped there (#465), and the Filter the search would save as (#471). The items
 * keep the caller's types, as in `workbenchPage`. */
export const inboxPage = <Item>(
  items: Item[],
  older: InboxCursor | null = null,
  show_muted: ShowMuted = "off",
  needs_github = false,
  truncated: Truncated | null = null,
  filter_query: string | null = null,
) => ok({ items, older, show_muted, needs_github, truncated, filter_query });

/** A result a search found that the cache lacks, as the Inbox draws it in GitHub's order (#465). */
export const placeholderItem = (
  number: number,
  item_kind: "issue" | "pull_request",
  updated_at = "2026-11-04T10:00:00Z",
): ListedItem => ({
  attention: null,
  key: { kind: item_kind, number },
  hidden: null,
  unpin: "stays",
  kind: "placeholder",
  number,
  item_kind,
  updated_at,
});

/** The Workbench's answer, listing `items`, counting those that Need attention, with no folded Muted item,
 * no Review request, no Stale review, no Pinned issue that Needs attention and no Filter, unless `more` says
 * otherwise. The items stay as loosely typed as the caller made them: a mocked command checks nothing, and a
 * typed one, as in the e2e specs, checks them there. */
export const workbenchPage = <Item, Filter = never>(
  items: Item[],
  more: {
    muted?: Item[];
    count?: number;
    review_requests?: number;
    stale_reviews?: number;
    pinned?: number;
    filters?: Filter[];
  } = {},
) =>
  ok({
    items,
    muted: more.muted ?? [],
    count: more.count ?? items.filter((item) => (item as { attention?: unknown }).attention != null).length,
    review_requests: more.review_requests ?? 0,
    stale_reviews: more.stale_reviews ?? 0,
    pinned: more.pinned ?? 0,
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
  overrides: Omit<Partial<Extract<ListedItem, { kind: "issue" }>>, "kind" | "number"> = {},
): Extract<ListedItem, { kind: "issue" }> => ({
  kind: "issue",
  number,
  title: `Issue ${String(number)}`,
  state: "OPEN",
  people: [],
  updated_at: "2026-11-04T10:00:00Z",
  issue_updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [],
  seen: { mark: { numbers: [number], up_to: null, findings: [] }, state: "seen" },
  mute_author: { kind: "mute", login: "issue-author" },
  pinned: false,
  attention: null,
  key: { kind: "issue", number },
  hidden: null,
  unpin: "stays",
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
  tree: { rows: [], folded: false },
  rows,
  news: [],
  threads: { kind: "shown", threads: [] },
  ...more,
});

/** A row of the rail's tree for the folder at `path`, which shows `name`, sits `depth` folders down and
 * holds the rows up to `end`. */
export const folderRow = (path: string, name: string, depth: number, end: number): TreeRow => ({
  kind: "folder",
  path,
  name,
  depth,
  end,
});

/** A row of the rail's tree for the file at `file` among the diff's files, named `name`, `depth` folders
 * down, with no thread or draft unless `notes` says so. */
export const fileRow = (
  file: number,
  name: string,
  depth: number,
  notes: { threads?: number; drafts?: number } = {},
): TreeRow => ({ kind: "file", file, name, depth, threads: 0, drafts: 0, ...notes });

/** A ready diff of four files, three of them in `Lib/`, their headers at the diff's rows 0, 10, 20 and 30,
 * with the tree the core sends with it, its folders folded if `folded` says the tree starts so.
 *
 * | Tree row | What |
 * |---|---|
 * | 0 | `Lib`, which holds rows 1 to 4 |
 * | 1 | `Lib/ast.py`, file 0 |
 * | 2 | `Lib/test`, which holds rows 3 and 4 |
 * | 3, 4 | `Lib/test/test_ast.py` and `Lib/test/test_re.py`, files 1 and 2 |
 * | 5 | `README.rst`, file 3 |
 */
export const treedFiles = (folded = false): ReadyFiles =>
  readyFiles(40, {
    files: [
      diffFile("Lib/ast.py", 0),
      diffFile("Lib/test/test_ast.py", 10),
      diffFile("Lib/test/test_re.py", 20),
      diffFile("README.rst", 30),
    ],
    tree: {
      rows: [
        folderRow("Lib", "Lib", 0, 5),
        fileRow(0, "ast.py", 1),
        folderRow("Lib/test", "test", 1, 5),
        fileRow(1, "test_ast.py", 2),
        fileRow(2, "test_re.py", 2),
        fileRow(3, "README.rst", 0),
      ],
      folded,
    },
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

/** What `home` answers with the window open on `repository`, and `others` below it in the picker, each
 * named, or named with when the user opened it. With no time given, none was opened at a time. */
export const openOn = (
  repository: RepositorySettings,
  others: readonly (string | Opened)[] = [],
  openedAt: string | null = null,
): Extract<Home, { screen: "repository" }> => ({
  screen: "repository",
  repository,
  opened_at: openedAt,
  others: others.map((other) => (typeof other === "string" ? { name: other, opened_at: null } : other)),
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

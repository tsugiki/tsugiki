// What the core answers, built the way the tests need it. It imports no Vitest, so the unit tests and the
// e2e specs share it, and it imports nothing at run time from the app, which Playwright can't resolve.
// They build the `_Serialize` shapes, which fill every field and so fit the `_Deserialize` ones too.
import type {
  DiffFile_Serialize as DiffFile,
  PrFiles_Serialize as PrFiles,
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

/** The Workbench's answer, listing `items`, counting those that Need attention, with no Review request, no
 * Stale review and no Filter, unless `more` says otherwise. The items stay as loosely typed as the caller
 * made them: a mocked command checks nothing, and a typed one, as in the e2e specs, checks them there. */
export const workbenchPage = <Item, Filter = never>(
  items: Item[],
  more: { count?: number; review_requests?: number; stale_reviews?: number; filters?: Filter[] } = {},
) =>
  ok({
    items,
    count: more.count ?? items.filter((item) => (item as { attention?: unknown }).attention != null).length,
    review_requests: more.review_requests ?? 0,
    stale_reviews: more.stale_reviews ?? 0,
    filters: more.filters ?? ([] as Filter[]),
  });

/** A PR's row, open and unreviewed, its CI passed, seen, with what `overrides` says instead. */
export const workbenchRow = (overrides: Partial<WorkbenchRow> = {}): WorkbenchRow => {
  const number = overrides.number ?? 10;
  return {
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
    seen: { mark: { numbers: [number], up_to: null }, state: "seen" },
    ...overrides,
  };
};

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

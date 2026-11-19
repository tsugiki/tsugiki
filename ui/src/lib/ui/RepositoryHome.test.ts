import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, onTestFinished, vi } from "vitest";
import { FileView } from "$lib/state/file-view.svelte";
import type { HitCode } from "$lib/state/hit-code.svelte";
import { type Action, keysIn, keyTable, remapKeys } from "$lib/state/keymap.svelte";
import { keyTarget } from "./keys";
import type { Peek, PeekLayout } from "$lib/state/peek";
import { PrFilesState, type FilesMode } from "$lib/state/pr-files.svelte";
import { PrViewState } from "$lib/state/pr-view.svelte";
import type { MuteAuthor } from "$lib/state/workbench.svelte";
import { done, ok, readyFiles, workbenchPage, workbenchRow } from "$lib/testing/fixtures";
import { keydown, nextTask, pressed } from "$lib/testing/helpers";
import RepositoryHome from "./RepositoryHome.svelte";

/** What the page asks the screen for the palette's rows, from the screen's own exports. */
type PaletteScreen = Pick<
  ReturnType<typeof render<typeof RepositoryHome>>["component"],
  "paletteRow" | "paneOf"
>;

/** The palette's row for `action` on `screen`, as the page asks for it, for the palette opened with the
 * focus on `target`: on no control at all, which goes to the list, unless it says. */
function paletteRow(screen: PaletteScreen, action: Action, target: EventTarget | null = document.body) {
  const row = keyTable().find((one) => one.action === action);
  return row && screen.paletteRow(row, keyTarget(target, screen.paneOf));
}

const core = vi.hoisted(() => ({
  workbench: vi.fn(),
  inbox: vi.fn(),
  issue: vi.fn(),
  openIssue: vi.fn(),
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  hideItem: vi.fn(),
  showItem: vi.fn(),
  closeItem: vi.fn(),
  markSeen: vi.fn(),
  markUnseen: vi.fn(),
  archive: vi.fn(),
  keepArchived: vi.fn(),
  unarchive: vi.fn(),
  mute: vi.fn(),
  unmute: vi.fn(),
  muteAuthor: vi.fn(),
  unmuteAuthor: vi.fn(),
  putFilterOnWorkbench: vi.fn(),
  openOnGithub: vi.fn(),
  pullRequestFiles: vi.fn(),
  pullRequestFind: vi.fn(),
  definitions: vi.fn(),
  // A PR that leaves the Detail pane cancels the lookup it may still run.
  cancelDefinitions: vi.fn(() => Promise.resolve()),
  cloning: vi.fn(),
  mirrorStates: vi.fn(),
  mirrorsHeld: vi.fn(),
  paneLayout: vi.fn(),
  savePaneLayout: vi.fn(),
  // The HUD logs a slow opening of an Issue whether or not it shows.
  recordMiss: vi.fn(() => Promise.resolve("a miss")),
  textSearch: vi.fn(),
  textSearchState: vi.fn(),
  listen: vi.fn(),
  stopListening: vi.fn(),
}));

vi.mock("$lib/data/bindings", async () => {
  const { prFilesEvents } = await import("$lib/testing/fixtures");
  return {
    commands: {
      workbench: core.workbench,
      inbox: core.inbox,
      issue: core.issue,
      openIssue: core.openIssue,
      pullRequest: core.pullRequest,
      openPullRequest: core.openPullRequest,
      hideItem: core.hideItem,
      showItem: core.showItem,
      closeItem: core.closeItem,
      markSeen: core.markSeen,
      markUnseen: core.markUnseen,
      archive: core.archive,
      keepArchived: core.keepArchived,
      unarchive: core.unarchive,
      mute: core.mute,
      unmute: core.unmute,
      muteAuthor: core.muteAuthor,
      unmuteAuthor: core.unmuteAuthor,
      putFilterOnWorkbench: core.putFilterOnWorkbench,
      openOnGithub: core.openOnGithub,
      pullRequestFiles: core.pullRequestFiles,
      pullRequestFind: core.pullRequestFind,
      definitions: core.definitions,
      cancelDefinitions: core.cancelDefinitions,
      cloning: core.cloning,
      mirrorStates: core.mirrorStates,
      mirrorsHeld: core.mirrorsHeld,
      paneLayout: core.paneLayout,
      savePaneLayout: core.savePaneLayout,
      recordMiss: core.recordMiss,
      textSearch: core.textSearch,
      textSearchState: core.textSearchState,
      textSearchCancel: () => Promise.resolve(null),
    },
    events: {
      ...prFilesEvents(() => core.listen),
      textSearchChanged: { listen: core.listen },
    },
  };
});

// A lookup hears what it waits for on a channel, which these tests leave quiet.
vi.mock("$lib/data/channel", () => ({
  Channel: class {
    onmessage = () => {};
  },
}));

const row = workbenchRow(160662, {
  title: "gh-112564: Fix UAF for gc debug print",
  author: "some-author",
  reasons: ["review_request"],
  seen: {
    mark: { numbers: [160662], up_to: "2026-11-04T10:00:00Z" },
    state: "new_activity",
  },
  mute_author: { kind: "mute", login: "some-author" },
});

const issue = {
  kind: "issue",
  number: 112564,
  title: "GIL drop during collection",
  state: "OPEN",
  people: ["issue-author", "some-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ kind: "pr", ...row, backports: [] }],
  seen: {
    mark: { numbers: [112564, 160662], up_to: "2026-11-04T10:00:00Z" },
    state: "new_activity",
  },
  mute_author: { kind: "mute", login: "issue-author" } as MuteAuthor,
  key: { kind: "issue", number: 112564 },
  hidden: null,
};

const DEFAULT_LAYOUT = {
  sidebar: null,
  list: null,
  peek: null,
  split_peek: null,
  rail: null,
  sidebar_hidden: false,
  sidebar_icons: false,
  list_hidden: false,
  detail_only_hid: null,
  order: "attention",
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(core.stopListening);
  core.openIssue.mockResolvedValue(done);
  core.openPullRequest.mockResolvedValue(done);
  for (const move of [core.hideItem, core.showItem, core.closeItem]) move.mockResolvedValue(done);
  core.pullRequestFiles.mockResolvedValue({ status: "ok", data: { state: "fetching" } });
  core.pullRequestFind.mockResolvedValue({ status: "ok", data: { matches: [], more: false } });
  core.cloning.mockResolvedValue(null);
  core.mirrorStates.mockResolvedValue([]);
  core.mirrorsHeld.mockResolvedValue(false);
  core.paneLayout.mockResolvedValue(DEFAULT_LAYOUT);
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULT_LAYOUT });
  core.pullRequest.mockImplementation((_repository: string, number: number) =>
    Promise.resolve({ status: "ok", data: { ...row, number, title: `PR ${String(number)}`, content: null } }),
  );
  core.openOnGithub.mockResolvedValue(done);
  core.markSeen.mockResolvedValue(done);
  core.markUnseen.mockResolvedValue(done);
  core.archive.mockResolvedValue(done);
  core.keepArchived.mockResolvedValue(done);
  core.unarchive.mockResolvedValue(done);
  core.mute.mockResolvedValue(done);
  core.unmute.mockResolvedValue(done);
  core.muteAuthor.mockResolvedValue(done);
  core.unmuteAuthor.mockResolvedValue(done);
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      merged: [],
      merged_count: 0,
      author: "issue-author",
      assignees: [],
      labels: [],
      needs_attention: [],
      content: null,
      seen: issue.seen,
      mute_author: issue.mute_author,
      hidden: null,
    },
  });
  core.workbench.mockResolvedValue(workbenchPage([]));
});

afterEach(() => {
  vi.useRealTimers();
});

/** The Issue list's pane, or null while it's hidden. */
const issueList = () => document.querySelector('[data-pane="list"]');

it("names the Repository in its hidden heading and atop the Sidebar, and starts the list with the view", async () => {
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await screen.findByText(/^Nothing here yet/);
  const list = within(issueList() as HTMLElement);
  const sidebar = within(screen.getByRole("navigation", { name: "Sidebar" }));

  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("python/cpython");
  expect(sidebar.getByRole("button", { name: "python/cpython" })).toBeTruthy();
  expect(list.getAllByRole("heading").map((heading) => heading.textContent)).toEqual(["Workbench"]);
  expect(list.queryByRole("button", { name: "Add repository" })).toBeNull();
});

it("names the Repository in its hidden heading with the Sidebar hidden", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar_hidden: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await vi.waitFor(() => {
    expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
  });

  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("python/cpython");
});

it("opens the Repository picker from the name atop the Sidebar", async () => {
  const onpick = vi.fn();
  render(RepositoryHome, { name: "python/cpython", onpick, onsettings: () => {} });
  await fireEvent.click(screen.getByRole("button", { name: "python/cpython" }));
  expect(onpick).toHaveBeenCalledOnce();
});

it("lists a Standalone PR with the time it changed", async () => {
  vi.setSystemTime("2026-11-04T13:00:00Z");
  core.workbench.mockResolvedValue(
    workbenchPage([{ kind: "standalone_pr", ...row, backports: [] }], { count: 1 }),
  );
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const item = await screen.findByRole("listitem");

  expect(core.workbench).toHaveBeenCalledWith("python/cpython", { kind: "whole" });
  expect(
    within(item).getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  ).toBeTruthy();
  expect(within(item).getByText("some-author")).toBeTruthy();
  expect(within(item).getByText("3h ago")).toBeTruthy();
  expect(within(item).getByText("Review request · some-author · into main · Open · CI passed")).toBeTruthy();
});

it("moves a time on while the window stays open", async () => {
  // Only these, because the wait for the first answer runs on the real `setTimeout`.
  vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
  vi.setSystemTime("2026-11-04T13:00:00Z");
  core.workbench.mockResolvedValue(workbenchPage([issue], { count: 1 }));
  const { unmount } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByText("3h ago");

  await vi.advanceTimersByTimeAsync(60 * 60_000);

  expect(screen.getByText("4h ago")).toBeTruthy();
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("says an Issue has no PR yet, with no count of none", async () => {
  core.workbench.mockResolvedValue(
    workbenchPage([{ ...issue, people: [], pull_requests: [] }], { count: 1 }),
  );
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const item = await screen.findByRole("listitem");

  expect([".people", ".count", ".facts"].map((part) => item.querySelector(part)?.textContent)).toEqual([
    "—",
    undefined,
    "No PR yet",
  ]);
});

it("lists an Issue folded, and its PRs under it once unfolded", async () => {
  vi.setSystemTime("2026-11-04T13:00:00Z");
  const attention = { since: "2026-11-04T10:00:00Z", why: [{ kind: "review_requested" }] };
  core.workbench.mockResolvedValue(workbenchPage([{ ...issue, attention }]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const fold = await screen.findByRole("button", { name: "PRs of #112564", expanded: false });
  const folded = [screen.getAllByRole("listitem").length, fold.textContent];
  // The folded row still says why the Issue Needs attention, though the PR that does is folded away.
  expect(screen.getByText("Review requested from you")).toBeTruthy();

  await fireEvent.click(fold);
  const [item, pullRequest] = screen.getAllByRole("listitem");

  expect([...folded, fold.getAttribute("aria-expanded"), fold.textContent]).toEqual([1, "›", "true", "⌄"]);

  expect(
    within(item as HTMLElement).getByRole("button", { name: "#112564 GIL drop during collection" }),
  ).toBeTruthy();
  expect([".people", ".count", ".age"].map((part) => item?.querySelector(part)?.textContent)).toEqual([
    "issue-author, some-author",
    "1 PR",
    "3h ago",
  ]);
  expect(
    within(pullRequest as HTMLElement).getByRole("button", {
      name: "#160662 gh-112564: Fix UAF for gc debug print",
    }),
  ).toBeTruthy();
  expect(
    within(pullRequest as HTMLElement).getByText(
      "Review request · some-author · into main · Open · CI passed",
    ),
  ).toBeTruthy();
});

it("says so while there is nothing to list", async () => {
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  expect(await screen.findByText(/Nothing here yet/)).toBeTruthy();
});

it("shows a Filter's Issues when the user selects it, with the Sidebar's counts", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 1, problem: null };
  core.workbench.mockImplementation(async (_: string, selected: { kind: string }) =>
    workbenchPage(selected.kind === "whole" ? [] : [issue], { count: 7, filters: [mine] }),
  );
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 1" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "filter", query: mine.query });
  expect(await screen.findByRole("button", { name: "#112564 GIL drop during collection" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Workbench 7" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Mine 1" }).getAttribute("aria-current")).toBe("true");
});

it("marks what Needs attention the way unread mail is, and says why", async () => {
  const attention = {
    since: "2026-11-04T10:00:00Z",
    why: [{ kind: "changes_requested", by: "some-reviewer" }, { kind: "review_requested" }],
  };
  const unread = { kind: "standalone_pr", ...row, backports: [], attention };
  core.workbench.mockResolvedValue(workbenchPage([unread]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const item = await screen.findByRole("listitem");

  const title = within(item).getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" });
  expect(title.firstElementChild?.tagName).toBe("STRONG");
  expect(item.classList.contains("attention")).toBe(true);
  expect(
    within(item).getByText("Changes requested by some-reviewer · Review requested from you"),
  ).toBeTruthy();
});

it("lists the items with nothing waiting in the one list, in the core's order, with no section to fold", async () => {
  const standalone = { kind: "standalone_pr", ...row, number: 7, backports: [], attention: null };
  const other = { ...issue, number: 8, title: "Another" };
  core.workbench.mockResolvedValue(workbenchPage([standalone, issue, other]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const title = await screen.findByRole("button", { name: "#7 gh-112564: Fix UAF for gc debug print" });
  expect(title.querySelector("strong")).toBeNull();
  expect(title.closest("li")?.classList.contains("attention")).toBe(false);
  const rows: (Element | null)[] = [
    "#7 gh-112564: Fix UAF for gc debug print",
    "#112564 GIL drop during collection",
    "#8 Another",
  ].map((name) => screen.getByRole("button", { name }).closest("li"));
  expect(screen.getAllByRole("listitem").filter((item) => rows.includes(item))).toEqual(rows);
  expect(screen.queryByRole("button", { name: /quiet/i })).toBeNull();
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("says when nobody has asked for a review", async () => {
  core.workbench.mockResolvedValue(workbenchPage([]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Review requests 0" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "review_requests" });
  expect(await screen.findByText("Nobody has asked you for a review.")).toBeTruthy();
});

it("asks for Stale reviews when the user picks it, and says when no Review of theirs is stale", async () => {
  core.workbench.mockResolvedValue(workbenchPage([], { stale_reviews: 2 }));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Stale reviews 2" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "stale_reviews" });
  expect(await screen.findByRole("heading", { name: "Stale reviews" })).toBeTruthy();
  expect(await screen.findByText("No Review of yours is stale.")).toBeTruthy();
});

it("switches a Filter on and off the Workbench from its view, and only there", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
  let saved = true;
  core.workbench.mockImplementation(async () =>
    workbenchPage([], { filters: [{ ...mine, on_workbench: saved }] }),
  );
  core.putFilterOnWorkbench.mockImplementation(async ({ on }: { on: boolean }) => {
    saved = on;
    return done;
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await screen.findByRole("button", { name: "Mine 0" });
  expect(screen.queryByRole("checkbox", { name: "On the Workbench" })).toBeNull();

  await fireEvent.click(screen.getByRole("button", { name: "Mine 0" }));
  const onWorkbench = await screen.findByRole<HTMLInputElement>("checkbox", { name: "On the Workbench" });
  expect(onWorkbench.checked).toBe(true);
  const asked = core.workbench.mock.calls.length;
  await fireEvent.click(onWorkbench);

  expect(core.putFilterOnWorkbench).toHaveBeenLastCalledWith({
    repository: "python/cpython",
    query: mine.query,
    on: false,
  });
  await waitFor(() => {
    expect(core.workbench).toHaveBeenCalledTimes(asked + 1);
  });
  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "filter", query: mine.query });
  expect(onWorkbench.checked).toBe(false);
  expect(screen.queryByRole("alert")).toBeNull();
});

it("keeps a Filter's switch in focus while its change saves, and saves the next change after it", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
  core.workbench.mockResolvedValue(workbenchPage([], { filters: [mine] }));
  const save = Promise.withResolvers<{ status: "ok"; data: null }>();
  core.putFilterOnWorkbench.mockReturnValueOnce(save.promise);
  core.putFilterOnWorkbench.mockResolvedValue(done);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 0" }));
  const onWorkbench = await screen.findByRole<HTMLInputElement>("checkbox", { name: "On the Workbench" });
  onWorkbench.focus();
  await fireEvent.click(onWorkbench);

  await waitFor(() => {
    expect(onWorkbench.getAttribute("aria-busy")).toBe("true");
  });
  expect(onWorkbench.disabled).toBe(false);
  expect(document.activeElement).toBe(onWorkbench);
  await fireEvent.click(onWorkbench);
  expect(onWorkbench.checked).toBe(true);
  expect(core.putFilterOnWorkbench).toHaveBeenCalledTimes(1);
  save.resolve(done);

  await waitFor(() => {
    expect(onWorkbench.getAttribute("aria-busy")).toBe("false");
  });
  expect(core.putFilterOnWorkbench).toHaveBeenLastCalledWith({
    repository: "python/cpython",
    query: mine.query,
    on: true,
  });
  expect(core.putFilterOnWorkbench).toHaveBeenCalledTimes(2);
});

it("turns a Filter's switch back and says why when the core can't save it", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", on_workbench: false, count: 0, problem: null };
  core.workbench.mockResolvedValue(workbenchPage([], { filters: [mine] }));
  core.putFilterOnWorkbench.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "settings.toml is read-only" },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 0" }));
  const onWorkbench = await screen.findByRole<HTMLInputElement>("checkbox", { name: "On the Workbench" });
  await fireEvent.click(onWorkbench);

  expect((await screen.findByRole("alert")).textContent).toBe(
    "Tsugiki couldn't save the Filter's switch: settings.toml is read-only",
  );
  await waitFor(() => {
    expect(onWorkbench.checked).toBe(false);
  });
});

it("says how to check a Filter that holds nothing", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
  core.workbench.mockResolvedValue(workbenchPage([], { filters: [mine] }));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 0" }));

  expect(await screen.findByText(/check its query in settings.toml/)).toBeTruthy();
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("says why it can't read the Workbench", async () => {
  core.workbench.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Tsugiki can't read this Workbench: no database",
  );
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("opens the other Repository's Workbench and stops listening for the last", async () => {
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByText(/Nothing here yet/);

  await rerender({ name: "tsugiki/tsugiki" });

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("tsugiki/tsugiki", { kind: "whole" });
  });
  expect(core.stopListening).toHaveBeenCalledOnce();
});

it("stops listening when it goes", async () => {
  const { unmount } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByText(/Nothing here yet/);
  unmount();
  expect(core.stopListening).toHaveBeenCalledOnce();
});

const answering = (...items: unknown[]) => {
  core.workbench.mockResolvedValue(workbenchPage(items, { count: items.length }));
};

const backportRow = {
  ...workbenchRow(160700, {
    title: "[3.14] gh-112564: Fix UAF for gc debug print (GH-160662)",
    author: "some-author",
    target_branch: "3.14",
    seen: row.seen,
  }),
  backport: "bot",
};

it("lists an Origin PR's Backports beneath it in the Issue", async () => {
  answering({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [backportRow] }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();

  const origin = (await screen.findByRole("button", { name: /^#160662 / })).closest("li") as HTMLElement;

  const beneath = within(origin)
    .getByRole("button", { name: /^#160700 / })
    .closest("li") as HTMLElement;
  expect(within(beneath).getByText("Bot backport · some-author · into 3.14 · Open · CI passed")).toBeTruthy();
});

it("lists a Backport with no Origin PR directly under its Issue", async () => {
  answering({ ...issue, pull_requests: [{ kind: "backport", ...backportRow }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();

  expect(await screen.findByText("Bot backport · some-author · into 3.14 · Open · CI passed")).toBeTruthy();
});

const staleReview = {
  kind: "stale_review",
  files: [
    { path: "Lib/ast.py", whitespace_only: false },
    { path: "Python/ast.c", whitespace_only: true },
  ],
};
const staleHover = "Changed since your Review: 2 files\nLib/ast.py\nPython/ast.c (whitespace only)";

it("shows a Stale review on a PR's row in the Issue's tree, with the files changed in its hover", async () => {
  const stale = { ...row, reasons: ["reviewed"], your_review: staleReview };
  answering({ ...issue, pull_requests: [{ kind: "pr", ...stale, backports: [] }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();

  const badge = await screen.findByText("Stale review");
  expect(badge.title).toBe(staleHover);
  expect(badge.closest(".facts")?.textContent).toBe(
    "Stale review · some-author · into main · Open · CI passed",
  );
});

it("shows the Origin PR's review state on each Bot backport's row beneath it", async () => {
  const backports = [
    { ...backportRow, your_review: staleReview },
    { ...backportRow, number: 160701, target_branch: "3.13", your_review: { kind: "reviewed" } },
  ];
  const origin = { kind: "pr", ...row, reasons: ["reviewed"], your_review: staleReview, backports };
  answering({ ...issue, pull_requests: [origin] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();

  const facts = (number: number) => {
    const button = screen.getByRole("button", { name: new RegExp(`^#${String(number)} `) });
    return button.closest(".row")?.querySelector(".facts") as HTMLElement;
  };
  await screen.findByRole("button", { name: /^#160700 / });
  expect(within(facts(160700)).getByText("Stale review").title).toBe(staleHover);
  expect(facts(160701).textContent).toBe(
    "Reviewed · Bot backport · some-author · into 3.13 · Open · CI passed",
  );
});

it.each([
  ["Standalone PR", { kind: "standalone_pr", ...row, backports: [] }],
  ["Backport with no Issue", { kind: "standalone_backport", ...row, backport: "manual" }],
])("shows a Stale review on a %s's row while nothing else about it waits", async (_, item) => {
  answering({ ...item, your_review: staleReview, attention: null });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  expect((await screen.findByText("Stale review")).title).toBe(staleHover);
});

it("lists a Standalone PR's Backports beneath it", async () => {
  answering({ kind: "standalone_pr", ...row, backports: [backportRow] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const origin = (await screen.findByRole("button", { name: /^#160662 / })).closest("li") as HTMLElement;

  expect(within(origin).getByRole("button", { name: /^#160700 / })).toBeTruthy();
});

it("lists a Backport with no Issue and no Origin PR by itself", async () => {
  answering({ kind: "standalone_backport", ...backportRow });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  expect(await screen.findByRole("button", { name: /^#160700 / })).toBeTruthy();
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
});

const standalone = {
  kind: "standalone_pr",
  ...row,
  number: 5,
  title: "Alone",
  backports: [],
  key: { kind: "pull_request", number: 5 },
  hidden: null,
};

/** A window on two items: the Issue, which it unfolds, and a Standalone PR. */
const listing = async (settings: { diffFiles?: FilesMode; diffPeek?: PeekLayout } = {}) => {
  core.workbench.mockResolvedValue(
    workbenchPage([
      { ...issue, attention: null },
      { ...standalone, attention: null },
    ]),
  );
  const { component, rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
    ...settings,
  });
  const issueRow = await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  await unfoldAll();
  return {
    component,
    rerender,
    issueRow,
    treeRow: screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
    prRow: screen.getByRole("button", { name: "#5 Alone" }),
  };
};

/** Unfolds each Issue the list shows, once it shows one, as each starts folded (#593). */
const unfoldAll = async () => {
  await screen.findAllByRole("button", { name: /^PRs of #/ });
  for (const fold of screen.queryAllByRole("button", { name: /^PRs of #/, expanded: false })) {
    await fireEvent.click(fold);
  }
};

const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(document.body, { key, ...init });

/** How many of the listeners the page started haven't stopped. */
const listening = () => core.listen.mock.calls.length - core.stopListening.mock.calls.length;

/** Catches every listener the page starts, of any event, so a test can say a Repository changed to them all,
 * as the core would. */
const catchEveryListener = () => {
  const callbacks: ((event: { payload: unknown }) => void)[] = [];
  core.listen.mockImplementation((callback: (event: { payload: unknown }) => void) => {
    callbacks.push(callback);
    return Promise.resolve(core.stopListening);
  });
  return (repository: string, items: number[] = []) => {
    for (const callback of callbacks) callback({ payload: { repository, items } });
  };
};

it("opens an Issue in the Detail pane on a click, and tells the core", async () => {
  const { issueRow } = await listing();

  await fireEvent.click(issueRow);

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
  expect(issueRow.getAttribute("aria-current")).toBe("true");
});

it("says how long ago each comment in the Detail pane came", async () => {
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      author: "issue-author",
      assignees: [],
      labels: [],
      needs_attention: [],
      seen: issue.seen,
      mute_author: issue.mute_author,
      content: {
        body_html: "<p>It crashes</p>",
        comments: [
          { author: "some-reviewer", created_at: new Date().toISOString(), body_html: "<p>Which build?</p>" },
        ],
        earlier_comments: 0,
      },
    },
  });
  const { issueRow } = await listing();

  await fireEvent.click(issueRow);

  expect((await screen.findByText("· just now")).parentElement?.textContent).toBe("some-reviewer · just now");
});

it("opening the Issue on show again tells the core again", async () => {
  const { issueRow } = await listing();

  await fireEvent.click(issueRow);
  await fireEvent.click(issueRow);

  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenCalledTimes(2);
  });
});

it("swaps the Detail pane for the next Issue", async () => {
  core.workbench.mockResolvedValue(
    workbenchPage([
      { ...issue, attention: null },
      { ...issue, number: 2, title: "Next", attention: null },
    ]),
  );
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await fireEvent.click(screen.getByRole("button", { name: "#2 Next" }));

  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenLastCalledWith("python/cpython", 2);
  });
});

it("opens a Standalone PR in the Detail pane on a click, on its Conversation, and tells the core", async () => {
  vi.setSystemTime("2026-11-04T12:00:00Z");
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: {
      ...standalone,
      content: {
        body_html: "<p>Why it fails</p>",
        comments: [{ author: "some-reviewer", created_at: "2026-11-04T10:00:00Z", body_html: "<p>LGTM</p>" }],
        earlier_comments: 0,
      },
    },
  });
  const { prRow } = await listing();

  await fireEvent.click(prRow);

  const conversation = await screen.findByRole("tab", { name: "Conversation" });
  expect(conversation.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByText("Why it fails")).toBeTruthy();
  expect(screen.getByText("· 2h ago").parentElement?.textContent).toBe("some-reviewer · 2h ago");
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 5);
  expect(core.openOnGithub).not.toHaveBeenCalled();
  expect(prRow.getAttribute("aria-current")).toBe("true");
  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens a PR in an Issue's tree with the way back to its Issue, and Esc goes back", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);
  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662);
  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();

  await press("Escape");

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
  expect(issueRow.getAttribute("aria-current")).toBe("true");
});

it("keeps the merged PRs unfolded after one of them opens and Esc comes back", async () => {
  const merged = { kind: "pr", ...row, number: 150001, title: "gh-112564: An old fix", backports: [] };
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      merged: [{ ...merged, state: "MERGED" }],
      merged_count: 1,
      author: "issue-author",
      assignees: [],
      labels: [],
      needs_attention: [],
      content: null,
      seen: issue.seen,
      mute_author: issue.mute_author,
    },
  });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(await screen.findByRole("button", { name: "Show 1 merged PR" }));
  await fireEvent.click(screen.getByRole("button", { name: "#150001 gh-112564: An old fix" }));
  await screen.findByRole("region", { name: "PR view" });

  await press("Escape");

  expect(await screen.findByRole("button", { name: "Hide 1 merged PR" })).toBeTruthy();
  expect(screen.getByRole("list", { name: "Merged PRs" })).toBeTruthy();
});

it("opens a PR that fixes two Issues again from its line under the other one", async () => {
  answering(issue, { ...issue, number: 112565, title: "Another crash" });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();
  const [first, second] = await screen.findAllByRole("button", { name: /^#160662 / });

  await fireEvent.click(first as HTMLElement);
  await screen.findByRole("button", { name: "gh-112564" });
  await fireEvent.click(second as HTMLElement);

  expect(await screen.findByRole("button", { name: "gh-112565" })).toBeTruthy();
  expect((await screen.findByRole("tab", { name: "Conversation" })).getAttribute("aria-selected")).toBe(
    "true",
  );
  expect(second?.getAttribute("aria-current")).toBe("true");
  expect(first?.getAttribute("aria-current")).toBeNull();
});

it("goes back to the Issue with the PR view's gh-N link, and focuses the Issue's row", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);

  await fireEvent.click(await screen.findByRole("button", { name: "gh-112564" }));

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
});

it.each([
  ["the PR's row", (treeRow: HTMLElement) => treeRow],
  ["a PR tab", () => screen.getByRole("tab", { name: "Files" })],
  [
    "the Sidebar",
    () =>
      within(screen.getByRole("navigation", { name: "Sidebar" })).getByRole("button", { name: /^Workbench/ }),
  ],
])("focuses the Issue's row on Esc from %s", async (_, focused) => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("tab", { name: "Files" });
  const from = focused(treeRow) as HTMLElement;
  from.focus();

  await fireEvent.keyDown(from, { key: "Escape" });

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
  expect(issueRow.getAttribute("aria-current")).toBe("true");
});

it("keeps the Issue view while a PR from its tree is on show, and shows it again at once", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("heading", { name: /GIL drop during collection/ });

  await fireEvent.click(treeRow);
  await press("Escape");
  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("button", { name: "gh-112564" }));
  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  await fireEvent.click(treeRow);
  await press("ArrowLeft", { altKey: true });

  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledOnce();
  expect(core.issue).toHaveBeenCalledOnce();
  expect(core.openPullRequest).toHaveBeenCalledTimes(3);
});

it("shows the Issue view it keeps at once on a click on the Issue's line, and tells the core", async () => {
  const send = catchEveryListener();
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("heading", { name: /GIL drop during collection/ });
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  send("python/cpython");

  await fireEvent.click(issueRow);

  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenCalledTimes(2);
  });
  expect(core.issue).toHaveBeenCalledTimes(2);
});

it("opens the Issue afresh on the way back when its view couldn't read it", async () => {
  core.issue.mockResolvedValueOnce({ status: "error", error: { kind: "internal", message: "no database" } });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByText(/no database/);
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });

  await press("Escape");

  expect(await screen.findByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledTimes(2);
});

it("tells the core the Issue view it keeps is hidden, and closes it when the PR gives way to another", async () => {
  const { issueRow, treeRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  const withIssue = listening();
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });

  await vi.waitFor(() => {
    expect(core.hideItem).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
  });
  expect(core.closeItem).not.toHaveBeenCalled();
  await fireEvent.click(prRow);
  await vi.waitFor(() => {
    expect(core.closeItem).toHaveBeenCalledTimes(2);
  });
  expect(core.closeItem).toHaveBeenCalledWith("python/cpython", 112564);
  expect(core.closeItem).toHaveBeenCalledWith("python/cpython", 160662);
  // With the kept view stopped, the Issue opens afresh and listens in its place.
  await fireEvent.click(issueRow);

  expect(core.openIssue).toHaveBeenCalledTimes(2);
  expect(listening()).toBe(withIssue);
});

it("stops the Issue view it keeps when it goes, and tells the core the user left both items", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });

  cleanup();

  expect(listening()).toBe(0);
  await vi.waitFor(() => {
    expect(core.closeItem).toHaveBeenCalledTimes(2);
  });
});

it("opens a Backport in the list in the PR view, with its Issue to go back to", async () => {
  answering({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [backportRow] }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();

  await fireEvent.click(await screen.findByRole("button", { name: /^#160700 / }));

  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160700);
});

it("opens a Standalone PR's Backport in the PR view, with no Issue to go back to", async () => {
  answering({ kind: "standalone_pr", ...row, backports: [backportRow] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: /^#160700 / }));

  expect(await screen.findByRole("tab", { name: "Conversation" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens a PR from the Issue view, and puts the cursor on its line in the tree", async () => {
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      ...issue,
      merged: [],
      merged_count: 0,
      author: "issue-author",
      assignees: [],
      labels: [],
      content: null,
      needs_attention: [],
    },
  });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  const issueView = await screen.findByRole("region", { name: "Issue view" });

  await fireEvent.click(await within(issueView).findByRole("button", { name: /^#160662 / }));

  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662);
  expect(treeRow.getAttribute("aria-current")).toBe("true");
});

it.each([
  ["a Standalone PR", standalone, "#5 Alone"],
  ["a Standalone PR's Backport", { ...standalone, backports: [backportRow] }, /^#160700 /],
  ["a Backport with no Issue", { kind: "standalone_backport", ...backportRow }, /^#160700 /],
])("leaves %s on show on Esc, with no Issue under it", async (_, item, name) => {
  answering(item);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const prRow = await screen.findByRole("button", { name });
  await fireEvent.click(prRow);
  await screen.findByRole("tab", { name: "Conversation" });
  const escape = keydown({ key: "Escape" });

  await fireEvent(document.body, escape);

  expect(escape.defaultPrevented).toBe(false);
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(prRow.getAttribute("aria-current")).toBe("true");
  expect(core.openPullRequest).toHaveBeenCalledOnce();
  expect(core.openIssue).not.toHaveBeenCalled();
});

it("leaves Esc alone with an Issue on show", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await press("Escape");

  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
});

it("switches the PR view's tabs with Ctrl+PageDown and Ctrl+PageUp", async () => {
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });
  const pageDown = keydown({ key: "PageDown", ctrlKey: true });

  await fireEvent(document.body, pageDown);
  expect(files.getAttribute("aria-selected")).toBe("true");
  expect(pageDown.defaultPrevented).toBe(true);
  await press("PageUp", { ctrlKey: true });
  expect(files.getAttribute("aria-selected")).toBe("false");
});

it("switches the Files tab between unified and side by side with Ctrl+Alt+U, for each PR after", async () => {
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { prRow, treeRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });
  const toggle = () => keydown({ key: "u", ctrlKey: true, altKey: true });
  // The Conversation tab has no diff to switch.
  const onConversation = toggle();
  await fireEvent(document.body, onConversation);
  await fireEvent.click(files);
  expect(await screen.findByRole("region", { name: "Diff" })).toBeTruthy();

  const onFiles = toggle();
  await fireEvent(document.body, onFiles);
  expect(await screen.findByRole("region", { name: "Side-by-side diff" })).toBeTruthy();
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));

  expect(await screen.findByRole("region", { name: "Side-by-side diff" })).toBeTruthy();
  expect([onConversation.defaultPrevented, onFiles.defaultPrevented]).toEqual([false, true]);
});

it("moves through a PR's open threads with F8 and Shift+F8 on its Files tab", async () => {
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const next = vi.spyOn(PrFilesState.prototype, "nextThread");
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });
  // The Conversation tab has no threads to move through.
  await press("F8");
  await fireEvent.click(files);

  await press("F8");
  await press("F8", { shiftKey: true });

  expect(next.mock.calls).toEqual([[1], [-1]]);
});

/** Opens the Files tab of the Standalone PR #5, whose diff is empty, and returns the tab. */
const onFiles = async () => {
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  return screen.findByRole("tab", { name: "Files" });
};

it("moves through a PR's changes with F7 and Shift+F7 on its Files tab", async () => {
  const next = vi.spyOn(PrFilesState.prototype, "nextChange");
  const files = await onFiles();
  // The Conversation tab has no changes to move through.
  await press("F7");
  await fireEvent.click(files);

  await press("F7");
  await press("F7", { shiftKey: true });

  expect(next.mock.calls).toEqual([[1], [-1]]);
});

it("names the views whose keys work where a press goes, for the `?` list", async () => {
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { component, treeRow } = await listing();
  const views = (target: EventTarget | null) => component.keysHere(keyTarget(target, component.paneOf)).views;
  const onList = [views(document.body), views(treeRow)];
  await fireEvent.click(treeRow);
  const detail = document.querySelector('[data-pane="detail"]');
  // The Conversation tab has no keys of its own.
  const onConversation = views(detail);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));
  await screen.findByRole("region", { name: "Diff" });

  expect([...onList, onConversation]).toEqual([["issue_list"], ["issue_list"], []]);
  // The diff's keys work from the list too, while the Files tab shows, every one of them.
  expect([views(detail), views(treeRow)]).toEqual([["diff"], ["issue_list", "diff"]]);
  const here = component.keysHere(keyTarget(detail, component.paneOf));
  expect(keysIn("diff").every((row) => here.works(row.action))).toBe(true);
});

it("opens what quick open found on its line, unfolded, which takes the focus, or one the list hasn't", async () => {
  core.workbench.mockResolvedValue(
    workbenchPage([
      { ...issue, attention: null },
      { ...standalone, attention: null },
    ]),
  );
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });

  component.openFound("pull_request", 160662);
  await tick();
  const underIssue = screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(underIssue);
  });
  const tab = () => screen.getByRole("tab", { selected: true }).textContent;
  await screen.findByRole("tab", { name: "Files" });
  const opened = tab();
  // Picking the PR on show leaves it on its tab, where a click on its row would switch.
  component.openFound("pull_request", 160662);
  await tick();
  expect([opened, tab()]).toEqual(["Conversation", "Conversation"]);
  component.openFound("pull_request", 5);
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "#5 Alone" }));
  });
  const alone = document.activeElement;
  component.openFound("issue", 99);
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();

  expect(document.activeElement).toBe(alone);
  expect(core.openPullRequest.mock.calls).toEqual([
    ["python/cpython", 160662],
    ["python/cpython", 5],
  ]);
  expect(core.openIssue).toHaveBeenCalledWith("python/cpython", 99);
});

it("lists for `?` only the keys that would run: the PR view's while a PR shows, a text box's in one", async () => {
  const { component, issueRow, treeRow } = await listing();
  const box = document.createElement("input");
  document.body.append(box);
  /** Whether each key that works anywhere would run for a press on `target`. */
  const works = (target: EventTarget | null) => {
    const here = component.keysHere(keyTarget(target, component.paneOf));
    return Object.fromEntries(keysIn("anywhere").map((row) => [row.action, here.works(row.action)]));
  };
  const inBox = works(box);
  box.remove();
  await fireEvent.click(issueRow);
  const onIssue = works(document.body);
  await fireEvent.click(treeRow);
  await screen.findByRole("tab", { name: "Files" });

  // The page decides on its own actions, such as Command Palette.
  expect(inBox).toMatchObject({ toggle_sidebar: true, toggle_detail_only: false, command_palette: true });
  expect(onIssue).toMatchObject({ next_tab: false, toggle_rail_files: false, toggle_detail_only: true });
  expect(works(document.body)).toMatchObject({ next_tab: true, toggle_rail_files: true });
});

it("moves between a PR's files with Alt+PageDown and Alt+PageUp on its Files tab", async () => {
  const next = vi.spyOn(PrFilesState.prototype, "nextFile");
  const files = await onFiles();
  // The Conversation tab has no files to move through.
  await press("PageDown", { altKey: true });
  await fireEvent.click(files);

  await press("PageDown", { altKey: true });
  await press("PageUp", { altKey: true });

  expect(next.mock.calls).toEqual([[1], [-1]]);
});

it("opens a PR's Files tab one file at a time as the settings say, and switches it with Ctrl+K O and the palette", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { component, treeRow } = await listing({ diffFiles: "one" });
  await fireEvent.click(treeRow);
  const tab = await screen.findByRole("tab", { name: "Files" });
  const command = () => paletteRow(component, "toggle_files_mode");
  // The Conversation tab shows no files to switch.
  component.runChord("toggle_files_mode", chordEnd());
  expect(command()).toBeUndefined();
  await fireEvent.click(tab);
  // The tab reads the diff the first time it shows.
  await screen.findByRole("region", { name: "Diff" });
  expect(pressed()).toEqual(["false", "true"]);

  expect(command()?.title).toBe("View: All Files");
  command()?.run();
  await tick();
  expect([pressed(), command()?.title]).toEqual([["true", "false"], "View: One File at a Time"]);
  component.runChord("toggle_files_mode", chordEnd());
  await tick();
  expect(pressed()).toEqual(["false", "true"]);
});

it("opens the rail with Ctrl+K F and the palette while a PR shows, and closes it on either", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { component, treeRow } = await listing();
  const command = () => paletteRow(component, "toggle_rail_files");
  const rail = () => screen.queryByRole("complementary", { name: "Rail" });
  // With no PR on show there is no rail to open.
  component.runChord("toggle_rail_files", chordEnd());
  const before = command();
  await fireEvent.click(treeRow);
  await screen.findByRole("tab", { name: "Conversation" });
  const closed = rail();

  component.runChord("toggle_rail_files", chordEnd());
  await tick();
  const opened = rail() !== null;
  command()?.run();
  await tick();

  expect([before, closed, opened, rail(), command()?.title]).toEqual([
    undefined,
    null,
    true,
    null,
    "View: Files in the Rail",
  ]);
});

it("opens the rail on Search with Ctrl+Shift+F and the palette, puts the selection or the picked word in its box, and focuses it", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  core.textSearch.mockResolvedValue({ status: "ok", data: { kind: "running", id: 1 } });
  core.textSearchState.mockResolvedValue({ state: "results", id: 1, files: [], matches: 0, end: "done" });
  const selection = vi.spyOn(window, "getSelection");
  const selected = (text: string) => selection.mockReturnValue({ toString: () => text } as Selection);
  const picked = vi.spyOn(PrFilesState.prototype, "picked", "get");
  const inView = vi.spyOn(PrFilesState.prototype, "fileView", "get");
  const { component, treeRow } = await listing();
  const command = () => paletteRow(component, "search_in_rail");
  const searchKey = () => press("F", { ctrlKey: true, shiftKey: true });
  const box = () => screen.getByRole("textbox", { name: "Search the PR's code" }) as HTMLInputElement;
  const railTab = () =>
    within(screen.getByRole("complementary", { name: "Rail" })).getByRole("tab", { selected: true })
      .textContent;
  // With no PR on show there is no rail to open.
  await searchKey();
  const before = [command(), screen.queryByRole("complementary", { name: "Rail" })];
  await fireEvent.click(treeRow);
  await screen.findByRole("tab", { name: "Conversation" });

  selected("gc_debug");
  await searchKey();
  await tick();
  const fromSelection = [railTab(), box().value, document.activeElement === box()];
  // Again with Search on show, a selection on one line outside the box goes in, as in VS Code, and the
  // rail stays open.
  box().blur();
  selected("gc_collect");
  await searchKey();
  await tick();
  const again = [railTab(), box().value, document.activeElement === box()];
  await vi.waitFor(() => {
    expect(core.textSearch.mock.lastCall?.[0]).toMatchObject({ query: { text: "gc_collect" } });
  });
  // The same selection again runs nothing again, which would lose the folds and the cursor.
  const runs = core.textSearch.mock.calls.length;
  box().blur();
  await searchKey();
  await tick();
  const rerun = core.textSearch.mock.calls.length - runs;
  // While the box has the focus, the selection is the box's own text, which stays as the user typed it.
  await fireEvent.input(box(), { target: { value: "gc_.*" } });
  box().focus();
  selected("gc_");
  await fireEvent.keyDown(box(), { key: "F", ctrlKey: true, shiftKey: true });
  await tick();
  const typed = box().value;
  const railOnFiles = () => paletteRow(component, "toggle_rail_files")?.run();
  // On the Files tab the word picked in the diff goes in, as a selection over two lines puts nothing in
  // the box, but only while Search isn't on show: then the box keeps its query.
  railOnFiles();
  box().blur();
  selected("two\nlines");
  await fireEvent.click(document.getElementById("pr-tab-files") as HTMLElement);
  picked.mockReturnValue({ word: "gc_list" } as never);
  command()?.run();
  await tick();
  const fromDiff = box().value;
  picked.mockReturnValue({ word: "gc_freeze" } as never);
  command()?.run();
  await tick();
  const kept = box().value;
  // The word picked in the file view over the diff goes in too. A window with no selection at all, as one
  // with no browsing context, has none to give.
  railOnFiles();
  box().blur();
  selection.mockReturnValue(null);
  inView.mockReturnValue({ code: { pick: { word: "visit" } } } as never);
  command()?.run();
  await tick();

  expect(before).toEqual([undefined, null]);
  expect(fromSelection).toEqual(["Search", "gc_debug", true]);
  expect([again, rerun, typed]).toEqual([["Search", "gc_collect", true], 0, "gc_.*"]);
  expect([fromDiff, kept, box().value, command()?.title]).toEqual([
    "gc_list",
    "gc_list",
    "visit",
    "View: Search in the Rail",
  ]);
  await vi.waitFor(() => {
    expect(core.textSearch).toHaveBeenCalledWith({
      repository: "python/cpython",
      base: "base",
      head: "abc",
      query: { text: "visit", case: "ignored", words: "within", syntax: "literal" },
    });
  });
});

it("offers no rail for a PR Tsugiki doesn't hold, which has no tabs to sit beside", async () => {
  const changed = catchEveryListener();
  core.pullRequest.mockResolvedValue(ok(null));
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByText("Tsugiki doesn't hold PR #160662.");
  const offered = paletteRow(component, "toggle_rail_files") !== undefined;

  component.runChord("toggle_rail_files", chordEnd());
  core.pullRequest.mockResolvedValue({ status: "ok", data: { ...row, content: null } });
  changed("python/cpython", [160662]);
  await screen.findByRole("tab", { name: "Conversation" });

  expect(offered).toBe(false);

  expect(screen.queryByRole("complementary", { name: "Rail" })).toBeNull();
  expect(paletteRow(component, "toggle_rail_files")?.title).toBe("View: Files in the Rail");
});

it("keeps the PR on show on Esc in the rail, gives the diff the focus, and gives it the focus as Ctrl+K F closes the rail from its edge", async () => {
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));
  const diff = await screen.findByRole("region", { name: "Diff" });
  component.runChord("toggle_rail_files", chordEnd());
  await tick();
  const close = () => screen.getByRole("button", { name: "Close the rail" });
  close().focus();

  await fireEvent.keyDown(close(), { key: "Escape" });
  const left = [document.activeElement === diff, screen.queryByRole("region", { name: "PR view" }) !== null];
  // The rail's edge takes the focus too, and hands it on as the rail goes.
  screen.getByRole("separator", { name: "Rail width" }).focus();
  component.runChord("toggle_rail_files", chordEnd());

  expect(left).toEqual([true, true]);
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(diff);
  });
  expect(screen.queryByRole("complementary", { name: "Rail" })).toBeNull();
});

it("unfolds the files unchanged since a Stale review with Ctrl+K J and the palette, on the Files tab", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const unfold = vi.spyOn(PrFilesState.prototype, "unfoldUnchanged").mockImplementation(() => {});
  const folded = vi.spyOn(PrFilesState.prototype, "foldedUnchanged", "get").mockReturnValue(2);
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  const tab = await screen.findByRole("tab", { name: "Files" });
  const command = () => paletteRow(component, "unfold_unchanged");
  // The Conversation tab shows no files to unfold.
  component.runChord("unfold_unchanged", chordEnd());
  expect(command()).toBeUndefined();
  await fireEvent.click(tab);
  await screen.findByRole("region", { name: "Diff" });

  expect(command()?.title).toBe("View: Unfold Unchanged Files");
  command()?.run();
  component.runChord("unfold_unchanged", chordEnd());
  expect(unfold).toHaveBeenCalledTimes(2);
  // Once no unchanged file is folded, the palette has nothing to offer.
  folded.mockReturnValue(0);
  expect(command()).toBeUndefined();
});

it("opens the search in the diff with Ctrl+F on the Files tab, and gives its box focus again from it", async () => {
  const files = await onFiles();
  await press("f", { ctrlKey: true });
  expect(screen.queryByRole("search")).toBeNull();
  await fireEvent.click(files);
  // The tab reads the diff the first time it shows.
  await screen.findByRole("region", { name: "Diff" });

  await press("f", { ctrlKey: true });
  const box = screen.getByRole<HTMLInputElement>("textbox", { name: "Find" });
  await fireEvent.input(box, { target: { value: "gc" } });
  await fireEvent.keyDown(box, { key: "f", ctrlKey: true });

  expect(screen.getByRole("search", { name: "Find in the diff" })).toBeTruthy();
  expect(document.activeElement).toBe(box);
  expect([box.selectionStart, box.selectionEnd]).toEqual([0, 2]);
});

it("closes the search with Escape from anywhere, then leaves the PR with the focus on the Issue's row", async () => {
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  await press("f", { ctrlKey: true });
  diff.focus();

  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(screen.queryByRole("search")).toBeNull();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: /^#112564 / }));
});

it("opens the Definition peek with Alt+F12 on the Files tab, once for a held key", async () => {
  const peekDefinition = vi.spyOn(PrFilesState.prototype, "peekDefinition").mockResolvedValue();
  const files = await onFiles();
  await press("F12", { altKey: true });
  expect(peekDefinition).not.toHaveBeenCalled();
  await fireEvent.click(files);
  await screen.findByRole("region", { name: "Diff" });

  await press("F12", { altKey: true });
  await press("F12", { altKey: true, repeat: true });

  expect(peekDefinition).toHaveBeenCalledTimes(1);
});

it("opens the Definition peek where [diff] peek says, below the file view's code when split", async () => {
  // Each lookup waits, so the peek says it looks.
  core.definitions.mockReturnValue(new Promise(() => {}));
  const view = eggsView();
  vi.spyOn(PrFilesState.prototype, "goToDefinition").mockImplementation(function (this: PrFilesState) {
    this.openFileView(view);
    return Promise.resolve();
  });
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { treeRow } = await listing({ diffPeek: "split" });
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));
  await screen.findByRole("region", { name: "Diff" });
  await press("F12");
  const fileView = await screen.findByRole("region", { name: "File view" });
  const code = within(fileView).getByRole("region", { name: "Definition's code" });
  view.code.pick = { line: 4, word: "x", start: 0, end: 1, caret: 0 };
  const peek = () => within(fileView).getByRole("group", { name: "Definition peek" });

  await press("F12", { altKey: true });

  // Below the view's code, and not inside it.
  expect(code.contains(peek())).toBe(false);
  expect(peek().textContent).toContain("Searching for the definition of x…");
});

it("goes to the definition with F12 on the Files tab, once for a held key", async () => {
  const goToDefinition = vi.spyOn(PrFilesState.prototype, "goToDefinition").mockResolvedValue();
  const files = await onFiles();
  await press("F12");
  expect(goToDefinition).not.toHaveBeenCalled();
  await fireEvent.click(files);
  await screen.findByRole("region", { name: "Diff" });

  await press("F12");
  await press("F12", { repeat: true });

  expect(goToDefinition).toHaveBeenCalledTimes(1);
});

/** A file view of `path` at the PR's base, whose code holds 10 lines it asks the core for none of. */
const eggsView = (path = "Lib/eggs.py") => {
  const place = { kind: "repository" as const, path, side: "base" as const, commit: "base" };
  const code = {
    count: 10,
    widest: 0,
    show: () => {},
    line: () => "x = 1",
    spans: () => undefined,
    retry: () => {},
  } as unknown as HitCode;
  return new FileView(place, 4, code);
};

/** A PR's Files tab with the file view over it. */
const onFileView = async () => {
  vi.spyOn(PrFilesState.prototype, "fileView", "get").mockReturnValue(eggsView());
  const files = await onFiles();
  await fireEvent.click(files);
  return screen.findByRole("region", { name: "File view" });
};

/** A PR's Files tab, from the tree of Issue #112564, where F12 opens the file view as Go to definition does
 * into a file the PR doesn't change. Returns the diff, the view, and the Files tab's state once it opens. */
const goIntoFileView = async () => {
  const view = eggsView();
  const opened: { files?: PrFilesState } = {};
  vi.spyOn(PrFilesState.prototype, "goToDefinition").mockImplementation(function (this: PrFilesState) {
    opened.files = this;
    this.openFileView(view);
    return Promise.resolve();
  });
  await fireEvent.click(await onFiles());
  const diff = await screen.findByRole("region", { name: "Diff" });
  await press("F12");
  await screen.findByRole("region", { name: "File view" });
  return { diff, view, opened };
};

/** Waits for the page's updates and what waits on them, such as a focus that moves once the page is drawn. */
const settled = async () => {
  await tick();
  await tick();
};

/** The tooltip of the history's arrow back, or forward for `way` 1. */
const arrowTitle = (way: -1 | 1 = -1) =>
  within(screen.getByRole("navigation", { name: "History" }))
    .getByRole("button", { name: way < 0 ? "Back" : "Forward" })
    .getAttribute("title");

it("closes the file view over the PR on show when quick open picks that PR, on the same tab, and records it", async () => {
  const view = eggsView();
  vi.spyOn(PrFilesState.prototype, "goToDefinition").mockImplementation(function (this: PrFilesState) {
    this.openFileView(view);
    return Promise.resolve();
  });
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));
  await screen.findByRole("region", { name: "Diff" });
  await press("F12");
  await screen.findByRole("region", { name: "File view" });

  component.openFound("pull_request", 160662);
  await settled();

  expect(screen.queryByRole("region", { name: "File view" })).toBeNull();
  expect(screen.getByRole("tab", { selected: true }).textContent).toBe("Files");
  expect(document.activeElement).toBe(treeRow);
  // The pick is the history's latest place, so Back goes to the file view it closed.
  expect(arrowTitle()).toMatch(/^Back to Lib\/eggs\.py /);
});

it("hands quick open the files of the PR on show, read only when asked, and none once it leaves", async () => {
  core.pullRequestFiles.mockResolvedValue({ status: "ok", data: { state: "fetching" } });
  vi.spyOn(PrFilesState.prototype, "paths", "get").mockReturnValue(["Lib/gc.py"]);
  const opened = vi.spyOn(PrViewState.prototype, "openFile").mockReturnValue();
  const loads = vi.spyOn(PrViewState.prototype, "loadFiles");
  const { component, prRow, treeRow } = await listing();
  const onIssueList = component.changedFiles();
  await fireEvent.click(prRow);
  await screen.findByRole("tab", { name: "Files" });

  const files = component.changedFiles();
  const unread = core.pullRequestFiles.mock.calls.length;
  files?.load();
  await vi.waitFor(() => {
    expect(core.pullRequestFiles).toHaveBeenCalledOnce();
  });
  files?.open("Lib/gc.py");
  const whileShown = files?.paths();
  await fireEvent.click(treeRow);
  files?.load();
  files?.open("Lib/gc.py");

  expect([onIssueList, unread, whileShown, files?.paths()]).toEqual([undefined, 0, ["Lib/gc.py"], []]);
  expect([opened.mock.calls, loads.mock.calls.length]).toEqual([[["Lib/gc.py"]], 1]);
});

it("goes back to the diff with Alt+← from the file view, and forward to the file view, each as it was", async () => {
  const { diff, view, opened } = await goIntoFileView();
  const row = document.querySelector('[data-item="112564#160662"]');
  expect(arrowTitle()).toMatch(/^Back to #160662 /);

  await press("ArrowLeft", { altKey: true });

  expect(screen.queryByRole("region", { name: "File view" })).toBeNull();
  // The same diff, never drawn again, so it keeps where the user scrolled it.
  expect(screen.getByRole("region", { name: "Diff" })).toBe(diff);
  expect(row?.getAttribute("aria-current")).toBe("true");
  expect(arrowTitle(1)).toMatch(/^Forward to Lib\/eggs\.py · #160662 /);

  await press("ArrowRight", { altKey: true });

  expect(await screen.findByRole("region", { name: "File view" })).toBeTruthy();
  expect(opened.files?.fileView).toBe(view);
  expect(row?.getAttribute("aria-current")).toBe("true");
  // Coming back to the file view is no new visit, which would push a second one.
  expect([arrowTitle(), arrowTitle(1)]).toEqual([
    expect.stringMatching(/^Back to #160662 /),
    "Forward (Alt+→): nothing there",
  ]);
  await settled();
  expect(document.activeElement).toBe(screen.getByRole("region", { name: "Definition's code" }));
});

it("closes the file view on a click on its PR's row, which switches tabs, and Alt+← brings it back", async () => {
  await goIntoFileView();
  const files = screen.getByRole("tab", { name: "Files" });

  await fireEvent.click(
    screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  expect(files.getAttribute("aria-selected")).toBe("false");
  await press("PageDown", { ctrlKey: true });

  expect(files.getAttribute("aria-selected")).toBe("true");
  expect(screen.queryByRole("region", { name: "File view" })).toBeNull();
  expect(arrowTitle()).toMatch(/^Back to Lib\/eggs\.py · #160662 /);
  await press("ArrowLeft", { altKey: true });
  expect(await screen.findByRole("region", { name: "File view" })).toBeTruthy();
});

it("goes back to the diff on Esc in the file view, and on to the Issue on the next Esc", async () => {
  const { diff } = await goIntoFileView();
  diff.blur();

  await press("Escape");

  expect(screen.queryByRole("region", { name: "File view" })).toBeNull();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(diff);
  });
  // Esc is a step of its own, so Alt+← goes back to the file view it left.
  expect(arrowTitle()).toMatch(/^Back to Lib\/eggs\.py · #160662 /);

  await press("Escape");

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
});

it("shows the file view again over its PR's Files tab when Alt+← comes back to it from another item", async () => {
  await goIntoFileView();
  const treeRow = document.querySelector('[data-item="112564#160662"]');
  await fireEvent.click(screen.getByRole("button", { name: "#112564 GIL drop during collection" }));
  await screen.findByRole("region", { name: "Issue view" });
  expect(arrowTitle()).toMatch(/^Back to Lib\/eggs\.py · #160662 /);

  await press("ArrowLeft", { altKey: true });

  expect(await screen.findByRole("region", { name: "File view" })).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("true");
  expect(treeRow?.getAttribute("aria-current")).toBe("true");
  await settled();
  expect(document.activeElement).toBe(screen.getByRole("region", { name: "Definition's code" }));
});

it("leaves the diff's keys alone while the file view covers it, and the cursor on the PR's row", async () => {
  const nextChange = vi.spyOn(PrFilesState.prototype, "nextChange");
  await onFileView();
  const row = document.querySelector('[data-item="112564#160662"]');

  await press("F7");

  expect(nextChange).not.toHaveBeenCalled();
  expect(row?.getAttribute("aria-current")).toBe("true");
});

it("leaves the diff's lookups alone of its keys for `?` while the file view covers it", async () => {
  vi.spyOn(PrFilesState.prototype, "fileView", "get").mockReturnValue(eggsView());
  core.pullRequestFiles.mockResolvedValue(ok(readyFiles(0)));
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));
  const here = component.keysHere(
    keyTarget(await screen.findByRole("region", { name: "File view" }), component.paneOf),
  );
  const works = Object.fromEntries(keysIn("diff").map((row) => [row.action, here.works(row.action)]));

  expect(here.views).toEqual(["diff"]);
  expect(works).toMatchObject({
    go_to_definition: true,
    peek_definition: true,
    next_change: false,
    find_in_diff: false,
  });
  expect(keysIn("anywhere").every((row) => here.works(row.action))).toBe(true);
});

it("adds a history entry for a file view F12 opens from another, and Alt+← comes back to the first", async () => {
  const { diff, view, opened } = await goIntoFileView();
  const other = eggsView("Lib/spam.py");
  vi.mocked(PrFilesState.prototype.goToDefinition).mockImplementation(function (this: PrFilesState) {
    this.openFileView(other);
    return Promise.resolve();
  });

  await press("F12");
  const second = [opened.files?.fileView, arrowTitle()];
  await press("ArrowLeft", { altKey: true });
  const backToFirst = [opened.files?.fileView, arrowTitle(1)];
  await press("ArrowLeft", { altKey: true });

  expect(second).toEqual([other, expect.stringMatching(/^Back to Lib\/eggs\.py · #160662 /)]);
  expect(backToFirst).toEqual([view, expect.stringMatching(/^Forward to Lib\/spam\.py · #160662 /)]);
  expect(screen.queryByRole("region", { name: "File view" })).toBeNull();
  expect(screen.getByRole("region", { name: "Diff" })).toBe(diff);
});

it("closes the Definition peek in the file view with Escape, and the file view with the next", async () => {
  const view = eggsView();
  const shown: { peek: Peek | undefined } = {
    peek: {
      at: { kind: "view", view, line: 4 },
      layout: "inline",
      symbol: "x",
      side: "base",
      commit: "base",
      lookup: { state: "looking" },
    },
  };
  vi.spyOn(PrFilesState.prototype, "fileView", "get").mockReturnValue(view);
  vi.spyOn(PrFilesState.prototype, "peek", "get").mockImplementation(() => shown.peek);
  const closePeek = vi.spyOn(PrFilesState.prototype, "closePeek").mockImplementation(() => {
    shown.peek = undefined;
  });
  const closeFileView = vi.spyOn(PrFilesState.prototype, "closeFileView").mockImplementation(() => {});
  await fireEvent.click(await onFiles());
  await screen.findByRole("region", { name: "File view" });

  await press("Escape");
  const first = [closePeek.mock.calls.length, closeFileView.mock.calls.length];
  await press("Escape");

  expect(first).toEqual([1, 0]);
  expect([closePeek.mock.calls.length, closeFileView.mock.calls.length]).toEqual([1, 1]);
});

it("goes to the definition with F12 and opens the peek with Alt+F12 from the file view, once for a held key", async () => {
  const goToDefinition = vi.spyOn(PrFilesState.prototype, "goToDefinition").mockResolvedValue();
  const peekDefinition = vi.spyOn(PrFilesState.prototype, "peekDefinition").mockResolvedValue();
  await onFileView();

  await press("F12");
  await press("F12", { repeat: true });
  await press("F12", { altKey: true });
  await press("F12", { altKey: true, repeat: true });

  expect(goToDefinition).toHaveBeenCalledTimes(1);
  expect(peekDefinition).toHaveBeenCalledTimes(1);
});

it("closes the Definition peek with Escape before the search, and the search before the PR", async () => {
  const shown: { peek: Peek | undefined } = {
    peek: {
      at: { kind: "row", row: 2 },
      layout: "inline",
      symbol: "gc",
      side: "head",
      commit: "abc",
      lookup: { state: "looking" },
    },
  };
  vi.spyOn(PrFilesState.prototype, "peek", "get").mockImplementation(() => shown.peek);
  const closePeek = vi.spyOn(PrFilesState.prototype, "closePeek").mockImplementation(() => {
    shown.peek = undefined;
  });
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  await press("f", { ctrlKey: true });
  diff.focus();

  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(closePeek).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("search")).toBeTruthy();
  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(screen.queryByRole("search")).toBeNull();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(closePeek).toHaveBeenCalledTimes(1);
});

it("stops Go to definition with Escape while it searches, before the PR closes", async () => {
  const closePeek = vi.spyOn(PrFilesState.prototype, "closePeek").mockImplementation(() => {});
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  vi.spyOn(PrFilesState.prototype, "going", "get").mockReturnValue(true);

  await fireEvent.keyDown(diff, { key: "Escape" });

  expect(closePeek).toHaveBeenCalledOnce();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
});

it("leaves the focus where it is when the Files tab is drawn again with the search open", async () => {
  const files = await onFiles();
  await fireEvent.click(files);
  await screen.findByRole("region", { name: "Diff" });
  await press("f", { ctrlKey: true });
  const conversation = screen.getByRole("tab", { name: "Conversation" });
  await fireEvent.click(conversation);
  conversation.focus();

  await fireEvent.click(files);

  expect(screen.getByRole("search")).toBeTruthy();
  expect(document.activeElement).toBe(conversation);
});

it("goes back and forward through the Issues and PRs it opened with Alt+← and Alt+→", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });

  await press("ArrowLeft", { altKey: true });
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(issueRow.getAttribute("aria-current")).toBe("true");
  await press("ArrowLeft", { altKey: true });
  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
  await press("ArrowRight", { altKey: true });
  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
  await press("ArrowRight", { altKey: true });
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
});

it("goes back and forward with the arrows over the Detail pane, which name their target", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  const history = within(await screen.findByRole("navigation", { name: "History" }));
  const back = history.getByRole("button", { name: "Back" });
  const forward = history.getByRole("button", { name: "Forward" });
  await vi.waitFor(() => {
    expect(back.getAttribute("title")).toBe("Back to #112564 GIL drop during collection (Alt+←)");
  });
  expect(forward.hasAttribute("disabled")).toBe(true);
  expect(forward.getAttribute("title")).toBe("Forward (Alt+→): nothing there");

  await fireEvent.click(back);

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(issueRow.getAttribute("aria-current")).toBe("true");
  const now = within(screen.getByRole("navigation", { name: "History" }));
  expect(now.getByRole("button", { name: "Back" }).hasAttribute("disabled")).toBe(true);
  expect(now.getByRole("button", { name: "Forward" }).getAttribute("title")).toBe(
    "Forward to #5 PR 5 (Alt+→)",
  );
  expect(now.getByText("#112564 GIL drop during collection")).toBeTruthy();
});

it("lists the recent items back on a right-click, and opens the one picked with the cursor on its row", async () => {
  const { issueRow, prRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  const history = within(screen.getByRole("navigation", { name: "History" }));

  await fireEvent.contextMenu(history.getByRole("button", { name: "Back" }));
  const menu = within(await screen.findByRole("menu", { name: "Recent items back" }));
  const items = menu.getAllByRole("menuitem").map((item) => item.textContent);
  expect(items).toHaveLength(2);
  expect(items[1]).toBe("#112564 GIL drop during collection");

  await fireEvent.click(menu.getByText("#112564 GIL drop during collection"));

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(issueRow.getAttribute("aria-current")).toBe("true");
  expect(screen.queryByRole("menu")).toBeNull();
});

it("goes back and forward on the mouse's own buttons, and keeps the page where it is", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });

  const pressed = new MouseEvent("mousedown", { button: 3, cancelable: true });
  window.dispatchEvent(pressed);
  const released = new MouseEvent("mouseup", { button: 3, cancelable: true });
  window.dispatchEvent(released);

  expect([pressed.defaultPrevented, released.defaultPrevented]).toEqual([true, true]);
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  await fireEvent.mouseUp(window, { button: 4 });
  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
  const other = new MouseEvent("mouseup", { button: 0, cancelable: true });
  window.dispatchEvent(other);
  expect(other.defaultPrevented).toBe(false);
});

it("leaves the item on show as it is when a pick from the history's list lands on it", async () => {
  const { issueRow, prRow } = await listing();
  for (const row of [issueRow, prRow, issueRow, prRow]) await fireEvent.click(row);
  await screen.findByRole("region", { name: "PR view" });
  /** Picks the second item of an arrow's list, two steps away. */
  const pick = async (way: "Back" | "Forward", title: string) => {
    const history = within(screen.getByRole("navigation", { name: "History" }));
    await fireEvent.contextMenu(history.getByRole("button", { name: way }));
    const items = within(await screen.findByRole("menu")).getAllByRole("menuitem");
    expect(items[1]?.textContent).toBe(title);
    await fireEvent.click(items[1] ?? document.body);
  };
  await vi.waitFor(() => {
    expect(screen.getByRole("navigation", { name: "History" }).textContent).toContain("#5 PR 5");
  });
  const readings = () => [core.openIssue.mock.calls.length, core.openPullRequest.mock.calls.length];
  const before = readings();

  await pick("Back", "#5 PR 5");
  expect(screen.getByRole("tab", { name: "Conversation" }).getAttribute("aria-selected")).toBe("true");
  expect(document.activeElement).toBe(prRow);
  await press("ArrowLeft", { altKey: true });
  await screen.findByRole("region", { name: "Issue view" });
  const back = readings();
  await pick("Forward", "#112564 GIL drop during collection");

  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
  expect([before, readings()]).toEqual([before, back]);
});

it("leaves the mouse's own buttons to a dialog on top, and still keeps the page where it is", async () => {
  core.workbench.mockResolvedValue(workbenchPage([{ ...issue, attention: null }]));
  render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
    keysActive: false,
  });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await unfoldAll();
  await fireEvent.click(
    screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  await screen.findByRole("region", { name: "PR view" });

  const released = new MouseEvent("mouseup", { button: 3, cancelable: true });
  window.dispatchEvent(released);

  expect(released.defaultPrevented).toBe(true);
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
});

it("offers the palette Go Back and Go Forward, each with its key", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { component, issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  const command = (action: Action) => paletteRow(component, action);

  expect([command("go_back")?.title, command("go_forward")?.title]).toEqual(["Go Back", "Go Forward"]);
  command("go_back")?.run();
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  command("go_forward")?.run();
  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
});

it("moves the focus with the cursor on Alt+← and Alt+→", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  treeRow.focus();

  await fireEvent.keyDown(treeRow, { key: "ArrowLeft", altKey: true });
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
  await fireEvent.keyDown(issueRow, { key: "ArrowRight", altKey: true });

  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
  expect(document.activeElement).toBe(treeRow);
});

/** The PR view's item as the core reads it after the list did: newer activity, so another mark. */
const laterMark = { numbers: [160662], up_to: "2026-11-05T10:00:00Z" };

it("marks the line under the cursor on Q and Shift+Q, and the Detail pane's item when it holds the focus", async () => {
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...row, seen: { mark: laterMark, state: "new_activity" }, content: null },
  });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);
  const view = await screen.findByRole("region", { name: "PR view" });
  await screen.findByRole("button", { name: "Mark Seen" });

  await fireEvent.keyDown(issueRow, { key: "q" });
  await fireEvent.keyDown(view, { key: "q" });
  await fireEvent.keyDown(issueRow, { key: "Q", shiftKey: true });
  await fireEvent.keyDown(issueRow, { key: "Q", shiftKey: true, repeat: true });

  expect(core.markSeen.mock.calls).toEqual([
    ["python/cpython", row.seen.mark],
    ["python/cpython", laterMark],
  ]);
  expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662, null);
});

/** A window on three Standalone PRs that Need attention, #5, #6 and #8, the cursor on #5 and #5 on show.
 * `markedSeen` answers as the core does once it saved Mark Seen on #5, or once a sync took #5's attention,
 * which drops it below the others, and says that the change names `items`. */
const threeAlone = async () => {
  const send = catchEveryListener();
  const alone = (number: number, title: string) => ({
    ...standalone,
    number,
    title,
    attention: { since: "2026-11-04T10:00:00Z", why: [{ kind: "commits", by: "octocat" }] },
    seen: { mark: { numbers: [number], up_to: "2026-11-04T10:00:00Z" }, state: "new_activity" },
  });
  const [first, next, last] = [alone(5, "Alone"), alone(6, "Next"), alone(8, "Last")];
  core.pullRequest.mockImplementation((_repository: string, number: number) =>
    Promise.resolve({
      status: "ok",
      data: { ...row, ...alone(number, `PR ${String(number)}`), content: null },
    }),
  );
  const answer = (items: object[]) => workbenchPage(items, { count: items.length });
  core.workbench.mockResolvedValue(answer([first, next, last]));
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  const marked = await screen.findByRole("button", { name: "#5 Alone" });
  await fireEvent.click(marked);
  const markedSeen = (items = [5]) => {
    core.workbench.mockResolvedValue(answer([next, last, { ...first, attention: null }]));
    send("python/cpython", items);
  };
  return { component, marked, markedSeen };
};

it("moves the cursor and the focus to the next row once Q drops the row under the cursor down the list", async () => {
  const { marked, markedSeen } = await threeAlone();
  marked.focus();

  await fireEvent.keyDown(marked, { key: "q" });
  markedSeen();

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(landed);
  });
  expect(landed.getAttribute("aria-current")).toBe("true");
});

it("moves the focus to the next row after Q on the page, where nothing held the focus", async () => {
  const { markedSeen } = await threeAlone();
  (document.activeElement as HTMLElement | null)?.blur();

  await press("q");
  await vi.waitFor(() => {
    expect(core.markSeen).toHaveBeenCalledOnce();
  });
  markedSeen();

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(landed);
  });
});

it("leaves the focus in a text box clicked between Q on a row and the core's answer", async () => {
  const { marked, markedSeen } = await threeAlone();
  marked.focus();
  const box = document.createElement("input");
  document.body.append(box);

  await fireEvent.keyDown(marked, { key: "q" });
  await vi.waitFor(() => {
    expect(core.markSeen).toHaveBeenCalledOnce();
  });
  box.focus();
  markedSeen();

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  await tick();
  expect(document.activeElement).toBe(box);
  box.remove();
});

it("moves the cursor, and the focus a row held, to the next row when a sync takes the row's attention", async () => {
  const { marked, markedSeen } = await threeAlone();
  marked.focus();

  markedSeen([]);

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(landed);
  });
  expect(landed.getAttribute("aria-current")).toBe("true");
});

it("moves only the cursor when a sync takes the attention of the row under it while the focus is elsewhere", async () => {
  const { markedSeen } = await threeAlone();
  const box = document.createElement("input");
  document.body.append(box);
  box.focus();

  markedSeen([]);

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  await tick();
  expect(document.activeElement).toBe(box);
  box.remove();
});

it("leaves the focus on the list's header when a sync takes the attention of the row under the cursor", async () => {
  const { markedSeen } = await threeAlone();
  const order = screen.getByRole("button", { name: /By attention/ });
  order.focus();

  markedSeen([]);

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  await tick();
  expect(document.activeElement).toBe(order);
});

it("moves only the cursor to the next row once the palette's Mark Seen from the Detail pane drops the item on show down the list", async () => {
  const { component, markedSeen } = await threeAlone();
  const before = await screen.findByRole("button", { name: "Mark Seen" });
  before.focus();

  // The palette opened from the Detail pane, so Mark Seen marks the item on show, as Q does there, and the
  // focus went back there.
  paletteRow(component, "mark_seen", document.querySelector('[data-pane="detail"]'))?.run();
  await vi.waitFor(() => {
    expect(core.markSeen).toHaveBeenCalledOnce();
  });
  markedSeen();

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  await tick();
  expect(document.activeElement).toBe(before);
});

/** Lays the list out as a browser would, which jsdom can't: a 40 px header, then each item's rows 50 px
 * tall, under a list scrolled 90 px down that shows 60 px. So the second item's row shows from the list's
 * top, and the first item's rows are above it. Gives how far the list has scrolled. Puts jsdom's layout
 * back once the test is done, whether it passed or not. */
const layOutList = () => {
  let scrolled = 90;
  const spies = [
    vi.spyOn(Element.prototype, "scrollTop", "get").mockImplementation(function (this: Element) {
      return this.matches(".list") ? scrolled : 0;
    }),
    vi.spyOn(Element.prototype, "scrollTop", "set").mockImplementation(function (this: Element, to: number) {
      if (this.matches(".list")) scrolled = to;
    }),
    vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(function (this: Element) {
      return this.matches(".list") ? 60 : 0;
    }),
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const item = this.closest(".item");
      if (item === null) return { top: 0, bottom: 60 } as DOMRect;
      const top = 40 + Array.from(item.parentElement?.children ?? []).indexOf(item) * 50 - scrolled;
      return { top, bottom: top + 50 } as DOMRect;
    }),
  ];
  onTestFinished(() => {
    for (const spy of spies) spy.mockRestore();
  });
  return () => scrolled;
};

/** Records each call to `focus` from here to the end of the test. A browser drops the focus of an element
 * the page moves, and jsdom keeps it, so the calls tell where the page gave it back. */
const spyOnFocus = () => {
  const focus = vi.spyOn(HTMLElement.prototype, "focus");
  onTestFinished(() => {
    focus.mockRestore();
  });
  return focus;
};

/** The list's two items, `#5 Alone` first once `moved`, as an update to it puts it By last update. */
const answerMoved = (moved: boolean) => {
  const items = [
    { ...issue, attention: null },
    { ...standalone, attention: null },
  ];
  answering(...(moved ? items.reverse() : items));
};

/** The headlines of the list's items, in its order. */
const itemOrder = () =>
  Array.from(document.querySelectorAll(".list .item > .row [data-item]"), (button) => button.textContent);

it("scrolls the list to the Detail pane's row once a change moves the row off the screen (#796)", async () => {
  const send = catchEveryListener();
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  const scrolled = layOutList();

  answerMoved(true);
  send("python/cpython", [5]);

  // The row's top comes to the focus line along the list's top, 2 px down.
  await vi.waitFor(() => {
    expect(scrolled()).toBe(38);
  });
  expect(document.activeElement).toBe(document.body);
});

it("gives the focus back to the row under the cursor when a change moves it off the screen, and scrolls nothing with nothing open (#796)", async () => {
  const send = catchEveryListener();
  const { prRow } = await listing();
  for (let step = 0; step < 3; step++) await press("ArrowDown");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(prRow);
  });
  const scrolled = layOutList();
  const focus = spyOnFocus();

  answerMoved(true);
  send("python/cpython", [5]);

  await vi.waitFor(() => {
    expect(focus.mock.contexts).toEqual([prRow]);
  });
  expect(focus.mock.calls).toEqual([[{ preventScroll: true }]]);
  expect(itemOrder()).toEqual(["#5 Alone", "#112564 GIL drop during collection"]);
  expect(document.activeElement).toBe(prRow);
  expect(scrolled()).toBe(90);
});

it("follows the Detail pane's PR to its line under an Issue, with the focus and the scroll (#796)", async () => {
  const send = catchEveryListener();
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  prRow.focus();
  const scrolled = layOutList();
  const focus = spyOnFocus();

  const linked = { ...standalone, kind: "pr", backports: [] };
  answering({ ...issue, attention: null, pull_requests: [...issue.pull_requests, linked] });
  send("python/cpython", [5]);

  const under = () => document.querySelector('[data-item="112564#5"]');
  await vi.waitFor(() => {
    expect(focus.mock.contexts.map((button) => (button as HTMLElement).dataset["item"])).toEqual([
      "112564#5",
    ]);
  });
  expect(under()?.getAttribute("aria-current")).toBe("true");
  expect(scrolled()).toBe(38);
});

it("leaves the list's scroll alone for a change that moves the Detail pane's item while the list is hidden or the cursor is on another row (#796)", async () => {
  const send = catchEveryListener();
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  const scrolled = layOutList();
  const changes = async (moved: boolean) => {
    const asked = core.workbench.mock.calls.length;
    answerMoved(moved);
    send("python/cpython", [5]);
    await vi.waitFor(() => {
      expect(core.workbench.mock.calls.length).toBe(asked + 1);
    });
    // The answer and the page's update take a few promises each, and all of them settle before a timer.
    await nextTask();
  };

  await toggleList();
  await changes(true);
  await toggleList();
  await changes(false);
  await press("ArrowUp");
  await changes(true);

  expect(itemOrder()).toEqual(["#5 Alone", "#112564 GIL drop during collection"]);
  expect(rowNamed("#5 Alone").getAttribute("aria-current")).toBeNull();
  expect(scrolled()).toBe(90);
});

it("leaves the list's scroll alone once a change takes the Detail pane's item off the list (#796)", async () => {
  const send = catchEveryListener();
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  const scrolled = layOutList();

  answering({ ...issue, attention: null });
  send("python/cpython", [5]);

  await vi.waitFor(() => {
    expect(itemOrder()).toEqual(["#112564 GIL drop during collection"]);
  });
  await tick();
  expect(scrolled()).toBe(90);
});

it("marks nothing on Q and Shift+Q with no line under the cursor, or with the list hidden and nothing open", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  await listing();

  await press("q");
  await press("Q", { shiftKey: true });
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });
  await screen.findByText(/Nothing open/);
  await press("q");
  await press("Q", { shiftKey: true });

  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
});

it("archives on E the item that holds the line under the cursor, and the cursor lands on the next row", async () => {
  const changed = catchEveryListener();
  const { treeRow } = await listing();
  treeRow.focus();
  await fireEvent.keyDown(treeRow, { key: "ArrowDown" });
  await fireEvent.keyDown(treeRow, { key: "ArrowUp" });

  await fireEvent.keyDown(treeRow, { key: "e" });
  await fireEvent.keyDown(treeRow, { key: "e", repeat: true });
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  changed("python/cpython", issue.seen.mark.numbers);

  expect(core.archive).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
  const landed = await screen.findByRole("button", { name: "#5 Alone" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  expect(document.activeElement).toBe(landed);
});

it("archives on E with the list hidden the item of the PR on show, and nothing once nothing is open", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });

  await press("e");
  expect(core.archive).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
  core.archive.mockClear();
  cleanup();
  await listing();
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });
  await screen.findByText(/Nothing open/);
  await press("e");

  expect(core.archive).not.toHaveBeenCalled();
});

it("archives on E the Issue on show while the Detail pane holds the focus", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  await screen.findByRole("button", { name: "Archive" });

  await fireEvent.keyDown(view, { key: "e" });

  expect(core.archive).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
});

it("archives nothing on E in the Detail pane before the Issue's view arrives", async () => {
  core.issue.mockReturnValue(new Promise(() => {}));
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });

  await fireEvent.keyDown(view, { key: "e" });

  expect(core.archive).not.toHaveBeenCalled();
});

it("archives the Issue on show from its button, which turns into Unarchive once the Issue is archived", async () => {
  const changed = catchEveryListener();
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
  expect(core.archive).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
  const shown = await core.issue.mock.results[0]?.value;
  core.issue.mockResolvedValue({ ...shown, data: { ...shown.data, hidden: "archived" } });
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  changed("python/cpython", issue.seen.mark.numbers);
  await fireEvent.click(await screen.findByRole("button", { name: "Unarchive" }));

  expect(core.unarchive).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key);
  const list = within(issueList() as HTMLElement);
  expect(list.queryByRole("button", { name: "#112564 GIL drop during collection" })).toBeNull();
});

it("says a Review request's PR on the list is Archived", async () => {
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null, hidden: "archived" }]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const item = await screen.findByRole("listitem");

  expect(within(item).getByText("Archived")).toBeTruthy();
});

it("keeps an Archived item Archived on Mark Seen, from its row and from the Issue view's button", async () => {
  const archived = { ...standalone, attention: null, hidden: "archived" };
  core.workbench.mockResolvedValue(workbenchPage([archived]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const row = await screen.findByRole("button", { name: "#5 Alone" });
  row.focus();
  await fireEvent.keyDown(row, { key: "ArrowDown" });

  await fireEvent.keyDown(row, { key: "q" });
  expect(core.keepArchived).toHaveBeenCalledExactlyOnceWith(
    "python/cpython",
    standalone.key,
    standalone.seen.mark,
  );
  core.keepArchived.mockClear();
  cleanup();
  const shown = (await core.issue()) as { data: object };
  core.issue.mockResolvedValue({ ...shown, data: { ...shown.data, hidden: "archived" } });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(await screen.findByRole("button", { name: "Mark Seen" }));

  expect(core.keepArchived).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
  expect(core.archive).not.toHaveBeenCalled();
  expect(core.markSeen).not.toHaveBeenCalled();
});

it("says why an Archive failed", async () => {
  core.archive.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  const { prRow } = await listing();
  prRow.focus();
  await fireEvent.keyDown(prRow, { key: "ArrowDown" });

  await fireEvent.keyDown(prRow, { key: "e" });

  expect(await screen.findByText("Tsugiki couldn't mark this item: no database")).toBeTruthy();
});

it("mutes on M the item that holds the line under the cursor, and the cursor lands on the next row", async () => {
  const changed = catchEveryListener();
  const { treeRow } = await listing();
  treeRow.focus();
  await fireEvent.keyDown(treeRow, { key: "ArrowDown" });
  await fireEvent.keyDown(treeRow, { key: "ArrowUp" });

  await fireEvent.keyDown(treeRow, { key: "m" });
  await fireEvent.keyDown(treeRow, { key: "m", repeat: true });
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  changed("python/cpython", issue.seen.mark.numbers);

  expect(core.mute).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
  const landed = await screen.findByRole("button", { name: "#5 Alone" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
});

it("mutes on M with the list hidden the item of the PR on show", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });

  await press("m");

  expect(core.mute).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
});

it("mutes the Issue on show from its button, the list follows, and the button turns into Unmute", async () => {
  const changed = catchEveryListener();
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Mute" }));
  expect(core.mute).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key, issue.seen.mark);
  const shown = await core.issue.mock.results[0]?.value;
  core.issue.mockResolvedValue({ ...shown, data: { ...shown.data, hidden: "muted" } });
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  changed("python/cpython", issue.seen.mark.numbers);
  const unmute = await screen.findByRole("button", { name: "Unmute" });

  const list = within(issueList() as HTMLElement);
  expect(list.queryByRole("button", { name: "#112564 GIL drop during collection" })).toBeNull();
  expect(screen.queryByRole("button", { name: /^Mark (Un)?[sS]een$/ })).toBeNull();
  expect(screen.queryByRole("button", { name: "Archive" })).toBeNull();
  await fireEvent.click(unmute);
  expect(core.unmute).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key);
});

it("says a Muted row is Muted, where Q, Shift+Q and E do nothing and M unmutes it", async () => {
  const muted = { ...standalone, attention: null, hidden: "muted" };
  core.workbench.mockResolvedValue(workbenchPage([muted]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const row = await screen.findByRole("button", { name: "#5 Alone" });
  row.focus();
  await fireEvent.keyDown(row, { key: "ArrowDown" });

  for (const key of [{ key: "q" }, { key: "Q", shiftKey: true }, { key: "e" }]) {
    await fireEvent.keyDown(row, key);
  }
  await fireEvent.keyDown(row, { key: "m" });

  expect(within(screen.getByRole("listitem")).getByText("Muted")).toBeTruthy();
  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
  expect(core.archive).not.toHaveBeenCalled();
  expect(core.mute).not.toHaveBeenCalled();
  expect(core.unmute).toHaveBeenCalledExactlyOnceWith("python/cpython", standalone.key);
});

it("mutes from the palette the author of the line under the cursor, a PR's author on a PR's line", async () => {
  const { component, treeRow } = await listing();
  treeRow.focus();
  await fireEvent.keyDown(treeRow, { key: "ArrowDown" });
  await fireEvent.keyDown(treeRow, { key: "ArrowDown" });
  const onPr = paletteRow(component, "mute_author", treeRow);

  onPr?.run();
  await fireEvent.keyDown(treeRow, { key: "ArrowUp" });
  const onIssue = paletteRow(component, "mute_author", treeRow);

  expect(onPr?.title).toBe("Mute Author");
  expect(core.muteAuthor).toHaveBeenCalledExactlyOnceWith("python/cpython", "some-author");
  expect(onIssue?.title).toBe("Mute Author");
  onIssue?.run();
  expect(core.muteAuthor).toHaveBeenLastCalledWith("python/cpython", "issue-author");
});

it("lands the cursor on the next row once Mute author takes the item under it away", async () => {
  const changed = catchEveryListener();
  const { component, issueRow } = await listing();
  issueRow.focus();
  await fireEvent.keyDown(issueRow, { key: "ArrowDown" });

  paletteRow(component, "mute_author", issueRow)?.run();
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  changed("python/cpython", issue.seen.mark.numbers);

  const landed = await screen.findByRole("button", { name: "#5 Alone" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
});

it("offers Unmute Author on a Muted author's row, where Q, Shift+Q, E and M do nothing", async () => {
  const theirs = {
    ...standalone,
    attention: null,
    hidden: "muted_author",
    mute_author: { kind: "unmute", login: "some-author" },
  };
  core.workbench.mockResolvedValue(workbenchPage([theirs]));
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  const row = await screen.findByRole("button", { name: "#5 Alone" });
  row.focus();
  await fireEvent.keyDown(row, { key: "ArrowDown" });

  for (const key of [{ key: "q" }, { key: "Q", shiftKey: true }, { key: "e" }, { key: "m" }]) {
    await fireEvent.keyDown(row, key);
  }
  const command = paletteRow(component, "mute_author", row);
  command?.run();

  expect(within(screen.getByRole("listitem")).getByText("Muted")).toBeTruthy();
  for (const mark of [core.markSeen, core.markUnseen, core.archive, core.mute, core.unmute]) {
    expect(mark).not.toHaveBeenCalled();
  }
  expect(command?.title).toBe("Unmute Author");
  expect(core.unmuteAuthor).toHaveBeenCalledExactlyOnceWith("python/cpython", "some-author");
});

it("does nothing on M in the Detail pane before a Muted author's PR view arrives, as the list says", async () => {
  core.pullRequest.mockReturnValue(new Promise(() => {}));
  const theirs = { ...standalone, attention: null, hidden: "muted_author" };
  core.workbench.mockResolvedValue(workbenchPage([theirs]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await fireEvent.click(await screen.findByRole("button", { name: "#5 Alone" }));
  const view = await screen.findByRole("region", { name: "PR view" });

  await fireEvent.keyDown(view, { key: "m" });

  expect(core.mute).not.toHaveBeenCalled();
  expect(core.unmute).not.toHaveBeenCalled();
});

it("leaves Mute Author out of the palette on the user's own row", async () => {
  const own = { ...standalone, attention: null, author: "me", mute_author: { kind: "off" } };
  core.workbench.mockResolvedValue(workbenchPage([own]));
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  const row = await screen.findByRole("button", { name: "#5 Alone" });
  row.focus();
  await fireEvent.keyDown(row, { key: "ArrowDown" });

  expect(paletteRow(component, "mute_author", row)).toBeUndefined();
});

it("mutes the author of the Issue on show from its button, the list follows, and the button turns into Unmute", async () => {
  const changed = catchEveryListener();
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Mute issue-author" }));
  expect(core.muteAuthor).toHaveBeenCalledExactlyOnceWith("python/cpython", "issue-author");
  const shown = await core.issue.mock.results[0]?.value;
  const unmute: MuteAuthor = { kind: "unmute", login: "issue-author" };
  core.issue.mockResolvedValue({
    ...shown,
    data: { ...shown.data, hidden: "muted_author", mute_author: unmute },
  });
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  changed("python/cpython", issue.seen.mark.numbers);
  const button = await screen.findByRole("button", { name: "Unmute issue-author" });

  const list = within(issueList() as HTMLElement);
  expect(list.queryByRole("button", { name: "#112564 GIL drop during collection" })).toBeNull();
  expect(screen.queryByRole("button", { name: /^(Mute|Unmute|Archive)$/ })).toBeNull();
  await fireEvent.click(button);
  expect(core.unmuteAuthor).toHaveBeenCalledExactlyOnceWith("python/cpython", "issue-author");
});

it("folds the Muted items at the list's end behind a line that shows and hides them", async () => {
  const muted = { ...standalone, attention: null, hidden: "muted" };
  core.workbench.mockResolvedValue(workbenchPage([], { muted: [muted] }));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  const fold = await screen.findByRole("button", { name: "Show 1 muted" });
  const folded = screen.queryByRole("button", { name: "#5 Alone" });
  const empty = screen.queryByText(/^Nothing here yet/);
  await fireEvent.click(fold);
  const shown = screen.getByRole("list", { name: "Muted items" });

  expect([folded, empty, fold.getAttribute("aria-expanded")]).toEqual([null, null, "true"]);
  expect(within(shown).getByRole("button", { name: "#5 Alone" })).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Hide 1 muted" }));
  expect(screen.queryByRole("button", { name: "#5 Alone" })).toBeNull();
});

it("offers no Mark Seen in the view of a PR whose item is Muted, as the view says, listed or not", async () => {
  core.pullRequest.mockResolvedValue({ status: "ok", data: { ...row, content: null, hidden: "muted" } });
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  const view = await screen.findByRole("region", { name: "PR view" });
  await within(view).findAllByText(/Fix UAF for gc debug print/);

  await fireEvent.keyDown(view, { key: "q" });
  await fireEvent.keyDown(view, { key: "Q", shiftKey: true });

  expect(screen.queryByRole("button", { name: /^Mark (Un)?[sS]een$/ })).toBeNull();
  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
});

/** A window on the Archived view (#491), which holds the Issue, unfolded, and the Standalone PR, with the
 * cursor on the Issue. */
const archivedView = async () => {
  const changed = catchEveryListener();
  const items = [issue, standalone].map((item) => ({ ...item, attention: null, hidden: "archived" }));
  core.workbench.mockResolvedValue(workbenchPage(items, { archived: 2 }));
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await fireEvent.click(await screen.findByRole("button", { name: "Archived 2" }));
  await screen.findByRole("heading", { name: "Archived" });
  await unfoldAll();
  const issueRow = screen.getByRole("button", { name: "#112564 GIL drop during collection" });
  issueRow.focus();
  await fireEvent.keyDown(issueRow, { key: "ArrowDown" });
  await fireEvent.keyDown(issueRow, { key: "ArrowUp" });
  return { changed, component, issueRow };
};

it("lists the Archived view with Unarchive on each row and no switch of order", async () => {
  const { component } = await archivedView();

  // One for each item, and none for the PR in the Issue's tree.
  const buttons = screen.getAllByRole("button", { name: "Unarchive" });
  await fireEvent.click(buttons[1] as HTMLElement);

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "archived" });
  expect(buttons).toHaveLength(2);
  expect(within(issueList() as HTMLElement).queryAllByText("Archived", { selector: "small" })).toEqual([]);
  expect(screen.queryByRole("button", { name: /⇅/ })).toBeNull();
  const titles = component.paletteCommands().map((command) => command.title);
  expect(titles.filter((title) => title.startsWith("Sort by"))).toEqual([]);
  expect(core.unarchive).toHaveBeenCalledExactlyOnceWith("python/cpython", standalone.key);
});

it("unarchives on E in the Archived view the item that holds the line under the cursor, which lands on the next row", async () => {
  const { changed, issueRow } = await archivedView();
  await fireEvent.keyDown(issueRow, { key: "ArrowDown" });

  await fireEvent.keyDown(issueRow, { key: "e" });
  core.workbench.mockResolvedValue(
    workbenchPage([{ ...standalone, attention: null, hidden: "archived" }], { archived: 1 }),
  );
  changed("python/cpython", [112564]);

  expect(core.unarchive).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.key);
  expect(core.archive).not.toHaveBeenCalled();
  const landed = await screen.findByRole("button", { name: "#5 Alone" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  expect(await screen.findByRole("button", { name: "Archived 1" })).toBeTruthy();
});

it("marks a line of the Archived view unseen on Shift+Q, which unarchives the item, and the cursor moves on", async () => {
  const { changed, issueRow } = await archivedView();
  await fireEvent.keyDown(issueRow, { key: "ArrowDown" });

  await fireEvent.keyDown(issueRow, { key: "Q", shiftKey: true });
  core.workbench.mockResolvedValue(
    workbenchPage([{ ...standalone, attention: null, hidden: "archived" }], { archived: 1 }),
  );
  changed("python/cpython", [160662, 112564]);

  expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662, issue.key);
  expect(core.unarchive).not.toHaveBeenCalled();
  const landed = await screen.findByRole("button", { name: "#5 Alone" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
});

it("offers the palette Archived, which shows the list with that view, and says when nothing is archived", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("navigation", { name: "Sidebar" });

  component
    .paletteCommands()
    .find((command) => command.title === "View: Archived")
    ?.run();

  expect(await screen.findByRole("heading", { name: "Archived" })).toBeTruthy();
  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "archived" });
  expect(screen.getByRole("button", { name: /^Archived/ }).getAttribute("aria-current")).toBe("true");
  expect(await screen.findByText("Nothing archived. E archives the item under the cursor.")).toBeTruthy();
});

it("offers Mark Unseen in the header of an item with nothing new, and the button marks it unseen", async () => {
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...row, number: 5, seen: { mark: row.seen.mark, state: "seen" }, content: null },
  });
  const { prRow } = await listing();
  await fireEvent.click(prRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Mark Unseen" }));

  expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 5, null);
  expect(core.markSeen).not.toHaveBeenCalled();
});

it("lets Q and Shift+Q type in a text box", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const box = document.body.appendChild(document.createElement("input"));

  await fireEvent.keyDown(box, { key: "q" });
  await fireEvent.keyDown(box, { key: "Q", shiftKey: true });

  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
  box.remove();
});

it("marks from the palette the item as it is when the row runs, after a sync that came while it was open", async () => {
  const changed = catchEveryListener();
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("button", { name: "Mark Seen" });
  const markSeen = paletteRow(component, "mark_seen", document.querySelector('[data-pane="detail"]'));

  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...row, seen: { mark: laterMark, state: "seen" }, content: null },
  });
  changed("python/cpython", [160662]);
  await screen.findByRole("button", { name: "Mark Unseen" });
  markSeen?.run();

  await vi.waitFor(() => {
    expect(core.markSeen).toHaveBeenCalledExactlyOnceWith("python/cpython", laterMark);
  });
});

it("marks nothing from the palette once what Q would mark has gone before the row runs", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { component } = await listing();
  // The cursor on a line, with nothing open.
  await press("ArrowDown");
  const rows = [paletteRow(component, "mark_seen"), paletteRow(component, "mark_unseen")];
  // With the list hidden, Q marks the Detail pane's item, and nothing is open.
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });
  await screen.findByText(/Nothing open/);
  for (const row of rows) row?.run();
  await tick();

  expect(rows.every((row) => row !== undefined)).toBe(true);
  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
});

it("lists Q and Shift+Q on ? only while they have an item to mark", async () => {
  const { component, issueRow } = await listing();
  const works = () => {
    const here = component.keysHere(keyTarget(document.body, component.paneOf));
    return [here.works("mark_seen"), here.works("mark_unseen")];
  };
  // No line is under the cursor, and nothing is on show.
  const nothing = works();
  await fireEvent.click(issueRow);

  expect([nothing, works()]).toEqual([
    [false, false],
    [true, true],
  ]);
});

it("marks from the palette what Q and Shift+Q mark where the focus was: the line under the cursor, or the Detail pane's item", async () => {
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...row, seen: { mark: laterMark, state: "new_activity" }, content: null },
  });
  const { component, issueRow, treeRow } = await listing();
  // With no line under the cursor and nothing on show, Q has nothing to mark.
  const nothing = paletteRow(component, "mark_seen");
  await fireEvent.click(treeRow);
  const view = await screen.findByRole("region", { name: "PR view" });
  await screen.findByRole("button", { name: "Mark Seen" });

  paletteRow(component, "mark_seen", issueRow)?.run();
  paletteRow(component, "mark_seen", view)?.run();
  paletteRow(component, "mark_unseen", issueRow)?.run();

  expect(nothing).toBeUndefined();
  await vi.waitFor(() => {
    expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662, null);
  });
  expect(core.markSeen.mock.calls).toEqual([
    ["python/cpython", row.seen.mark],
    ["python/cpython", laterMark],
  ]);
});

it("marks the item on show from its header's button, and says why the core refused", async () => {
  core.markSeen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Mark Seen" }));

  expect(core.markSeen).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.seen.mark);
  expect((await screen.findByText(/couldn't mark/)).textContent).toBe(
    "Tsugiki couldn't mark this item: no database",
  );
});

it("leaves a mark's refusal behind with the Repository it was made in", async () => {
  let refuse: (value: unknown) => void = () => {};
  core.markSeen.mockReturnValue(
    new Promise((resolve) => {
      refuse = resolve;
    }),
  );
  const { issueRow, rerender } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(await screen.findByRole("button", { name: "Mark Seen" }));

  await rerender({ name: "python/other" });
  refuse({ status: "error", error: { kind: "internal", message: "no database" } });
  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("python/other", { kind: "whole" });
  });
  await nextTask();

  expect(screen.queryByText(/couldn't mark/)).toBeNull();
});

it("offers no command to fold the items with nothing waiting", async () => {
  answering({ ...issue, attention: null });
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });

  expect(component.paletteCommands().map((command) => command.title)).not.toContainEqual(
    expect.stringMatching(/quiet/i),
  );
});

it("shows another Repository as a launch does: its Workbench, an empty Detail pane, the cursor on top", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 1, problem: null };
  core.workbench.mockResolvedValue(
    workbenchPage([{ ...issue, attention: null }], { count: 1, filters: [mine] }),
  );
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await fireEvent.click(await screen.findByRole("button", { name: "Mine 1" }));
  await unfoldAll();
  await fireEvent.click(
    await screen.findByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  await screen.findByRole("region", { name: "PR view" });

  await rerender({ name: "python/other" });

  const workbench = await screen.findByRole("button", { name: "Workbench 1" });
  await vi.waitFor(() => {
    expect(workbench.getAttribute("aria-current")).toBe("true");
  });
  expect(core.workbench).toHaveBeenLastCalledWith("python/other", { kind: "whole" });
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
  // As at a launch, no line has the cursor yet, and the first ↓ puts it on the top one.
  const first = await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  expect(document.querySelector("[data-item][aria-current]")).toBeNull();
  await press("ArrowDown");
  expect(first.getAttribute("aria-current")).toBe("true");
});

it("unfolds an Issue with → and folds it with ←, which on a PR's row goes to its Issue first", async () => {
  const { issueRow, treeRow, prRow } = await listing();
  const fold = screen.getByRole("button", { name: "PRs of #112564" });
  const shown = () => fold.getAttribute("aria-expanded");
  await press("ArrowDown");
  await press("ArrowDown");
  expect(document.activeElement).toBe(treeRow);

  await press("ArrowRight");
  await press("ArrowLeft");
  const onIssue = [document.activeElement === issueRow, shown()];
  await press("ArrowLeft");
  const folded = shown();
  await press("ArrowDown");
  const skipped = document.activeElement === prRow;
  await press("ArrowLeft");
  await press("ArrowUp");
  await press("ArrowRight");

  expect([...onIssue, folded, skipped]).toEqual([true, "true", "false", true]);
  expect([shown(), document.activeElement]).toEqual(["true", issueRow]);
  await fireEvent.click(fold);
  expect(shown()).toBe("false");
});

it("leaves ← and → to whatever has the focus outside the list, such as a diff that scrolls sideways", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const pane = await screen.findByRole("region", { name: "Issue view" });

  const sideways = keydown({ key: "ArrowLeft" });
  pane.dispatchEvent(sideways);

  expect(sideways.defaultPrevented).toBe(false);
  expect(screen.getByRole("button", { name: "PRs of #112564" }).getAttribute("aria-expanded")).toBe("true");
});

it("leaves ← and → to a pane's edge that has the focus, and to a hidden list", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const fold = screen.getByRole("button", { name: "PRs of #112564" });
  const edge = screen.getByRole("separator", { name: "Sidebar width" });
  const heard = vi.fn();
  window.addEventListener("keydown", heard);

  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  window.removeEventListener("keydown", heard);
  // The keys the edge takes go no further.
  expect(heard).not.toHaveBeenCalled();
  const kept = fold.getAttribute("aria-expanded");
  await press("B", { ctrlKey: true, altKey: true });
  await press("ArrowLeft");
  await press("B", { ctrlKey: true, altKey: true });

  expect([
    kept,
    screen.getByRole("button", { name: "PRs of #112564" }).getAttribute("aria-expanded"),
  ]).toEqual(["true", "true"]);
});

it("goes back with Alt+← to a PR in a folded Issue, and unfolds it to put the cursor on the PR", async () => {
  const { treeRow, prRow } = await listing();
  await fireEvent.click(treeRow);
  await fireEvent.click(prRow);
  await fireEvent.click(screen.getByRole("button", { name: "PRs of #112564" }));

  await press("ArrowLeft", { altKey: true });

  const back = await screen.findByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" });
  expect(back.getAttribute("aria-current")).toBe("true");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(back);
  });
});

it("starts the way back fresh in another Repository", async () => {
  answering({ ...issue, attention: null });
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await rerender({ name: "python/other" });
  await unfoldAll();
  await fireEvent.click(
    await screen.findByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  await screen.findByRole("region", { name: "PR view" });

  await press("ArrowLeft", { altKey: true });

  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
});

it("leaves Ctrl+PageDown alone with no PR on show", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const pageDown = keydown({ key: "PageDown", ctrlKey: true });

  await fireEvent(document.body, pageDown);

  expect(pageDown.defaultPrevented).toBe(false);
});

it("switches tabs on a click and on Enter on the open PR's line, and reads it once", async () => {
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });

  await fireEvent.click(prRow);
  expect(files.getAttribute("aria-selected")).toBe("true");
  await press("Enter");
  expect(files.getAttribute("aria-selected")).toBe("false");
  expect(core.openPullRequest).toHaveBeenCalledOnce();
});

it("says so when the browser won't open", async () => {
  core.openOnGithub.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  await listing();

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });

  expect((await screen.findByRole("alert")).textContent).toContain(
    "Tsugiki couldn't open the system browser.",
  );
});

it("leaves the browser's problem behind with the Repository it came from", async () => {
  core.openOnGithub.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  answering({ ...issue, attention: null });
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });
  await screen.findByRole("alert");

  await rerender({ name: "python/other" });

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("python/other", { kind: "whole" });
  });
  expect(screen.queryByRole("alert")).toBeNull();
});

it("moves through the list and the PRs in its trees with the arrows, and focus follows", async () => {
  const { issueRow, treeRow, prRow } = await listing();

  await press("ArrowDown");
  expect(document.activeElement).toBe(issueRow);
  await press("ArrowDown");
  expect(document.activeElement).toBe(treeRow);
  await press("ArrowDown");
  expect(document.activeElement).toBe(prRow);
  await press("ArrowUp");
  expect(document.activeElement).toBe(treeRow);
  expect(core.openIssue).not.toHaveBeenCalled();
  expect(core.openPullRequest).not.toHaveBeenCalled();
});

it("does nothing on the arrows with an empty list", async () => {
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await screen.findByText(/Nothing here yet/);

  await press("ArrowDown");

  expect(document.activeElement).toBe(document.body);
});

it("opens the selection with Enter when focus has left the list", async () => {
  const { issueRow } = await listing();
  await press("ArrowDown");
  issueRow.blur();

  await press("Enter");

  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
});

it("leaves Enter alone with nothing selected, and on a focused control", async () => {
  const { issueRow } = await listing();

  await press("Enter");
  await press("ArrowDown");
  await fireEvent.keyDown(issueRow, { key: "Enter" });

  expect(core.openIssue).not.toHaveBeenCalled();
});

it("takes a held Enter as one press", async () => {
  const { issueRow } = await listing();
  await press("ArrowDown");
  const repeat = keydown({ key: "Enter", repeat: true });

  await fireEvent(issueRow, repeat);

  expect(repeat.defaultPrevented).toBe(true);
});

it("leaves F12 to the window's guard away from the Files tab, where it has nothing to go to", async () => {
  await listing();
  const f12 = keydown({ key: "F12" });

  await fireEvent(document.body, f12);

  expect(f12.defaultPrevented).toBe(false);
});

it("Ctrl+Shift+G still works from a text box", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const box = document.body.appendChild(document.createElement("input"));

  await fireEvent.keyDown(box, { key: "G", ctrlKey: true, shiftKey: true });
  box.remove();

  expect(core.openOnGithub).toHaveBeenCalledOnce();
});

it("leaves the arrows to a text box", async () => {
  await listing();
  const box = document.body.appendChild(document.createElement("input"));

  await fireEvent.keyDown(box, { key: "ArrowDown" });
  box.remove();

  expect(document.activeElement).toBe(document.body);
});

it("leaves its keys alone while a dialog is on top", async () => {
  core.workbench.mockResolvedValue(workbenchPage([{ ...issue, attention: null }]));
  render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
    keysActive: false,
  });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });

  expect(document.activeElement).toBe(document.body);
  expect(core.openOnGithub).not.toHaveBeenCalled();
});

it("Ctrl+Shift+G opens the Issue on show on GitHub", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await press("G", { ctrlKey: true, shiftKey: true });

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", {
    kind: "issue",
    number: 112564,
  });
});

it("Ctrl+Shift+G opens what's under the cursor when no Issue is on show", async () => {
  await listing();

  await press("G", { ctrlKey: true, shiftKey: true });
  expect(core.openOnGithub).not.toHaveBeenCalled();

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });
  expect(core.openOnGithub).toHaveBeenLastCalledWith("python/cpython", { kind: "issue", number: 112564 });

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });
  expect(core.openOnGithub).toHaveBeenLastCalledWith("python/cpython", {
    kind: "pull_request",
    number: 160662,
  });
});

it("Ctrl+Shift+G opens the PR on show on GitHub", async () => {
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("button", { name: "gh-112564" });

  await press("G", { ctrlKey: true, shiftKey: true });

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", {
    kind: "pull_request",
    number: 160662,
  });
});

it("closes the Detail pane when the Repository changes", async () => {
  core.workbench.mockResolvedValue(workbenchPage([{ ...issue, attention: null }]));
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();

  await rerender({ name: "python/other" });

  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
});

/** Lays the panes out `sidebar`, `list` and `detail` pixels wide, as jsdom lays nothing out. */
const laidOut = (sidebar: number, list: number, detail: number) => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const width = this.matches("nav") ? sidebar : this.matches(".list") ? list : detail;
    return { width } as DOMRect;
  });
};

const home = () => document.querySelector<HTMLElement>(".home") as HTMLElement;

// The panes read which of them holds the focus with :focus-within, which e2e/focus.spec.ts checks in a
// browser. Only whether the window holds it needs the page's help.
it("marks the panes as out of focus while another app is in front", async () => {
  await listing();
  const marked = () => home().classList.contains("window-focused");

  await fireEvent.blur(window);
  const away = marked();
  await fireEvent.focus(window);

  expect([away, marked()]).toEqual([false, true]);
  expect(document.querySelector('[data-pane="list"]')?.classList.contains("list")).toBe(true);
});

it("drags the Sidebar's edge between its minimum and maximum, saves where it stops, and resets on a double-click", async () => {
  laidOut(200, 600, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  // A click that doesn't move the edge has nothing to save.
  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerUp(edge);

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 340 });
  const dragged = [home().style.getPropertyValue("--sidebar-width"), edge.getAttribute("aria-valuenow")];
  await fireEvent.pointerMove(edge, { clientX: 2000 });
  const widest = home().style.getPropertyValue("--sidebar-width");
  // 130 px: under the least width, and short of where the Sidebar folds to icons.
  await fireEvent.pointerMove(edge, { clientX: 230 });
  await fireEvent.pointerUp(edge);
  // After the drag, the pointer moves the edge no more, and the other button never does.
  await fireEvent.pointerMove(edge, { clientX: 340 });
  await fireEvent.pointerUp(edge);
  await fireEvent.pointerDown(edge, { button: 2, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 340 });
  const narrowest = home().style.getPropertyValue("--sidebar-width");

  await fireEvent.dblClick(edge);

  expect([dragged, widest, narrowest]).toEqual([["240px", "240"], "480px", "140px"]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([[{ ...DEFAULT_LAYOUT, sidebar: 140 }], [DEFAULT_LAYOUT]]);
  });
  expect(home().style.getPropertyValue("--sidebar-width")).toBe("200px");
});

it("stops the Sidebar's edge where the list would go under its minimum", async () => {
  laidOut(200, 300, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 400 });

  expect(home().style.getPropertyValue("--sidebar-width")).toBe("220px");
});

it("stops the Sidebar's edge where the Detail pane would go under its minimum while the list is hidden", async () => {
  laidOut(200, 600, 500);
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await toggleList();
  const edge = screen.getByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 2000 });

  expect(home().style.getPropertyValue("--sidebar-width")).toBe("380px");
});

it("offers the list's edge beside a Detail pane, and keeps the Detail pane's minimum", async () => {
  laidOut(200, 600, 500);
  const { issueRow } = await listing();
  const before = screen.queryByRole("separator", { name: "Workbench width" });
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  const edge = screen.getByRole("separator", { name: "Workbench width" });
  const list = document.querySelector(".list") as HTMLElement;
  const even = [list.classList.contains("sized"), home().style.getPropertyValue("--list-width")];

  await fireEvent.pointerDown(edge, { button: 0, clientX: 800, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 2000 });
  const widest = home().style.getPropertyValue("--list-width");
  await fireEvent.pointerMove(edge, { clientX: 0 });
  await fireEvent.pointerUp(edge);

  expect([before, even, widest]).toEqual([null, [false, ""], "780px"]);
  expect([home().style.getPropertyValue("--list-width"), list.classList.contains("sized")]).toEqual([
    "280px",
    true,
  ]);
  expect(core.savePaneLayout).toHaveBeenCalledExactlyOnceWith({ ...DEFAULT_LAYOUT, list: 280 });
  await fireEvent.dblClick(edge);
  expect(list.classList.contains("sized")).toBe(false);
});

it("lets the Sidebar grow into the room the Detail pane has to spare", async () => {
  laidOut(200, 280, 700);
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  const edge = screen.getByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 200, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 400 });

  expect(home().style.getPropertyValue("--sidebar-width")).toBe("400px");
});

it("moves an edge with the bare arrow keys, and saves a held key once, as it comes up", async () => {
  laidOut(200, 600, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });

  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  const right = [home().style.getPropertyValue("--sidebar-width"), core.savePaneLayout.mock.calls.length];
  await fireEvent.keyUp(edge, { key: "ArrowRight" });
  // The layout jsdom stands in for still says 200 px, so both presses land on 184 px.
  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyDown(edge, { key: "ArrowLeft", repeat: true });
  await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  await fireEvent.keyDown(edge, { key: "ArrowLeft", altKey: true });
  await fireEvent.keyDown(edge, { key: "Enter" });
  await fireEvent.keyUp(edge, { key: "Enter" });

  expect([right, home().style.getPropertyValue("--sidebar-width")]).toEqual([["216px", 0], "184px"]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, sidebar: 216 }],
      [{ ...DEFAULT_LAYOUT, sidebar: 184 }],
    ]);
  });
});

it("opens with the widths saved on this machine, and says when it can't save them", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260, list: 500 });
  core.savePaneLayout.mockResolvedValue({ status: "error", error: { kind: "internal", message: "locked" } });
  laidOut(260, 500, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await vi.waitFor(() => {
    expect(edge.getAttribute("aria-valuenow")).toBe("260");
  });

  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  await fireEvent.keyUp(edge, { key: "ArrowRight" });

  expect(home().style.getPropertyValue("--list-width")).toBe("500px");
  expect((await screen.findByRole("alert")).textContent).toBe("Tsugiki can't save the panes' layout: locked");
});

it("hides the Sidebar and its edge with Ctrl+B, and brings them back at their width", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260 });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await vi.waitFor(() => {
    expect(edge.getAttribute("aria-valuenow")).toBe("260");
  });

  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  const hidden = [screen.queryByRole("navigation"), screen.queryByRole("separator")];
  // A held key flips it no more.
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, repeat: true });
  const held = screen.queryByRole("navigation");
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });

  expect([hidden, held]).toEqual([[null, null], null]);
  expect(screen.getByRole("separator", { name: "Sidebar width" }).getAttribute("aria-valuenow")).toBe("260");
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, sidebar: 260, sidebar_hidden: true }],
      [{ ...DEFAULT_LAYOUT, sidebar: 260 }],
    ]);
  });
});

it("leaves a text box the key [keys] gives Hide or Show the Sidebar when it types there", async () => {
  remapKeys({ toggle_sidebar: "s" });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const box = document.createElement("input");
  document.body.append(box);

  await fireEvent.keyDown(box, { key: "s", code: "KeyS" });
  const typed = screen.queryByRole("navigation", { name: "Sidebar" });
  await fireEvent.keyDown(document.body, { key: "s", code: "KeyS" });
  box.remove();
  remapKeys({});

  expect(typed).toBeTruthy();
  expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
});

it("opens with the Sidebar hidden when it was hidden on this machine", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar_hidden: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await vi.waitFor(() => {
    expect(screen.queryByRole("navigation")).toBeNull();
  });
});

it("leaves the Sidebar alone on Ctrl+B while a dialog is on top", async () => {
  render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
    keysActive: false,
  });
  await screen.findByRole("navigation", { name: "Sidebar" });

  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });

  expect(screen.getByRole("navigation", { name: "Sidebar" })).toBeTruthy();
  expect(core.savePaneLayout).not.toHaveBeenCalled();
});

it("offers the palette a command that hides or shows the Sidebar, named for what it does", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const hide = paletteRow(component, "toggle_sidebar");

  hide?.run();
  await vi.waitFor(() => {
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  expect([hide?.title, paletteRow(component, "toggle_sidebar")?.title]).toEqual([
    "Hide Sidebar",
    "Show Sidebar",
  ]);
});

it("moves the focus to the cursor's row when Ctrl+B hides the Sidebar it was in", async () => {
  const { issueRow } = await listing();
  await fireEvent.keyDown(window, { key: "ArrowDown" });
  const view = screen.getByRole("button", { name: /^Review requests/ });
  view.focus();

  await fireEvent.keyDown(view, { key: "b", code: "KeyB", ctrlKey: true });

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(issueRow);
  });
});

/** The Sidebar on show: "full", "icons", or null while it's hidden. */
const sidebarShape = () => {
  const sidebar = screen.queryByRole("navigation", { name: "Sidebar" });
  return sidebar === null ? null : sidebar.classList.contains("icons") ? "icons" : "full";
};

it("shrinks the Sidebar to icons with «, and a click on an icon brings it back at its width on that view", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260 });
  const { issueRow } = await listing();
  await vi.waitFor(() => {
    expect(home().style.getPropertyValue("--sidebar-width")).toBe("260px");
  });
  await fireEvent.keyDown(window, { key: "ArrowDown" });
  const fold = screen.getByRole("button", { name: "Sidebar as Icons" });
  fold.focus();

  await fireEvent.click(fold);
  const icons = sidebarShape();
  const edge = screen.getByRole("separator", { name: "Sidebar width" }).getAttribute("aria-valuenow");
  // « goes with the full Sidebar, so the focus goes to the cursor's row.
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(issueRow);
  });
  await fireEvent.click(screen.getByRole("button", { name: "Stale reviews" }));

  expect([icons, edge, sidebarShape()]).toEqual(["icons", "48", "full"]);
  expect(home().style.getPropertyValue("--sidebar-width")).toBe("260px");
  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "stale_reviews" });
  // One save for «, and one for the click, which brings back the full Sidebar and the list together.
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, sidebar: 260, sidebar_icons: true }],
      [{ ...DEFAULT_LAYOUT, sidebar: 260 }],
    ]);
  });
});

it("brings back the full Sidebar and a hidden list with one save for a click on an icon", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar_icons: true, list_hidden: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await vi.waitFor(() => {
    expect(sidebarShape()).toBe("icons");
  });

  await fireEvent.click(screen.getByRole("button", { name: "Review requests" }));

  expect([sidebarShape(), document.querySelector(".list") !== null]).toEqual(["full", true]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([[DEFAULT_LAYOUT]]);
  });
});

it("brings back the full Sidebar on the view on show with a click on the Repository's initials", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar_icons: true });
  await listing();
  await vi.waitFor(() => {
    expect(sidebarShape()).toBe("icons");
  });
  const asked = core.workbench.mock.calls.length;

  await fireEvent.click(screen.getByRole("button", { name: "Full Sidebar" }));

  expect([sidebarShape(), core.workbench.mock.calls.length]).toEqual(["full", asked]);
});

it("brings the icons back with Ctrl+B twice when they were showing", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar_icons: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await vi.waitFor(() => {
    expect(sidebarShape()).toBe("icons");
  });

  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  const hidden = [sidebarShape(), screen.queryByRole("separator")];
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });

  expect([hidden, sidebarShape()]).toEqual([[null, null], "icons"]);
});

it("offers the palette each shape of the Sidebar but the one on show, and both while it's hidden", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const shapes = () =>
    component
      .paletteCommands()
      .map((command) => command.title)
      .filter((title) => title.includes("Sidebar"));
  const command = (title: string) => component.paletteCommands().find((one) => one.title === title);
  const full = shapes();

  command("View: Sidebar as Icons")?.run();
  await tick();
  const icons = [sidebarShape(), shapes()];
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  const hidden = shapes();
  command("View: Full Sidebar")?.run();
  await tick();

  expect([full, icons, hidden, sidebarShape()]).toEqual([
    ["View: Sidebar as Icons"],
    ["icons", ["View: Full Sidebar"]],
    ["View: Sidebar as Icons", "View: Full Sidebar"],
    "full",
  ]);
});

it("folds the Sidebar to icons on a drag past its least width, and keeps the width it had for the full one", async () => {
  laidOut(200, 600, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 260 });
  await fireEvent.pointerMove(edge, { clientX: 225 });
  const short = [sidebarShape(), home().style.getPropertyValue("--sidebar-width")];
  await fireEvent.pointerMove(edge, { clientX: 219 });
  const folded = [sidebarShape(), edge.getAttribute("aria-valuenow")];
  // The same drag, back past where it folds, unfolds the Sidebar at the width it reaches.
  await fireEvent.pointerMove(edge, { clientX: 250 });
  const back = [sidebarShape(), home().style.getPropertyValue("--sidebar-width")];
  await fireEvent.pointerMove(edge, { clientX: 100 });
  await fireEvent.pointerUp(edge);

  expect([short, folded, back]).toEqual([
    ["full", "140px"],
    ["icons", "48"],
    ["full", "150px"],
  ]);
  expect([sidebarShape(), home().style.getPropertyValue("--sidebar-width")]).toEqual(["icons", "200px"]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([[{ ...DEFAULT_LAYOUT, sidebar_icons: true }]]);
  });
});

it("folds the Sidebar with ← on its edge at its least width, and unfolds it with →", async () => {
  // A fractional scale may measure the Sidebar a hair over its least width.
  laidOut(140.0001, 600, 0);
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 140 });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await vi.waitFor(() => {
    expect(edge.getAttribute("aria-valuenow")).toBe("140");
  });

  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  const folded = sidebarShape();
  // Folded, the edge's value is the strip's width, within its range.
  const range = ["aria-valuenow", "aria-valuemin", "aria-valuemax"].map((name) => edge.getAttribute(name));
  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  const still = sidebarShape();
  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  await fireEvent.keyUp(edge, { key: "ArrowRight" });

  expect([folded, range, still, sidebarShape(), edge.getAttribute("aria-valuenow")]).toEqual([
    "icons",
    ["48", "48", "480"],
    "icons",
    "full",
    "140",
  ]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, sidebar: 140, sidebar_icons: true }],
      [{ ...DEFAULT_LAYOUT, sidebar: 140 }],
    ]);
  });
});

it("brings the Sidebar back at the width the last key let go at, when ← walks it down to fold", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 200 });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await vi.waitFor(() => {
    expect(edge.getAttribute("aria-valuenow")).toBe("200");
  });
  // jsdom lays nothing out, so the Sidebar measures the width it was given.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const width = this.matches("nav") ? Number(edge.getAttribute("aria-valuenow")) : 600;
    return { width } as DOMRect;
  });

  // Each press saves as its key comes up: 184, 168, 152 and 140, and the sixth folds.
  for (let press = 0; press < 6; press++) {
    await fireEvent.keyDown(edge, { key: "ArrowLeft" });
    await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  }
  const folded = sidebarShape();
  await fireEvent.click(screen.getByRole("button", { name: "Full Sidebar" }));

  expect([folded, sidebarShape(), home().style.getPropertyValue("--sidebar-width")]).toEqual([
    "icons",
    "full",
    "140px",
  ]);
});

it("brings the Sidebar back at the width the last drag let go at, when a second drag folds it", async () => {
  laidOut(200, 600, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 240 });
  await fireEvent.pointerUp(edge);
  laidOut(140, 600, 0);

  await fireEvent.pointerDown(edge, { button: 0, clientX: 240, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 200 });
  await fireEvent.pointerUp(edge);
  const folded = sidebarShape();
  await fireEvent.click(screen.getByRole("button", { name: "Full Sidebar" }));

  expect([folded, home().style.getPropertyValue("--sidebar-width")]).toEqual(["icons", "140px"]);
});

it("puts back the full Sidebar on Reset Layout", async () => {
  core.paneLayout.mockResolvedValue({
    ...DEFAULT_LAYOUT,
    sidebar: 260,
    sidebar_icons: true,
    sidebar_hidden: true,
  });
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await vi.waitFor(() => {
    expect(core.paneLayout).toHaveBeenCalled();
  });
  await tick();

  paletteRow(component, "reset_layout")?.run();

  await vi.waitFor(() => {
    expect(sidebarShape()).toBe("full");
  });
});

/** Presses Ctrl+Alt+B, which hides the Issue list or brings it back. */
/** The press that ends a chord's wait, which the page hands the chord's action. */
const chordEnd = () => new KeyboardEvent("keydown", { key: "z", code: "KeyZ" });

const toggleList = (target: Element = document.body, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(target, { key: "b", code: "KeyB", ctrlKey: true, altKey: true, ...init });

/** The list's row named `name`, which the Detail pane may name too. */
const rowNamed = (name: string) =>
  within(document.querySelector<HTMLElement>(".list") as HTMLElement).getByRole("button", { name });

it("hides the Issue list and its edge with Ctrl+Alt+B, and brings them back at their width", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, list: 500 });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("separator", { name: "Workbench width" });

  await toggleList();
  const hidden = [issueList(), screen.queryByRole("separator", { name: "Workbench width" })];
  const detail = screen.queryByRole("region", { name: "Issue view" });
  // A held key flips it no more.
  await toggleList(document.body, { repeat: true });
  const held = issueList();
  await toggleList();

  expect([hidden, held, detail === null]).toEqual([[null, null], null, false]);
  expect(screen.getByRole("separator", { name: "Workbench width" })).toBeTruthy();
  expect(home().style.getPropertyValue("--list-width")).toBe("500px");
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, list: 500, list_hidden: true }],
      [{ ...DEFAULT_LAYOUT, list: 500 }],
    ]);
  });
});

it("opens with the Issue list hidden when it was hidden on this machine", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, list_hidden: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });

  await vi.waitFor(() => {
    expect(issueList()).toBeNull();
  });
});

it("leaves the Issue list alone on Ctrl+Alt+B while a dialog is on top, and in a text box", async () => {
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
    keysActive: false,
  });
  await vi.waitFor(() => {
    expect(core.paneLayout).toHaveBeenCalled();
  });
  await toggleList();
  const behindDialog = issueList();
  await rerender({ keysActive: true });
  const box = document.createElement("input");
  document.body.append(box);

  await toggleList(box);
  box.remove();

  expect([behindDialog === null, issueList() === null]).toEqual([false, false]);
  expect(core.savePaneLayout).not.toHaveBeenCalled();
});

it("offers the palette a command that hides or shows the Issue list, named for what it does", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const command = () => paletteRow(component, "toggle_issue_list");
  const before = command();

  before?.run();
  await vi.waitFor(() => {
    expect(issueList()).toBeNull();
  });

  expect([before?.title, command()?.title]).toEqual(["Hide Issue List", "Show Issue List"]);
  expect([before?.id, command()?.id]).toEqual(["toggle_issue_list", "toggle_issue_list"]);
});

it("offers the palette Stale reviews, which shows the list with that view", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("navigation", { name: "Sidebar" });
  paletteRow(component, "toggle_issue_list")?.run();
  await vi.waitFor(() => {
    expect(issueList()).toBeNull();
  });

  component
    .paletteCommands()
    .find((command) => command.title === "View: Stale Reviews")
    ?.run();

  expect(await screen.findByRole("heading", { name: "Stale reviews" })).toBeTruthy();
  expect(issueList()).not.toBeNull();
  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "stale_reviews" });
  expect(screen.getByRole("button", { name: /^Stale reviews/ }).getAttribute("aria-current")).toBe("true");
});

it("names the view over the list, and switches its order, which the list asks for once it's saved", async () => {
  const { component } = await listing();
  expect(screen.getByRole("heading", { name: "Workbench" })).toBeTruthy();
  const asked = core.workbench.mock.calls.length;

  await fireEvent.click(screen.getByRole("button", { name: "By attention ⇅" }));

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenCalledTimes(asked + 1);
  });
  expect(core.savePaneLayout).toHaveBeenCalledExactlyOnceWith({ ...DEFAULT_LAYOUT, order: "last_update" });
  expect(core.savePaneLayout.mock.invocationCallOrder[0]).toBeLessThan(
    core.workbench.mock.invocationCallOrder[asked] ?? 0,
  );
  await fireEvent.click(screen.getByRole("button", { name: "By last update ⇅" }));
  expect(await screen.findByRole("button", { name: "By attention ⇅" })).toBeTruthy();
  expect(core.savePaneLayout).toHaveBeenLastCalledWith(DEFAULT_LAYOUT);

  // The palette offers the order the list isn't in.
  const sort = () => component.paletteCommands().find((command) => command.title.startsWith("Sort by"));
  const offered = [sort()?.title];
  sort()?.run();
  await screen.findByRole("button", { name: "By last update ⇅" });
  offered.push(sort()?.title);
  sort()?.run();
  expect(await screen.findByRole("button", { name: "By attention ⇅" })).toBeTruthy();
  expect(offered).toEqual(["Sort by Last Update", "Sort by Attention"]);
});

/** Presses Ctrl+K Z, which the page hands the screen once the chord ends. */
const detailOnly = async (component: { runChord: (action: Action, e: KeyboardEvent) => void }) => {
  component.runChord("toggle_detail_only", chordEnd());
  await tick();
};

const sidebar = () => screen.queryByRole("navigation", { name: "Sidebar" });

it("shows only the Detail pane on Ctrl+K Z, with the focus in it, and brings back both panes", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  // The cursor leaves the Issue on show for the PR under it.
  await press("ArrowDown");

  await detailOnly(component);
  const hidden = [sidebar(), issueList()];
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
  await detailOnly(component);

  expect(hidden).toEqual([null, null]);
  expect([sidebar() === null, issueList() === null]).toEqual([false, false]);
  const row = rowNamed("#112564 GIL drop during collection");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(row);
  });
  expect(row.getAttribute("aria-current")).toBe("true");
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...DEFAULT_LAYOUT, sidebar_hidden: true, list_hidden: true, detail_only_hid: "both" }],
    [DEFAULT_LAYOUT],
  ]);
});

it("runs a chord's action on a key of one press, and a key's action on a chord, as [keys] gives them", async () => {
  remapKeys({ toggle_detail_only: "ctrl+d", toggle_issue_list: "ctrl+k l" });
  const { component } = await listing();

  await press("d", { code: "KeyD", ctrlKey: true });
  await tick();
  expect([sidebar(), issueList()]).toEqual([null, null]);
  await press("d", { code: "KeyD", ctrlKey: true });
  await tick();
  component.runChord("toggle_issue_list", chordEnd());
  await tick();

  expect(sidebar()).toBeTruthy();
  expect(screen.getByRole("region", { name: "Detail pane" })).toBeTruthy();
  remapKeys({});
});

it("brings back only the Sidebar on Ctrl+K Z when the list was hidden, with the focus left in the Detail pane", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  await toggleList();

  await detailOnly(component);
  const hidden = sidebar();
  await detailOnly(component);

  expect([hidden, sidebar() === null, issueList()]).toEqual([null, false, null]);
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
});

it("leaves the focus on a control in the Detail pane while Ctrl+K Z keeps the list hidden", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  await toggleList();
  const title = await within(view).findByRole("button", { name: "#112564 GIL drop during collection" });
  title.focus();

  await detailOnly(component);
  await tick();
  const hiding = document.activeElement;
  await detailOnly(component);
  await tick();

  expect([hiding, document.activeElement]).toEqual([title, title]);
});

it("offers the palette Ctrl+K Z's command, named for what it does", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const command = () => paletteRow(component, "toggle_detail_only");
  const only = command();

  only?.run();
  await vi.waitFor(() => {
    expect(sidebar()).toBeNull();
  });
  const hidden = issueList();
  const panes = command();
  panes?.run();
  expect([only?.title, panes?.title]).toEqual(["Show Only the Detail Pane", "Show the Panes"]);

  await vi.waitFor(() => {
    expect(sidebar()).toBeTruthy();
  });
  expect([hidden, issueList() === null]).toEqual([null, false]);
});

/** Runs the palette's "Reset Layout". */
const resetLayout = (component: PaletteScreen) => {
  const command = paletteRow(component, "reset_layout");
  if (!command) throw new Error("The palette offers no Reset Layout");
  command.run();
};

it("resets the layout from the palette: both panes on show at their default widths, saved", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260, list: 500 });
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  await toggleList();

  resetLayout(component);

  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.lastCall).toEqual([DEFAULT_LAYOUT]);
  });
  expect(screen.getByRole("navigation", { name: "Sidebar" })).toBeTruthy();
  expect(issueList()).toBeTruthy();
  expect(home().style.getPropertyValue("--sidebar-width")).toBe("200px");
  expect(home().style.getPropertyValue("--list-width")).toBe("");
});

it("brings the list back on Reset Layout with the cursor and the focus on the open item's row", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await press("ArrowDown");
  await toggleList();

  resetLayout(component);

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(rowNamed("#112564 GIL drop during collection"));
  });
  expect(document.activeElement?.getAttribute("aria-current")).toBe("true");
});

it("leaves the cursor and the focus alone on Reset Layout while the list shows", async () => {
  const { component, issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  // The cursor leaves the Issue on show for the PR under it.
  await press("ArrowDown");

  resetLayout(component);
  await vi.waitFor(() => {
    expect(core.savePaneLayout).toHaveBeenCalled();
  });
  await tick();

  expect(document.activeElement).toBe(treeRow);
  expect(treeRow.getAttribute("aria-current")).toBe("true");
});

it("moves the focus to the Detail pane as Ctrl+Alt+B hides the list", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  issueRow.focus();

  await toggleList(issueRow);

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
});

it("brings the list back with the cursor and the focus on the open item's row", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  // The cursor leaves the Issue on show for the PR under it.
  await press("ArrowDown");
  await toggleList();

  await toggleList();

  const row = rowNamed("#112564 GIL drop during collection");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(row);
  });
  expect(row.getAttribute("aria-current")).toBe("true");
});

it("brings the list back with the focus on the cursor's row when the open item isn't in it", async () => {
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  await press("ArrowUp");
  // The PR leaves the Issue's tree, so the list has no row for what the Detail pane shows.
  core.workbench.mockResolvedValue(workbenchPage([{ ...issue, pull_requests: [], attention: null }]));
  await fireEvent.click(screen.getByRole("button", { name: "Review requests 0" }));
  await vi.waitFor(() => {
    expect(
      screen.queryByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
    ).toBeNull();
  });
  await toggleList();

  await toggleList();

  const row = rowNamed("#112564 GIL drop during collection");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(row);
  });
});

it("scrolls the Detail pane with ↑ and ↓ while it holds the focus beside the list (#602)", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  const scrollBy = vi.fn();
  Object.defineProperty(view, "scrollBy", { value: scrollBy });
  const title = within(view).getByRole("button", { name: "#112564 GIL drop during collection" });

  await fireEvent.keyDown(title, { key: "ArrowDown" });
  await fireEvent.keyDown(title, { key: "ArrowUp" });
  const cursor = rowNamed("#112564 GIL drop during collection").getAttribute("aria-current");
  await press("ArrowDown");

  expect(scrollBy.mock.calls).toEqual([[{ top: 40 }], [{ top: -40 }]]);
  expect(cursor, "the list's cursor stays while the Detail pane scrolls").toBe("true");
  expect(rowNamed("#160662 gh-112564: Fix UAF for gc debug print").getAttribute("aria-current")).toBe("true");
});

it("scrolls the Detail pane with ↑ and ↓ while the list is hidden, and leaves the cursor alone", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  const scrollBy = vi.fn();
  Object.defineProperty(view, "scrollBy", { value: scrollBy });
  await toggleList();

  await press("ArrowDown");
  await press("ArrowUp");
  await toggleList();

  expect(scrollBy.mock.calls).toEqual([[{ top: 40 }], [{ top: -40 }]]);
  expect(rowNamed("#112564 GIL drop during collection").getAttribute("aria-current")).toBe("true");
});

it("lets the Detail pane take the focus only while the list is hidden, so a click in it leaves Enter alone", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  const shown = view.getAttribute("tabindex");

  await toggleList();

  expect([shown, view.getAttribute("tabindex")]).toEqual([null, "-1"]);
});

it("says what went wrong above the panes while the list is hidden", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "error", error: { kind: "internal", message: "locked" } });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await screen.findByRole("navigation", { name: "Sidebar" });

  await toggleList();

  expect((await screen.findByRole("alert")).textContent).toBe("Tsugiki can't save the panes' layout: locked");
  expect(issueList()).toBeNull();
});

it("opens nothing on GitHub with Ctrl+Shift+G while the list is hidden and nothing is open", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();

  await press("G", { ctrlKey: true, shiftKey: true });

  expect(core.openOnGithub).not.toHaveBeenCalled();
});

it("opens nothing on Enter while the list is hidden, as the cursor's row doesn't show", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();

  await press("Enter");

  expect(screen.getByRole("region", { name: "Detail pane" })).toBeTruthy();
  expect(core.openIssue).not.toHaveBeenCalled();
});

it("scrolls the file view's code with ↑ and ↓ while the list is hidden, and not the diff under it", async () => {
  await onFileView();
  const diff = await screen.findByRole("region", { name: "Diff", hidden: true });
  const code = screen.getByRole("region", { name: "Definition's code" });
  const [onDiff, onCode] = [vi.fn(), vi.fn()];
  Object.defineProperty(diff, "scrollBy", { value: onDiff });
  Object.defineProperty(code, "scrollBy", { value: onCode });
  await toggleList();

  await press("ArrowDown");

  expect(onCode.mock.calls).toEqual([[{ top: 40 }]]);
  expect(onDiff).not.toHaveBeenCalled();
});

it("scrolls the diff with ↑ and ↓ on a PR's Files tab while the list is hidden", async () => {
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  const scrollBy = vi.fn();
  Object.defineProperty(diff, "scrollBy", { value: scrollBy });
  await toggleList();

  await press("ArrowDown");

  expect(scrollBy.mock.calls).toEqual([[{ top: 40 }]]);
});

it("still goes back through the Issues and PRs it opened with Alt+← while the list is hidden", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  await toggleList();

  await press("ArrowLeft", { altKey: true });

  const view = await screen.findByRole("region", { name: "Issue view" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
});

it("brings the list back with the view a click on the Sidebar picks, and keeps the Detail pane", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await toggleList();

  await fireEvent.click(screen.getByRole("button", { name: "Review requests 0" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "review_requests" });
  expect(issueList()).toBeTruthy();
  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.lastCall).toEqual([DEFAULT_LAYOUT]);
  });
});

it("says how to bring the list back when nothing is open, and does so on a click of the key", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();
  const empty = screen.getByRole("region", { name: "Detail pane" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(empty);
  });
  const said = empty.textContent;

  await fireEvent.click(within(empty).getByRole("button", { name: "Ctrl+Alt+B" }));

  expect(said).toBe("Nothing open. Ctrl+Alt+B shows the Issue list.");
  expect(issueList()).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Detail pane" })).toBeNull();
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(rowNamed("#112564 GIL drop during collection"));
  });
});

it("offers a button to bring the list back once [keys] leaves its key with none", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();
  remapKeys({ toggle_issue_list: "" });
  await tick();
  const empty = screen.getByRole("region", { name: "Detail pane" });

  expect(empty.textContent).toBe("Nothing open. Show the Issue list");
  await fireEvent.click(within(empty).getByRole("button", { name: "Show the Issue list" }));

  expect(screen.queryByRole("region", { name: "Detail pane" })).toBeNull();
  remapKeys({});
});

it("marks each Issue and PR in the list with the icon of its state", async () => {
  core.workbench.mockResolvedValue(
    workbenchPage([
      { ...issue, state: "NOT_PLANNED", attention: null },
      { ...issue, number: 1, state: null, pull_requests: [], attention: null },
      { ...standalone, is_draft: true, attention: null },
    ]),
  );
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await unfoldAll();

  const icons = screen.getAllByRole("img").map((icon) => icon.getAttribute("title"));

  expect(icons).toEqual(["Closed as not planned", "Open PR", "Draft PR"]);
});

/** A page of the Inbox holding `items`, whose next page starts after item `older`, if it names one. */
const inboxPage = (items: unknown[], older?: number) =>
  ok({ items, older: older === undefined ? null : { updated_at: "2026-11-04T09:00:00Z", number: older } });

/** Lays the Issue list out, as jsdom doesn't: 500 px of room over `content` px of items. */
function listRoom(content: number) {
  const tall = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(content);
  const room = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(500);
  onTestFinished(() => {
    tall.mockRestore();
    room.mockRestore();
  });
  return tall;
}

it("lists the Inbox from the Sidebar, each Issue with its PRs, in its own order", async () => {
  core.workbench.mockResolvedValue(workbenchPage([{ ...standalone, attention: null }]));
  core.inbox.mockResolvedValue(inboxPage([{ ...issue, attention: null }]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await screen.findByRole("button", { name: "#5 Alone" });
  expect(issueList()?.querySelector("ul.lazy")).toBeNull();

  await fireEvent.click(screen.getByRole("button", { name: "Inbox" }));

  expect(await screen.findByRole("heading", { name: "Inbox" })).toBeTruthy();
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  expect(screen.queryByRole("button", { name: "#5 Alone" })).toBeNull();
  expect(core.inbox).toHaveBeenCalledExactlyOnceWith("python/cpython", { kind: "latest" });
  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "whole" });
  expect(screen.getByRole("button", { name: "Inbox" }).getAttribute("aria-current")).toBe("true");
  expect(screen.queryByRole("button", { name: /⇅$/ })).toBeNull();
  // The browser lays out and paints only the items on screen.
  expect(issueList()?.querySelector("ul.lazy")).not.toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "PRs of #112564" }));
  expect(screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" })).toBeTruthy();
});

it("reads the Inbox's next page once the list is scrolled near its end", async () => {
  listRoom(5000);
  core.inbox
    .mockResolvedValueOnce(inboxPage([{ ...issue, attention: null }], 112564))
    .mockResolvedValueOnce(inboxPage([{ ...standalone, attention: null }]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  const pane = issueList() as HTMLElement;

  // Two screens from the end.
  pane.scrollTop = 3999;
  await fireEvent.scroll(pane);
  expect(core.inbox).toHaveBeenCalledOnce();

  pane.scrollTop = 4000;
  await fireEvent.scroll(pane);

  await screen.findByRole("button", { name: "#5 Alone" });
  expect(core.inbox).toHaveBeenLastCalledWith("python/cpython", {
    kind: "older",
    after: { updated_at: "2026-11-04T09:00:00Z", number: 112564 },
  });
  expect(screen.getByRole("button", { name: "#112564 GIL drop during collection" })).toBeTruthy();
});

it("reads the Inbox's pages one after another while they leave the list short of its end", async () => {
  core.inbox
    .mockResolvedValueOnce(inboxPage([{ ...issue, attention: null }], 112564))
    .mockResolvedValueOnce(inboxPage([{ ...standalone, attention: null }]));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));

  await screen.findByRole("button", { name: "#5 Alone" });
  expect(core.inbox).toHaveBeenCalledTimes(2);
});

it("reads no page of the Inbox while the list is hidden, and looks again once it's back", async () => {
  const tall = listRoom(5000);
  core.inbox
    .mockResolvedValueOnce(inboxPage([{ ...issue, attention: null }], 112564))
    .mockResolvedValueOnce(inboxPage([{ ...standalone, attention: null }]));
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });

  paletteRow(component, "toggle_issue_list")?.run();
  await vi.waitFor(() => {
    expect(issueList()).toBeNull();
  });
  tall.mockReturnValue(0);
  await tick();
  expect(core.inbox).toHaveBeenCalledOnce();

  paletteRow(component, "toggle_issue_list")?.run();

  await screen.findByRole("button", { name: "#5 Alone" });
  expect(core.inbox).toHaveBeenCalledTimes(2);
});

it("offers the palette the Inbox, which keeps its own order", async () => {
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onsettings: () => {},
  });
  await screen.findByText(/^Nothing here yet/);
  core.inbox.mockResolvedValue(inboxPage([]));
  const titles = () => component.paletteCommands().map((command) => command.title);
  expect(titles()).toContain("Sort by Last Update");

  component
    .paletteCommands()
    .find((command) => command.title === "View: Inbox")
    ?.run();

  expect(await screen.findByRole("heading", { name: "Inbox" })).toBeTruthy();
  expect(await screen.findByText(/^Nothing open here yet/)).toBeTruthy();
  expect(titles().filter((title) => title.startsWith("Sort by"))).toEqual([]);
});

it("says why it couldn't read the Inbox's older items, and keeps the list on show", async () => {
  core.inbox
    .mockResolvedValueOnce(inboxPage([{ ...issue, attention: null }], 112564))
    .mockResolvedValueOnce({ status: "error", error: { kind: "internal", message: "disk gone" } });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onsettings: () => {} });
  await fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));

  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toBe("Tsugiki can't read the Inbox's older items: disk gone");
  expect(screen.getByRole("button", { name: "#112564 GIL drop during collection" })).toBeTruthy();
});

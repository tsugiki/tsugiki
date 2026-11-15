import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { PanesState } from "$lib/state/panes.svelte";
import { PrViewState, type PrView } from "$lib/state/pr-view.svelte";
import PrDetail from "./PrDetail.svelte";

const core = vi.hoisted(() => ({
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  fetchPullRequest: vi.fn(),
  openOnGithub: vi.fn(),
  openLink: vi.fn(),
  pullRequestFiles: vi.fn(),
  cloning: vi.fn(),
  mirrorStates: vi.fn(),
  mirrorsHeld: vi.fn(),
  savePaneLayout: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequest: core.pullRequest,
    openPullRequest: core.openPullRequest,
    fetchPullRequest: core.fetchPullRequest,
    openOnGithub: core.openOnGithub,
    openLink: core.openLink,
    pullRequestFiles: core.pullRequestFiles,
    cloning: core.cloning,
    mirrorStates: core.mirrorStates,
    mirrorsHeld: core.mirrorsHeld,
    savePaneLayout: core.savePaneLayout,
  },
  events: {
    changed: { listen: core.listen },
    readFailed: { listen: core.listen },
    headsFetched: { listen: core.listen },
    headsFetchFailed: { listen: core.listen },
    cloningChanged: { listen: core.listen },
    mirrorStatesChanged: { listen: core.listen },
    mirrorsHeldChanged: { listen: core.listen },
  },
}));

const view: PrView = {
  number: 11,
  title: "gh-10: Fix the crash",
  author: "pr-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: { kind: "unreviewed" },
  seen: { mark: { numbers: [11], up_to: "2026-11-04T10:00:00Z" }, state: "new_activity" },
  content: {
    body_html: "<p>The description</p>",
    comments: [
      {
        id: 1,
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: '<p><a href="https://github.com/python/cpython/pull/1">A link</a></p>',
        hidden: null,
      },
    ],
    earlier_comments: 2,
    opened_at: null,
    reviews: [
      {
        id: 2,
        author: "some-reviewer",
        state: "APPROVED",
        commit: "0123abcdef",
        submitted_at: "2026-11-04T10:00:00Z",
        body_html: "",
        hidden: null,
        threads: [],
      },
    ],
    earlier_reviews: 0,
  },
  shares_review_of: null,
};

const NOW = new Date("2026-11-04T11:00:00Z");

/** The PR on show, which stops after each test, as its Files tab loads once it shows. */
let onShow: PrViewState | undefined;

/** Shows the PR as opened from Issue `issue`, in the panes laid out as `panes` says. */
const shown = async (
  answer: PrView | null,
  issue: number | undefined,
  onback = vi.fn(),
  onmark = vi.fn(),
  panes = new PanesState(),
) => {
  core.pullRequest.mockResolvedValue({ status: "ok", data: answer });
  const state = new PrViewState("python/cpython", 11, issue, panes);
  onShow = state;
  await state.load();
  render(PrDetail, { repository: "python/cpython", pr: state, panes, now: NOW, onback, onmark });
  return state;
};

afterEach(() => {
  onShow?.stop();
  onShow = undefined;
});

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(vi.fn());
  core.openPullRequest.mockResolvedValue({ status: "ok", data: null });
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.openLink.mockResolvedValue({ status: "ok", data: null });
  core.pullRequestFiles.mockResolvedValue({ status: "ok", data: { state: "fetching" } });
  core.cloning.mockResolvedValue(null);
  core.mirrorStates.mockResolvedValue([]);
  core.mirrorsHeld.mockResolvedValue(false);
});

it("opens on the Conversation: the title and author, the description, comments and Reviews", async () => {
  await shown(view, 10);

  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(screen.getByRole("heading", { level: 2 }).textContent).toContain("#11 gh-10: Fix the crash");
  expect(screen.getByText("pr-author").tagName).toBe("B");
  expect(screen.getByRole("tab", { name: "Conversation" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("false");
  expect(screen.getByText("The description").tagName).toBe("P");
  const headers = [...document.querySelectorAll("article > header, p.review")].map(
    (line) => line.textContent,
  );
  expect(headers).toEqual(["some-reviewer · 2h ago", "some-reviewer approved at 0123abc · 1h ago"]);
  expect(screen.getByText("2 earlier comments are on GitHub.")).toBeTruthy();
});

it("says over the title which Issue the PR is under, where it goes and when it changed", async () => {
  await shown(view, 10);

  expect(document.querySelector(".crumb")?.textContent.replace(/\s+/g, " ").trim()).toBe(
    "gh-10 › into main · updated 1h ago",
  );
  expect(document.querySelector("p.by")?.textContent).toBe(
    `pr-author ${document.querySelector(".words")?.textContent ?? ""}`,
  );
});

it("marks the PR's title with the icon of its state", async () => {
  await shown({ ...view, state: "OPEN", is_draft: true }, 10);

  expect(
    within(screen.getByRole("heading", { level: 2 }))
      .getByRole("img")
      .getAttribute("title"),
  ).toBe("Draft PR");
});

it("says it's reading from GitHub until the first read lands", async () => {
  await shown({ ...view, author: null, content: null }, 10);

  expect(screen.getByText("Reading from GitHub…")).toBeTruthy();
  expect(screen.getByText("deleted user")).toBeTruthy();
});

it.each([
  [
    "no_token",
    "Tsugiki can't ask GitHub for the Conversation without a token. The status bar says how to sign in.",
  ],
  ["waits_on_github", "The Conversation comes once GitHub answers."],
  ["not_on_github", "GitHub doesn't have this PR, or your token can't see it."],
  ["other", "Tsugiki couldn't read this PR's Conversation from GitHub. tsugiki.log says why."],
] as const)("says why the first read failed: %s", async (why, words) => {
  const state = await shown({ ...view, content: null }, 10);

  state.failedRead = why;

  await vi.waitFor(() => expect(screen.getByText(words)).toBeTruthy());
  expect(screen.queryByText("Reading from GitHub…")).toBeNull();
});

it("shows the Conversation it holds when a later read fails", async () => {
  const state = await shown(view, 10);

  state.failedRead = "waits_on_github";

  await vi.waitFor(() => expect(screen.getByText("The description")).toBeTruthy());
  expect(screen.queryByText("The Conversation comes once GitHub answers.")).toBeNull();
});

it("switches to the Files tab on a click, which waits for the PR's commits", async () => {
  const state = await shown(view, 10);

  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));

  expect(state.tab).toBe("files");
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("true");
  await vi.waitFor(() =>
    expect(screen.getByRole("tabpanel", { name: "Files" }).textContent?.trim()).toBe(
      "Fetching the PR's commits…",
    ),
  );
  expect(screen.queryByText("The description")).toBeNull();

  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));
  expect(state.tab).toBe("conversation");
});

it("opens a thread's place in the Files tab", async () => {
  const thread = {
    id: "T1",
    path: "Lib/ast.py",
    subject: { kind: "line" as const, side: "RIGHT" as const, line: 12, original: 12 },
    opening: { kind: "words" as const, words: "Why not a set?" },
    replies: 0,
    resolution: "open" as const,
    anchor: "current" as const,
  };
  const reviews = view.content?.reviews.map((review) => ({ ...review, threads: [thread] })) ?? [];
  const content = view.content === null ? null : { ...view.content, reviews };
  const state = await shown({ ...view, content }, 10);
  const reveal = vi.spyOn(state.files, "reveal");

  await fireEvent.click(screen.getByRole("button", { name: "Lib/ast.py:12" }));

  expect(state.tab).toBe("files");
  expect(reveal).toHaveBeenCalledExactlyOnceWith("T1");
});

/** A PR whose Review at commit abc1234 went stale in three files, one of them only in whitespace. */
const stale: PrView = {
  ...view,
  your_review: {
    kind: "stale_review",
    reviewed: "abc1234def5678abc1234def5678abc1234def56",
    head: "def5678abc1234def5678abc1234def5678abc12",
    files: [
      { path: "Lib/ast.py", whitespace_only: false },
      { path: "Lib/os.py", whitespace_only: true },
      { path: "Lib/re.py", whitespace_only: false },
    ],
  },
};

/** The line over the tabs that says the user's Review is stale, if it shows. */
const staleLine = () => document.querySelector(".stale")?.textContent.replace(/\s+/g, " ").trim();

it("says over both tabs that the user's Review is stale, at which commit and how many files changed", async () => {
  await shown(stale, 10);
  const onConversation = staleLine();

  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));

  expect(onConversation).toBe(
    "Your Review at abc1234 is stale: 3 files changed, 1 only in whitespace Review changes",
  );
  expect(staleLine()).toBe(onConversation);
});

it("opens the Files tab at the first file changed since the Review on Review changes", async () => {
  const state = await shown(stale, 10);
  const showChanges = vi.spyOn(state.files, "showChanges");

  await fireEvent.click(screen.getByRole("button", { name: "Review changes" }));

  expect(state.tab).toBe("files");
  expect(showChanges).toHaveBeenCalledOnce();
});

it("names the Origin PR whose stale Review a Bot backport shares", async () => {
  await shown({ ...stale, shares_review_of: 160662 }, 10);

  // Its verdict read the Origin PR's commits, so the Backport's own files have nothing to point at.
  expect(staleLine()).toBe(
    "Your Review of #160662 at abc1234 is stale: 3 files changed, 1 only in whitespace",
  );
  expect(screen.queryByRole("button", { name: "Review changes" })).toBeNull();
});

it("says why a Review's check can't run over both tabs, with nothing to review yet", async () => {
  const reviewed = "abc1234def5678abc1234def5678abc1234def56";
  await shown({ ...view, your_review: { kind: "cant_check", reviewed, reason: { kind: "cloning" } } }, 10);

  expect(staleLine()).toBe(
    "Your Review at abc1234 may be stale: Tsugiki can't check it until the mirror is cloned",
  );
  expect(screen.queryByRole("button", { name: "Review changes" })).toBeNull();
});

it.each<PrView["your_review"]>([{ kind: "unreviewed" }, { kind: "reviewed" }])(
  "says nothing over the tabs of a PR %j",
  async (review) => {
    await shown({ ...view, your_review: review }, 10);

    expect(staleLine()).toBeUndefined();
    await fireEvent.click(screen.getByRole("tab", { name: "Files" }));
    expect(staleLine()).toBeUndefined();
  },
);

it("leaves a click on the tab already open alone", async () => {
  const state = await shown(view, 10);

  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));

  expect(state.tab).toBe("conversation");
});

it("goes back to the Issue with its gh-N link", async () => {
  const onback = vi.fn();
  await shown(view, 10, onback);

  await fireEvent.click(screen.getByRole("button", { name: "gh-10" }));

  expect(onback).toHaveBeenCalledExactlyOnceWith(10);
});

it("has no way back for a PR opened from no Issue", async () => {
  await shown(view, undefined);

  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens the title on GitHub", async () => {
  await shown(view, 10);

  await fireEvent.click(screen.getByRole("button", { name: "#11 gh-10: Fix the crash" }));

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", {
    kind: "pull_request",
    number: 11,
  });
});

it("sends a link in a comment to the browser, and says if that fails", async () => {
  core.openLink.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  await shown(view, 10);

  await fireEvent.click(screen.getByRole("link", { name: "A link" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith("https://github.com/python/cpython/pull/1");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Tsugiki couldn't open the system browser.",
  );
});

it("sends a link in a NEWS entry to the browser", async () => {
  // jsdom lays nothing out, so bind:clientHeight has nothing to observe.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const url = "https://github.com/python/cpython/issues/1";
  const news = [
    {
      path: "Misc/NEWS.d/next/Library/a.rst",
      section: "Library",
      issue: { text: "gh-1", to: { site: "gh_issue" as const, number: 1 } },
      blocks: [],
    },
  ];
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news, threads: [] },
  });
  await shown(view, 10);
  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));

  await fireEvent.click(await screen.findByRole("link", { name: "gh-1" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith(url);
  vi.unstubAllGlobals();
});

it("says so for a PR Tsugiki doesn't hold", async () => {
  await shown(null, 10);

  expect(screen.getByText("Tsugiki doesn't hold PR #11.")).toBeTruthy();
});

it("says why it can't show the PR", async () => {
  core.pullRequest.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "no database" },
  });
  const panes = new PanesState();
  const state = new PrViewState("python/cpython", 11, 10, panes);
  await state.load();
  render(PrDetail, {
    repository: "python/cpython",
    pr: state,
    panes,
    now: NOW,
    onback: vi.fn(),
    onmark: vi.fn(),
  });
  state.stop();

  expect(screen.getByRole("alert").textContent).toContain("Tsugiki can't show this PR: no database");
});

it("offers Mark Seen in its header while the PR has New activity, and Mark Unseen once it has none", async () => {
  const onmark = vi.fn();
  await shown(view, 10, vi.fn(), onmark);

  await fireEvent.click(screen.getByRole("button", { name: "Mark Seen" }));
  expect(onmark).toHaveBeenCalledOnce();
  cleanup();
  onShow?.stop();
  await shown({ ...view, seen: { ...view.seen, state: "seen" } }, 10);
  expect(screen.getByRole("button", { name: "Mark Unseen" })).toBeTruthy();
});

const rail = () => screen.queryByRole("complementary", { name: "Rail" });
const railButton = () => screen.getByRole("button", { name: "Files" });

it("opens the rail from the Files button at the right of the tabs, and closes it on a second click", async () => {
  const panes = new PanesState();
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const before = [rail(), railButton().getAttribute("aria-pressed")];

  await fireEvent.click(railButton());
  const opened = [
    within(rail() as HTMLElement).getByRole("heading").textContent,
    railButton().getAttribute("aria-pressed"),
    railButton().title,
    panes.railOpen,
  ];
  await fireEvent.click(railButton());

  expect(before).toEqual([null, "false"]);
  expect(opened).toEqual(["Files", "true", "Files in the rail (Ctrl+K F)", true]);
  expect([rail(), railButton().getAttribute("aria-pressed"), panes.railOpen]).toEqual([null, "false", false]);
  // The button stands beside the tabs, not among them.
  expect(within(screen.getByRole("tablist")).queryByRole("button", { name: "Files" })).toBeNull();
});

it("closes the rail on ×, and gives the focus to the Files button, as the whole pane scrolls the Conversation again", async () => {
  const panes = new PanesState();
  panes.railOpen = true;
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const panel = screen.getByRole("tabpanel");
  const beside = [panel.tabIndex, panel.hasAttribute("data-scrolls")];
  const close = screen.getByRole("button", { name: "Close the rail" });
  const tooltip = close.title;
  close.focus();

  await fireEvent.click(close);

  expect([beside, tooltip]).toEqual([[0, true], "Close (Ctrl+K F)"]);
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(railButton());
  });
  // Without the rail the whole pane scrolls, as before.
  expect([rail(), panel.hasAttribute("tabindex"), panel.hasAttribute("data-scrolls")]).toEqual([
    null,
    false,
    false,
  ]);
});

it("leaves the focus where it went after the rail held it, as the rail closes", async () => {
  const panes = new PanesState();
  panes.railOpen = true;
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const close = screen.getByRole("button", { name: "Close the rail" });
  close.focus();
  // A click on the page's background takes the focus to nowhere, and the Files button then takes it.
  close.blur();
  railButton().focus();

  await fireEvent.click(railButton());
  await tick();

  expect([rail(), document.activeElement]).toEqual([null, railButton()]);
});

it("gives the focus back to the Conversation on Esc in the rail, which stays open, and goes no further", async () => {
  const panes = new PanesState();
  panes.railOpen = true;
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const heard = vi.fn();
  window.addEventListener("keydown", heard);
  const close = screen.getByRole("button", { name: "Close the rail" });
  close.focus();

  // Another key in the rail goes on to the window.
  await fireEvent.keyDown(close, { key: "a" });
  await fireEvent.keyDown(close, { key: "Escape" });
  const fromClose = document.activeElement;
  // The edge is the rail's too.
  const edge = screen.getByRole("separator", { name: "Rail width" });
  edge.focus();
  await fireEvent.keyDown(edge, { key: "Escape" });
  window.removeEventListener("keydown", heard);

  expect([fromClose, document.activeElement]).toEqual([
    screen.getByRole("tabpanel"),
    screen.getByRole("tabpanel"),
  ]);
  expect([rail() !== null, heard.mock.calls.length]).toEqual([true, 1]);
});

it("keeps the rail open, at its width, from the Conversation to the Files tab and back", async () => {
  const panes = new PanesState();
  panes.railOpen = true;
  panes.rail = 360;
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const body = () => document.querySelector<HTMLElement>(".body") as HTMLElement;

  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));
  const onFiles = [rail() !== null, body().style.getPropertyValue("--rail-width")];
  // The diff scrolls by itself, so the tab doesn't.
  const panel = screen.getByRole("tabpanel");
  const scrolls = [panel.hasAttribute("tabindex"), panel.hasAttribute("data-scrolls")];
  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));

  expect([onFiles, scrolls]).toEqual([
    [true, "360px"],
    [false, false],
  ]);
  expect([rail() !== null, body().style.getPropertyValue("--rail-width")]).toEqual([true, "360px"]);
});

it("drags the rail's left edge between its least and greatest widths, saves where it stops, and resets on a double-click", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: null });
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    return { width: this.matches(".rail") ? 280 : 600 } as DOMRect;
  });
  // jsdom has no pointer capture.
  HTMLElement.prototype.setPointerCapture = vi.fn();
  const panes = new PanesState();
  panes.railOpen = true;
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const edge = screen.getByRole("separator", { name: "Rail width" });
  const width = () => panes.rail;

  await fireEvent.pointerDown(edge, { button: 0, clientX: 500, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 400 });
  const dragged = [width(), edge.getAttribute("aria-valuenow"), edge.getAttribute("aria-orientation")];
  // The Conversation keeps 240 px of its 600, which the rail can take the rest of, up to 640.
  await fireEvent.pointerMove(edge, { clientX: -2000 });
  const widest = width();
  await fireEvent.pointerMove(edge, { clientX: 3000 });
  await fireEvent.pointerUp(edge);
  const narrowest = width();
  // ← widens it, as it moves the edge left, and → narrows it.
  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  const keyed = width();
  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  await fireEvent.keyUp(edge, { key: "ArrowRight" });

  await fireEvent.dblClick(edge);

  expect([dragged, widest, narrowest, keyed]).toEqual([[380, "380", "vertical"], 640, 200, 216]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls.map(([layout]) => layout.rail)).toEqual([200, 216, 200, null]);
  });
});

it("keeps the rail's width while a narrow pane squeezes it, and narrows it from that width", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: null });
  // The rail keeps 280 px but shows 150, beside a Conversation at its least.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    return { width: this.matches(".rail") ? 150 : 240 } as DOMRect;
  });
  const panes = new PanesState();
  panes.railOpen = true;
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const edge = screen.getByRole("separator", { name: "Rail width" });

  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  const widened = panes.rail;
  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  await fireEvent.keyUp(edge, { key: "ArrowRight" });

  expect([widened, panes.rail]).toEqual([null, 264]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls.map(([layout]) => layout.rail)).toEqual([264]);
  });
});

it("keeps the Conversation where it was read to as the rail opens and closes, though its scroller moves", async () => {
  const panes = new PanesState();
  await shown(view, 10, vi.fn(), vi.fn(), panes);
  const section = screen.getByRole("region", { name: "PR view" });
  const panel = screen.getByRole("tabpanel");
  /** Lets `element` keep its scroll, which jsdom doesn't. */
  const scrolling = (element: HTMLElement) => {
    let top = 0;
    Object.defineProperty(element, "scrollTop", {
      get: () => top,
      set: (to: number) => {
        top = to;
      },
    });
  };
  scrolling(section);
  scrolling(panel);
  // The header above the tab is 100 px tall, and scrolls away with the pane.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    return { top: this === panel ? 100 - section.scrollTop : 0 } as DOMRect;
  });
  section.scrollTop = 400;

  panes.railOpen = true;
  await tick();
  const opened = panel.scrollTop;
  panel.scrollTop = 500;
  panes.railOpen = false;
  await tick();
  const closed = section.scrollTop;
  // Back at the top of the Conversation, the pane shows its header again.
  section.scrollTop = 50;
  panes.railOpen = true;
  await tick();
  const topOpened = panel.scrollTop;
  panes.railOpen = false;
  await tick();

  expect([opened, closed, topOpened, section.scrollTop]).toEqual([300, 600, 0, 0]);
});

it("hands the focus the pane takes on to the Conversation beside the rail, which scrolls as the pane doesn't", async () => {
  const panes = new PanesState();
  core.pullRequest.mockResolvedValue({ status: "ok", data: view });
  const state = new PrViewState("python/cpython", 11, 10, panes);
  onShow = state;
  await state.load();
  render(PrDetail, {
    repository: "python/cpython",
    pr: state,
    panes,
    now: NOW,
    onback: vi.fn(),
    onmark: vi.fn(),
    focusable: true,
  });
  const section = screen.getByRole("region", { name: "PR view" });
  const panel = screen.getByRole("tabpanel");
  section.focus();
  const alone = document.activeElement;
  panes.railOpen = true;
  await tick();
  const opened = document.activeElement;
  // A focus inside the pane stays where it is.
  railButton().focus();
  const button = document.activeElement;

  section.focus();

  expect([alone, opened, button, document.activeElement]).toEqual([section, panel, railButton(), panel]);
});

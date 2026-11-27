import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import type { IssueContent, IssueEvent } from "$lib/state/issue-view.svelte";
import type { PullRequestContent, Review, ThreadLine } from "$lib/state/pr-view.svelte";
import Conversation from "./Conversation.svelte";
import { stampOf } from "./time";

const NOW = new Date("2026-11-04T12:00:00Z");

const thread = (changes: Partial<ThreadLine>): ThreadLine => ({
  id: "T1",
  path: "Lib/ast.py",
  subject: { kind: "line", side: "RIGHT", line: 12, original: 12 },
  opening: { kind: "words", words: "Why not a set?" },
  replies: 0,
  resolution: "open",
  anchor: "current",
  ...changes,
});

const review = (changes: Partial<Review>): Review => ({
  id: null,
  author: "some-reviewer",
  state: "COMMENTED",
  commit: "0123abcdef",
  submitted_at: "2026-11-04T10:00:00Z",
  body_html: "",
  hidden: null,
  threads: [],
  ...changes,
});

const reviewed = (reviews: Review[], changes: Partial<PullRequestContent> = {}): PullRequestContent => ({
  body_html: "<p>The description</p>",
  comments: [],
  earlier_comments: 0,
  events: [],
  earlier_events: 0,
  opened_at: null,
  reviews,
  earlier_reviews: 0,
  ...changes,
});

const shown = (content: IssueContent | PullRequestContent, onthread?: (id: string) => void) => {
  const onlink = vi.fn();
  const props = { content, now: NOW, onlink };
  const { container } = render(Conversation, onthread === undefined ? props : { ...props, onthread });
  return container;
};

/** Each block's first line, top to bottom. */
const blocks = (container: HTMLElement) =>
  [...container.querySelectorAll("article > header")].map((line) => line.textContent.trim());

it("heads the description, and says when there is none", () => {
  const container = shown(reviewed([]));
  expect(screen.getByRole("heading", { name: "Description" })).toBeTruthy();
  expect(container.querySelector(".post")?.textContent.trim()).toBe("The description");
  cleanup();

  shown(reviewed([], { body_html: "" }));
  expect(screen.getByText("No description.")).toBeTruthy();
});

it("gives a comment's and a Review's time the date and time in a tooltip", () => {
  const container = shown(
    reviewed([review({})], {
      comments: [
        {
          id: 1,
          author: "some-author",
          created_at: "2026-11-04T09:00:00Z",
          body_html: "<p>Hi</p>",
          hidden: null,
        },
      ],
    }),
  );

  const times = [...container.querySelectorAll(".muted")].map((muted) => [
    muted.textContent,
    muted.getAttribute("title"),
  ]);
  expect(times).toEqual([
    [" · 3h ago", stampOf("2026-11-04T09:00:00Z")],
    [" at 0123abc · 2h ago", stampOf("2026-11-04T10:00:00Z")],
  ]);
});

it("opens the person Filter from a Review's author, and does nothing unless the page listens", async () => {
  const content = reviewed([review({ author: "review-author" })], {
    comments: [
      {
        id: 1,
        author: "comment-author",
        created_at: "2026-11-04T09:00:00Z",
        body_html: "<p>Hi</p>",
        hidden: null,
      },
    ],
  });
  const onperson = vi.fn();
  const first = render(Conversation, { content, now: NOW, onlink: vi.fn(), onperson });

  await fireEvent.click(within(first.container).getByRole("button", { name: "review-author" }));
  expect(onperson).toHaveBeenCalledExactlyOnceWith("review-author");
  first.unmount();

  render(Conversation, { content, now: NOW, onlink: vi.fn() });
  await fireEvent.click(screen.getByRole("button", { name: "comment-author" }));
});

it("puts a Review's state in a chip, green for an approval and red for changes requested", () => {
  const container = shown(reviewed([review({ state: "APPROVED" }), review({ state: "CHANGES_REQUESTED" })]));

  const chips = [...container.querySelectorAll(".chip")];
  expect(
    chips.map((chip) => [
      chip.textContent,
      chip.classList.contains(chip.textContent === "approved" ? "approved" : "changes_requested"),
    ]),
  ).toEqual([
    ["approved", true],
    ["requested changes", true],
  ]);
});

it("shows each Review as a block in time order among the comments, with a line for each thread", () => {
  const container = shown(
    reviewed(
      [
        review({
          author: "other-reviewer",
          state: "CHANGES_REQUESTED",
          submitted_at: "2026-11-04T11:00:00Z",
          body_html: "<p>A few notes</p>",
          threads: [
            thread({ replies: 2, resolution: "resolved" }),
            thread({
              path: "Lib/test/test_ast.py",
              subject: { kind: "file" },
              replies: 1,
              anchor: "outdated",
            }),
          ],
        }),
        review({
          submitted_at: "2026-11-04T09:00:00Z",
          threads: [thread({ subject: { kind: "line", side: "LEFT", line: 30, original: 28 } })],
        }),
      ],
      {
        comments: [
          {
            id: 1,
            author: "pr-author",
            created_at: "2026-11-04T10:00:00Z",
            body_html: "<p>Done</p>",
            hidden: null,
          },
        ],
      },
    ),
  );

  expect(blocks(container)).toEqual([
    "some-reviewer reviewed at 0123abc · 3h ago",
    "pr-author · 2h ago",
    "other-reviewer requested changes at 0123abc · 1h ago",
  ]);
  expect(screen.getByText("A few notes").tagName).toBe("P");
  const lines = screen.getAllByRole("listitem").map((line) => line.textContent.replace(/\s+/g, " ").trim());
  expect(lines).toEqual([
    "Lib/ast.py:30 Why not a set?",
    "Lib/ast.py:12 Why not a set? 2 replies Resolved",
    "Lib/test/test_ast.py Why not a set? 1 reply Outdated",
  ]);
});

it("opens a thread from its place, for a PR", async () => {
  const onthread = vi.fn();
  shown(reviewed([review({ threads: [thread({ id: "T7" })] })]), onthread);

  await fireEvent.click(screen.getByRole("button", { name: "Lib/ast.py:12" }));

  expect(onthread).toHaveBeenCalledExactlyOnceWith("T7");
});

it("links every thread to Files, an Outdated one and one on the whole file too", () => {
  const moved = { kind: "line", side: "RIGHT", line: null, original: 7 } as const;
  const container = shown(
    reviewed([
      review({
        threads: [
          thread({ anchor: "outdated", subject: moved }),
          thread({ subject: moved }),
          thread({ subject: { kind: "file" } }),
          thread({ subject: { kind: "line", side: "RIGHT", line: null, original: null } }),
        ],
      }),
    ]),
    vi.fn(),
  );

  expect([...container.querySelectorAll("button.place")].map((place) => place.textContent)).toEqual([
    "Lib/ast.py:7",
    "Lib/ast.py:7",
    "Lib/ast.py",
    "Lib/ast.py",
  ]);
});

it("shows a Review with no body and no threads as one line", () => {
  const container = shown(
    reviewed([review({ state: "APPROVED", commit: null, author: null }), review({ state: "DISMISSED" })]),
  );

  expect(blocks(container)).toEqual([]);
  const lines = [...container.querySelectorAll("p.review")].map((line) => line.textContent.trim());
  expect(lines).toEqual([
    "deleted user approved · 2h ago",
    "some-reviewer reviewed at 0123abc, since dismissed · 2h ago",
  ]);
});

it("says a dismissed Review was dismissed when GitHub no longer has its commit", () => {
  const container = shown(reviewed([review({ state: "DISMISSED", commit: null })]));

  expect(container.querySelector("p.review")?.textContent.trim()).toBe(
    "some-reviewer reviewed, since dismissed · 2h ago",
  );
});

it("folds a hidden comment or Review to its reason, and shows it on a click", async () => {
  shown(
    reviewed(
      [
        review({
          body_html: "<p>Buy now</p>",
          hidden: "spam",
          threads: [thread({ opening: { kind: "hidden", reason: "abuse" } })],
        }),
      ],
      {
        comments: [
          {
            id: 2,
            author: "some-author",
            created_at: "2026-11-04T11:00:00Z",
            body_html: "<p>Unrelated</p>",
            hidden: "off_topic",
          },
          {
            id: 3,
            author: "other-author",
            created_at: "2026-11-04T11:30:00Z",
            body_html: "<p>Who knows</p>",
            hidden: "unstated",
          },
        ],
      },
    ),
  );

  expect(screen.queryByText("Buy now")).toBeNull();
  expect(screen.queryByText("Unrelated")).toBeNull();
  expect(screen.getByRole("listitem").textContent).toContain("Hidden as abuse");
  const offTopic = screen.getByRole("button", { name: "Hidden as off-topic" });
  expect(offTopic.getAttribute("aria-expanded")).toBe("false");

  await fireEvent.click(offTopic);

  expect(screen.getByText("Unrelated")).toBeTruthy();
  expect(offTopic.getAttribute("aria-expanded")).toBe("true");
  expect(screen.queryByText("Buy now")).toBeNull();

  await fireEvent.click(screen.getByRole("button", { name: "Hidden as spam" }));
  await fireEvent.click(screen.getByRole("button", { name: "Hidden" }));

  expect(screen.getByText("Buy now")).toBeTruthy();
  expect(screen.getByText("Who knows")).toBeTruthy();

  await fireEvent.click(offTopic);

  expect(screen.queryByText("Unrelated")).toBeNull();
});

it("says how many earlier comments only GitHub shows, and that it holds older Reviews", () => {
  shown(reviewed([review({ state: "APPROVED" })], { earlier_comments: 1, earlier_reviews: 3 }));

  expect(screen.getByText("1 earlier comment is on GitHub.")).toBeTruthy();
  // GitHub's count takes in the Reviews that only replied in threads, which it doesn't show.
  expect(screen.getByText("Older Reviews are on GitHub.")).toBeTruthy();
});

it("says nothing of Reviews for an Issue", () => {
  const container = shown({
    body_html: "<p>It crashes</p>",
    comments: [],
    earlier_comments: 0,
    events: [],
    earlier_events: 0,
    opened_at: null,
  });

  expect(container.textContent).not.toContain("Review");
});

it("shows the events the Issue view draws among the comments, naming what each changes", () => {
  const container = shown({
    body_html: "<p>It crashes</p>",
    comments: [
      {
        id: 1,
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: "<p>Which build?</p>",
        hidden: null,
      },
    ],
    earlier_comments: 0,
    opened_at: null,
    earlier_events: 0,
    events: [
      {
        actor: "some-author",
        created_at: "2026-11-04T07:00:00Z",
        kind: "labeled",
        label: "type-bug",
      },
      {
        actor: "some-author",
        created_at: "2026-11-04T10:00:00Z",
        kind: "closed",
        completed: true,
      },
      {
        actor: "some-author",
        created_at: "2026-11-04T10:30:00Z",
        kind: "closed",
        completed: false,
      },
      {
        actor: "some-author",
        created_at: "2026-11-04T10:45:00Z",
        kind: "assigned",
        assignee: "some-assignee",
      },
      {
        actor: null,
        created_at: "2026-11-04T11:00:00Z",
        kind: "renamed",
        previous_title: "Old title",
      },
    ],
  });

  const lines = [...container.querySelectorAll(".event, article > header")].map((line) =>
    line.textContent.replace(/\s+/g, " ").trim(),
  );
  expect(lines).toEqual([
    "some-author added the type-bug label · 5h ago",
    "some-reviewer · 3h ago",
    "some-author closed this as completed · 2h ago",
    "some-author closed this as not planned · 1h ago",
    "some-author assigned some-assignee · 1h ago",
    'deleted user renamed this from "Old title" · 1h ago',
  ]);
  // Each event's own time keeps its date and time in the tooltip.
  expect(container.querySelector(".event .muted")?.getAttribute("title")).toBe(
    stampOf("2026-11-04T07:00:00Z"),
  );
});

it("opens a cross-reference, a hand link and a commit as any link does, with its address in the tooltip", async () => {
  const reference = (number: number, title: string, url: string) => ({
    repository: "python/cpython",
    number,
    title,
    url,
  });
  const onlink = vi.fn();
  render(Conversation, {
    now: NOW,
    onlink,
    content: {
      body_html: "",
      comments: [],
      earlier_comments: 0,
      opened_at: null,
      earlier_events: 0,
      events: [
        {
          actor: "some-author",
          created_at: "2026-11-04T09:00:00Z",
          kind: "cross-referenced",
          reference: reference(12, "Fix it", "https://github.com/python/cpython/pull/12"),
        },
        {
          actor: "some-author",
          created_at: "2026-11-04T10:00:00Z",
          kind: "connected",
          reference: reference(13, "Link it", "https://github.com/python/cpython/pull/13"),
        },
        {
          actor: "some-author",
          created_at: "2026-11-04T11:00:00Z",
          kind: "referenced",
          commit: {
            sha: "0123abc",
            message: "Fix the crash",
            url: "https://github.com/python/cpython/commit/0123abcd",
          },
        },
      ],
    },
  });

  expect(
    [...document.querySelectorAll(".event")].map((line) => line.textContent.replace(/\s+/g, " ").trim()),
  ).toEqual([
    "some-author mentioned this in python/cpython#12 Fix it · 3h ago",
    "some-author connected this to python/cpython#13 Link it · 2h ago",
    "some-author added a commit that references this: 0123abc Fix the crash · 1h ago",
  ]);
  const links = screen.getAllByRole("link");
  expect(links.map((link) => [link.textContent, link.getAttribute("href")])).toEqual([
    ["python/cpython#12", "https://github.com/python/cpython/pull/12"],
    ["python/cpython#13", "https://github.com/python/cpython/pull/13"],
    ["0123abc", "https://github.com/python/cpython/commit/0123abcd"],
  ]);
  for (const link of links) {
    expect(link.getAttribute("title")).toBe(`Open in the browser: ${link.getAttribute("href") ?? ""}`);
  }

  await fireEvent.click(links[0]!);
  await fireEvent.click(links[2]!);

  expect(onlink).toHaveBeenNthCalledWith(1, "https://github.com/python/cpython/pull/12");
  expect(onlink).toHaveBeenNthCalledWith(2, "https://github.com/python/cpython/commit/0123abcd");
});

it("shows every event kind in its own words", () => {
  const reference = (number: number, title: string) => ({
    repository: "python/cpython",
    number,
    title,
    url: `https://github.com/python/cpython/pull/${String(number)}`,
  });
  const at = (hour: number, minute = 0) =>
    `2026-11-04T${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}:00Z`;
  const events: IssueEvent[] = [
    { actor: "some-author", created_at: at(0), kind: "cross-referenced", reference: reference(1, "One") },
    {
      actor: "some-author",
      created_at: at(1),
      kind: "referenced",
      commit: {
        sha: "abc1234",
        message: "Fix it",
        url: "https://github.com/python/cpython/commit/abc1234",
      },
    },
    { actor: "some-author", created_at: at(2), kind: "connected", reference: reference(3, "Three") },
    { actor: "some-author", created_at: at(3), kind: "disconnected", reference: reference(4, "Four") },
    { actor: "some-author", created_at: at(4), kind: "labeled", label: "type-bug" },
    { actor: "some-author", created_at: at(5), kind: "unlabeled", label: "type-bug" },
    { actor: "some-author", created_at: at(6), kind: "assigned", assignee: "some-assignee" },
    { actor: "some-author", created_at: at(7), kind: "assigned", assignee: null },
    { actor: "some-author", created_at: at(8), kind: "unassigned", assignee: "some-assignee" },
    { actor: "some-author", created_at: at(9), kind: "unassigned", assignee: null },
    { actor: "some-author", created_at: at(10), kind: "closed", completed: true },
    { actor: "some-author", created_at: at(10, 30), kind: "closed", completed: false },
    { actor: "some-author", created_at: at(11), kind: "reopened" },
    { actor: "some-author", created_at: at(11, 30), kind: "renamed", previous_title: "Old title" },
  ];
  const container = shown({
    body_html: "",
    comments: [],
    earlier_comments: 0,
    opened_at: null,
    earlier_events: 0,
    events,
  });

  const lines = [...container.querySelectorAll(".event")].map((line) =>
    line.textContent.replace(/\s+/g, " ").trim(),
  );
  expect(lines).toEqual([
    "some-author mentioned this in python/cpython#1 One · 12h ago",
    "some-author added a commit that references this: abc1234 Fix it · 11h ago",
    "some-author connected this to python/cpython#3 Three · 10h ago",
    "some-author disconnected this from python/cpython#4 Four · 9h ago",
    "some-author added the type-bug label · 8h ago",
    "some-author removed the type-bug label · 7h ago",
    "some-author assigned some-assignee · 6h ago",
    "some-author assigned deleted user · 5h ago",
    "some-author unassigned some-assignee · 4h ago",
    "some-author unassigned deleted user · 3h ago",
    "some-author closed this as completed · 2h ago",
    "some-author closed this as not planned · 1h ago",
    "some-author reopened this · 1h ago",
    'some-author renamed this from "Old title" · 30m ago',
  ]);
});

it("follows an event's links on a middle click, and only with its button", async () => {
  const onlink = vi.fn();
  render(Conversation, {
    now: NOW,
    onlink,
    content: {
      body_html: "",
      comments: [],
      earlier_comments: 0,
      opened_at: null,
      earlier_events: 0,
      events: [
        {
          actor: "some-author",
          created_at: "2026-11-04T09:00:00Z",
          kind: "cross-referenced",
          reference: {
            repository: "python/cpython",
            number: 12,
            title: "Fix it",
            url: "https://github.com/python/cpython/pull/12",
          },
        },
        {
          actor: "some-author",
          created_at: "2026-11-04T10:00:00Z",
          kind: "referenced",
          commit: {
            sha: "abc1234",
            message: "Fix it",
            url: "https://github.com/python/cpython/commit/abc1234",
          },
        },
      ],
    },
  });
  const [reference, commit] = screen.getAllByRole("link");
  const auxclick = (link: Element, button: number) => {
    link.dispatchEvent(new MouseEvent("auxclick", { bubbles: true, cancelable: true, button }));
  };
  // Another button's auxclick leaves the link alone.
  auxclick(reference!, 0);
  auxclick(commit!, 0);
  auxclick(reference!, 1);
  auxclick(commit!, 1);

  expect(onlink).toHaveBeenNthCalledWith(1, "https://github.com/python/cpython/pull/12");
  expect(onlink).toHaveBeenNthCalledWith(2, "https://github.com/python/cpython/commit/abc1234");
});

it("says how many earlier events only GitHub shows", () => {
  shown({
    body_html: "",
    comments: [],
    earlier_comments: 0,
    opened_at: null,
    events: [],
    earlier_events: 2,
  });

  expect(screen.getByText("2 earlier events are on GitHub.")).toBeTruthy();
  cleanup();
  shown({
    body_html: "",
    comments: [],
    earlier_comments: 0,
    opened_at: null,
    events: [],
    earlier_events: 1,
  });
  expect(screen.getByText("1 earlier event is on GitHub.")).toBeTruthy();
});

it("keeps a hidden comment open through a read that brings a newer comment", async () => {
  const offTopic = {
    id: 4,
    author: "some-author",
    created_at: "2026-11-04T10:00:00Z",
    body_html: "<p>Unrelated</p>",
    hidden: "off_topic",
  } as const;
  const spam = {
    ...offTopic,
    id: 5,
    created_at: "2026-11-04T10:30:00Z",
    body_html: "<p>Buy now</p>",
    hidden: "spam",
  } as const;
  const onlink = vi.fn();
  const { rerender } = render(Conversation, {
    content: reviewed([], { comments: [offTopic, spam] }),
    now: NOW,
    onlink,
  });
  await fireEvent.click(screen.getByRole("button", { name: "Hidden as off-topic" }));

  // The window of recent comments moves on by one, so each comment takes the place of the one before.
  await rerender({
    content: reviewed([], { comments: [spam, { ...spam, created_at: "2026-11-04T11:00:00Z" }] }),
  });

  expect(screen.queryByText("Buy now")).toBeNull();
  await rerender({ content: reviewed([], { comments: [offTopic, spam] }) });
  expect(screen.getByText("Unrelated")).toBeTruthy();
});

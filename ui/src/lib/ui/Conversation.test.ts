import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import type { IssueContent } from "$lib/state/issue-view.svelte";
import type { PullRequestContent, Review, ThreadLine } from "$lib/state/pr-view.svelte";
import Conversation from "./Conversation.svelte";

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
  [...container.querySelectorAll("article > .by")].map((line) => line.textContent.trim());

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

it("offers no link to a thread Files can't place by its line yet", () => {
  const moved = { kind: "line", side: "RIGHT", line: null, original: 7 } as const;
  shown(
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

  expect(screen.queryAllByRole("button")).toEqual([]);
  expect([...document.querySelectorAll(".place")].map((place) => place.textContent)).toEqual([
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
            author: "some-author",
            created_at: "2026-11-04T11:00:00Z",
            body_html: "<p>Unrelated</p>",
            hidden: "off_topic",
          },
          {
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
  const container = shown({ body_html: "<p>It crashes</p>", comments: [], earlier_comments: 0 });

  expect(container.textContent).not.toContain("Review");
});

it("keeps a hidden comment open through a read that brings a newer comment", async () => {
  const offTopic = {
    author: "some-author",
    created_at: "2026-11-04T10:00:00Z",
    body_html: "<p>Unrelated</p>",
    hidden: "off_topic",
  } as const;
  const spam = {
    ...offTopic,
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

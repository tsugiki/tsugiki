import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import type { DiffThread } from "$lib/state/pr-files.svelte";
import ReviewThread from "./ReviewThread.svelte";

const NOW = new Date("2026-11-04T12:00:00Z");

const comment = (changes: Partial<DiffThread["comments"][number]> = {}) => ({
  author: "some-reviewer",
  created_at: "2026-11-04T10:00:00Z",
  body_html: "<p>Why not a set?</p>",
  hidden: null,
  ...changes,
});

const thread = (changes: Partial<DiffThread> = {}): DiffThread => ({
  id: "T1",
  row: 2,
  resolution: "open",
  comments: [comment()],
  later_comments: 0,
  ...changes,
});

const shown = (shown: DiffThread, folded = false) => {
  const ontoggle = vi.fn();
  const onlink = vi.fn();
  render(ReviewThread, { thread: shown, folded, now: NOW, ontoggle, onlink });
  return { ontoggle, onlink };
};

const head = () => screen.getAllByRole("button")[0];

it("shows who started a thread and its comments, oldest first, and folds on a click on its first line", async () => {
  const { ontoggle } = shown(
    thread({ comments: [comment(), comment({ author: null, body_html: "<p>It reads better</p>" })] }),
  );

  expect(head()?.textContent).toBe("▾some-reviewer · 2 comments");
  expect(head()?.getAttribute("aria-expanded")).toBe("true");
  expect([...document.querySelectorAll("article .by")].map((by) => by.textContent)).toEqual([
    "some-reviewer · 2h ago",
    "deleted user · 2h ago",
  ]);
  expect(screen.getByText("It reads better")).toBeTruthy();
  await fireEvent.click(head() as HTMLElement);
  expect(ontoggle).toHaveBeenCalledOnce();
});

it("shows a folded thread as its first line, marked Resolved", () => {
  shown(thread({ resolution: "resolved", later_comments: 2 }), true);

  expect(head()?.textContent).toBe("▸some-reviewer · 3 commentsResolved");
  expect(head()?.getAttribute("aria-label")).toBe("some-reviewer · 3 comments, resolved");
  expect(head()?.getAttribute("aria-expanded")).toBe("false");
  expect(document.querySelector("article")).toBeNull();
});

it("folds a hidden comment to why GitHub hid it, and opens it on a click", async () => {
  shown(thread({ comments: [comment({ hidden: "spam", body_html: "<p>Buy now</p>" })] }));

  const fold = screen.getByRole("button", { name: "Hidden as spam" });
  expect(screen.queryByText("Buy now")).toBeNull();
  await fireEvent.click(fold);
  expect(screen.getByText("Buy now")).toBeTruthy();
  expect(fold.getAttribute("aria-expanded")).toBe("true");
  await fireEvent.click(fold);
  expect(screen.queryByText("Buy now")).toBeNull();
});

it("says how many later comments are on GitHub", () => {
  shown(thread({ later_comments: 1 }));
  expect(screen.getByText("1 later comment is on GitHub.")).toBeTruthy();
});

it("counts the comments on GitHub alone, for a thread Tsugiki holds none of", () => {
  shown(thread({ comments: [], later_comments: 1 }));

  expect(head()?.textContent).toBe("▾deleted user · 1 comment");
  expect(screen.getByText("1 later comment is on GitHub.")).toBeTruthy();
});

it("sends a link in a comment to the browser", async () => {
  const { onlink } = shown(
    thread({ comments: [comment({ body_html: '<p><a href="https://example.com/">a page</a></p>' })] }),
  );

  await fireEvent.click(screen.getByText("a page"));
  expect(onlink).toHaveBeenCalledWith("https://example.com/");
});

it("says how many later comments are on GitHub, two or more", () => {
  shown(thread({ later_comments: 2 }));
  expect(screen.getByText("2 later comments are on GitHub.")).toBeTruthy();
});

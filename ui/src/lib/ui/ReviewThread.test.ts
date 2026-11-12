import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import type { DiffThread } from "$lib/state/pr-files.svelte";
import ReviewThread from "./ReviewThread.svelte";

const NOW = new Date("2026-11-04T12:00:00Z");

const comment = (changes: Partial<DiffThread["comments"][number]> = {}) => ({
  id: null,
  author: "some-reviewer",
  created_at: "2026-11-04T10:00:00Z",
  body_html: "<p>Why not a set?</p>",
  hidden: null,
  submission: "submitted" as const,
  ...changes,
});

const thread = (changes: Partial<DiffThread> = {}): DiffThread => ({
  id: "T1",
  at: { kind: "row", row: 2 },
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
  expect(head()?.getAttribute("aria-label")).toBe("some-reviewer · 3 comments, Resolved");
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

it("says when a thread is on the whole file, Outdated, outside the diff or on a file no longer in it", () => {
  const places: DiffThread["at"][] = [
    { kind: "file", file: 0 },
    { kind: "outdated", file: 0, code: null },
    { kind: "outside", file: 0, line: 80, head_line: 82 },
    { kind: "gone", path: "Lib/old.py", code: null },
  ];
  for (const at of places) shown(thread({ at, resolution: "resolved" }), true);

  expect(screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual([
    "some-reviewer · 1 comment, On the file, Resolved",
    "some-reviewer · 1 comment, Outdated, Resolved",
    "some-reviewer · 1 comment, Line 80, outside the diff, Resolved",
    "some-reviewer · 1 comment, Lib/old.py, Resolved",
  ]);
  expect(head()?.textContent).toBe("▸some-reviewer · 1 commentOn the fileResolved");
});

/** The hunk of `Lib/ast.py` an Outdated thread was written against: 6 rows, the last highlighted. */
const code = {
  rows: [
    { kind: "hunk" as const, text: "@@ -1,3 +1,3 @@" },
    { kind: "context" as const, old: 1, new: 1, text: "x = 1" },
    { kind: "removed" as const, old: 2, text: "y = 2" },
    { kind: "added" as const, new: 2, text: "y = 3" },
    { kind: "note" as const, text: "\\ No newline at end of file" },
    { kind: "context" as const, old: 3, new: 3, text: "def z" },
  ],
  spans: [[], [], [], [], [], [{ start: 0, end: 3, highlight: "keyword" as const }]],
};

it("shows the last rows of the code an Outdated thread was written against, and all of them on Show more", async () => {
  shown(thread({ at: { kind: "outdated", file: 0, code } }));
  const rows = () => [...document.querySelectorAll(".code .row")].map((row) => row.textContent);

  expect(rows()).toEqual(["2-y = 2", "2+y = 3", "\\ No newline at end of file", "33 def z"]);
  expect(document.querySelector(".code .hl-keyword")?.textContent).toBe("def");
  const more = screen.getByRole("button", { name: "Show more" });
  await fireEvent.click(more);
  expect(rows()).toHaveLength(6);
  expect(rows()[0]).toBe("@@ -1,3 +1,3 @@");
  expect(more.getAttribute("aria-expanded")).toBe("true");
  await fireEvent.click(screen.getByRole("button", { name: "Show less" }));
  expect(rows()).toHaveLength(4);
});

it("shows short code whole, with nothing more to show, and the code of a thread on a file no longer in the diff", () => {
  // Rows with no spans, should a payload leave them out, show plain.
  const short = { rows: code.rows.slice(-2), spans: [] };
  shown(thread({ at: { kind: "gone", path: "Lib/ast.py", code: short } }));

  expect([...document.querySelectorAll(".code .row")].map((row) => row.textContent)).toEqual([
    "\\ No newline at end of file",
    "33 def z",
  ]);
  expect(document.querySelector(".code .hl-keyword")).toBeNull();
  expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
});

it("marks a draft from the user's Pending review as theirs", () => {
  shown(thread({ comments: [comment(), comment({ submission: "pending", author: "the-user" })] }));

  expect([...document.querySelectorAll("article .by")].map((by) => by.textContent)).toEqual([
    "some-reviewer · 2h ago",
    "the-user · 2h agoPending, yours",
  ]);
  expect(screen.getByText("Pending, yours").getAttribute("title")).toBe(
    "Only you see it until you submit your review on github.com.",
  );
});

import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { IssueViewState, type IssueView } from "$lib/state/issue-view.svelte";
import type { TreeEntry } from "$lib/state/workbench.svelte";
import IssueDetail from "./IssueDetail.svelte";

const core = vi.hoisted(() => ({
  issue: vi.fn(),
  openIssue: vi.fn(),
  fetchIssue: vi.fn(),
  openOnGithub: vi.fn(),
  openLink: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    issue: core.issue,
    openIssue: core.openIssue,
    fetchIssue: core.fetchIssue,
    openOnGithub: core.openOnGithub,
    openLink: core.openLink,
  },
  events: { changed: { listen: core.listen }, readFailed: { listen: core.listen } },
}));

const row = {
  title: "gh-7: Fix it",
  author: "some-author",
  state: "MERGED" as const,
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS" as const,
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: "APPROVED" as const,
  seen: { mark: { numbers: [], up_to: null }, state: "seen" as const },
};

const opened: TreeEntry = {
  kind: "pr",
  number: 13,
  ...row,
  state: "OPEN",
  your_review: null,
  reasons: [],
  backports: [],
};
const orphan: TreeEntry = { kind: "backport", number: 14, ...row, target_branch: "3.12", backport: "manual" };

const view: IssueView = {
  number: 7,
  title: "It crashes",
  state: "OPEN",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  author: "issue-author",
  assignees: [],
  labels: [],
  needs_attention: [],
  pull_requests: [
    {
      kind: "pr",
      number: 10,
      ...row,
      backports: [
        { number: 11, ...row, target_branch: "3.14", backport: "bot" },
        { number: 12, ...row, state: "OPEN", target_branch: "3.13", backport: "manual" },
      ],
    },
    opened,
    orphan,
  ],
  merged: [],
  merged_count: 0,
  seen: {
    mark: { numbers: [7, 10, 11, 12, 13, 14], up_to: null },
    state: "seen",
  },
  content: {
    body_html: "<p>The description</p>",
    comments: [
      {
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: '<p><a href="https://github.com/python/cpython/issues/1">A link</a></p>',
        hidden: null,
      },
      { author: null, created_at: "2026-11-04T10:00:00Z", body_html: "<p>Gone</p>", hidden: null },
    ],
    earlier_comments: 3,
    opened_at: "2026-10-22T10:00:00Z",
  },
};

const content = (body_html: string, earlier_comments: number) => ({
  body_html,
  comments: [],
  earlier_comments,
  opened_at: null,
});

const NOW = new Date("2026-11-04T11:00:00Z");

const shown = async (answer: IssueView | null, onopenpr = vi.fn(), onmark = vi.fn()) => {
  core.issue.mockResolvedValue({ status: "ok", data: answer });
  const state = new IssueViewState("python/cpython", 7);
  await state.load();
  render(IssueDetail, { repository: "python/cpython", issue: state, now: NOW, onopenpr, onmark });
  state.stop();
  return state;
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(vi.fn());
  core.openIssue.mockResolvedValue({ status: "ok", data: null });
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.openLink.mockResolvedValue({ status: "ok", data: null });
});

it("shows the title and author, the description and the recent comments", async () => {
  await shown(view);

  expect(screen.getByRole("heading", { level: 2 }).textContent).toContain("#7 It crashes");
  expect(screen.getByText("The description").tagName).toBe("P");
  const headers = [...document.querySelectorAll("article > header")].map((header) => header.textContent);
  expect(headers).toEqual(["some-reviewer · 2h ago", "deleted user · 1h ago"]);
  expect(screen.getByText("3 earlier comments are on GitHub.")).toBeTruthy();
});

it("offers Mark Seen in its header while something is new or it is Marked unseen, and Mark Unseen otherwise", async () => {
  const onmark = vi.fn();
  const seen = (state: "seen" | "new_activity" | "marked_unseen") => ({
    ...view,
    seen: { mark: { numbers: [7], up_to: "2026-11-04T10:00:00Z" }, state },
  });

  await shown(seen("seen"), vi.fn(), onmark);
  await fireEvent.click(screen.getByRole("button", { name: "Mark Unseen" }));
  expect(onmark).toHaveBeenCalledOnce();
  cleanup();
  await shown(seen("new_activity"));
  expect(screen.getByRole("button", { name: "Mark Seen" })).toBeTruthy();
  cleanup();
  await shown(seen("marked_unseen"));
  expect(screen.getByRole("button", { name: "Mark Seen" })).toBeTruthy();
});

/** The line under the title, and the part of it that names who opened the Issue, in the text colour. */
const authorLine = () => {
  const author = screen.getByText((_, element) => element?.matches("p.by b") ?? false);
  return { author, opened: author.parentElement as HTMLElement, line: author.closest("p") as HTMLElement };
};

it("says under the title who opened the Issue and when, and how many comments it has", async () => {
  await shown(view);

  const { author, opened, line } = authorLine();
  const when = new Date("2026-10-22T10:00:00Z").toLocaleString([], {
    dateStyle: "medium",
    timeStyle: "short",
  });
  expect(author.textContent).toBe("issue-author");
  expect(opened.getAttribute("title")).toBe(when);
  expect(line.textContent).toBe("issue-author opened this 13d ago · 5 comments");
});

it("says whom the Issue is assigned to", async () => {
  await shown({ ...view, assignees: ["some-assignee", "other-assignee"] });

  expect(authorLine().line.textContent).toBe(
    "issue-author opened this 13d ago · 5 comments · assigned to some-assignee, other-assignee",
  );
});

it("shows the Issue's labels as chips in the order the core gives", async () => {
  await shown({ ...view, labels: ["3.14", "type-bug"] });

  const labels = screen.getByRole("list", { name: "Labels" });
  const chips = within(labels).getAllByRole("listitem");
  expect(chips.map((chip) => chip.textContent)).toEqual(["3.14", "type-bug"]);
  expect(chips.every((chip) => chip.classList.contains("chip"))).toBe(true);
});

it("shows no row of chips for an Issue with no labels", async () => {
  await shown(view);

  expect(screen.queryByRole("list", { name: "Labels" })).toBeNull();
  expect(screen.getByRole("button", { name: "Mark Unseen" })).toBeTruthy();
});

it("counts one comment as one, and leaves out when a read from before Tsugiki asked was opened", async () => {
  await shown({ ...view, content: content("<p>Short</p>", 1) });

  const { opened, line } = authorLine();
  expect(opened.getAttribute("title")).toBeNull();
  expect(line.textContent).toBe("issue-author · 1 comment");
  expect(screen.getByText("1 earlier comment is on GitHub.")).toBeTruthy();
});

it("says nothing about earlier comments when it shows them all", async () => {
  await shown({ ...view, content: content("<p>Short</p>", 0) });

  expect(screen.queryByText(/earlier comment/)).toBeNull();
});

it("shows each Origin PR's state, CI and review state, and folds its Backports into one line", async () => {
  await shown(view);

  const words = [...document.querySelectorAll(".words")].map((line) => line.textContent);
  expect(words.filter((line) => line === "Merged · CI passed · You approved")).toHaveLength(2);
  expect(words).toContain("Open · CI passed");
  const backports = screen.getByText(/Backports:/);
  expect(backports.textContent).toContain("3.14 Merged");
  expect(backports.textContent).toContain("3.13 Open");
});

/** The heading over the tree. The Conversation's headings follow it. */
const treeHeading = () => screen.getByRole("heading", { level: 3, name: /^Origin PRs/ }).textContent;

it("counts the open PRs of all the PRs in the tree, Backports included", async () => {
  await shown(view);

  expect(treeHeading()).toBe("Origin PRs 2 open of 5 PRs");
});

it("counts one PR as one", async () => {
  await shown({ ...view, pull_requests: [orphan] });

  expect(treeHeading()).toBe("Origin PRs 0 open of 1 PR");
});

it("starts each PR's line with its branch and ends it with its author", async () => {
  await shown({ ...view, pull_requests: [opened, { ...orphan, author: null }] });

  const lines = within(screen.getByRole("list", { name: "PRs" }))
    .getAllByRole("listitem")
    .map((item) => item.firstElementChild?.textContent?.trim());
  expect(lines).toEqual([
    "main #13 gh-7: Fix it some-author Open · CI passed",
    "3.12 #14 gh-7: Fix it deleted user Merged · CI passed · You approved",
  ]);
});

it("shows in bold each PR and Backport that Needs attention, and only those", async () => {
  await shown({ ...view, needs_attention: [12, 13] });

  const bold = (name: string) => screen.getByRole("button", { name }).classList.contains("attention");

  expect(bold("#13 gh-7: Fix it")).toBe(true);
  expect(bold("3.13 Open")).toBe(true);
  expect(bold("#10 gh-7: Fix it")).toBe(false);
  expect(bold("3.14 Merged")).toBe(false);
});

it("says under the tree what each icon and the bold mean", async () => {
  await shown(view);

  const legend = screen.getByText("Needs attention", { exact: false });
  const icons = within(legend.parentElement as HTMLElement).getAllByRole("img", { hidden: true });

  expect(legend.parentElement?.textContent).toBe("OpenDraftMergedClosed Bold: Needs attention");
  expect(icons.map((icon) => icon.getAttribute("title"))).toEqual([
    "Open PR",
    "Draft PR",
    "Merged PR",
    "Closed PR",
  ]);
});

/** The Issue with two Origin PRs that merged with their Backport, three PRs in all, folded away. */
const withMerged: IssueView = {
  ...view,
  merged: [
    {
      kind: "pr",
      number: 20,
      ...row,
      title: "gh-7: First fix",
      backports: [{ number: 21, ...row, target_branch: "3.14", backport: "bot" }],
    },
    { kind: "pr", number: 22, ...row, title: "gh-7: Second fix", backports: [] },
  ],
  merged_count: 3,
};

it("counts the merged PRs folded away in the heading, and gives an Issue with only those its heading", async () => {
  await shown(withMerged);
  expect(treeHeading()).toBe("Origin PRs 2 open of 8 PRs");
  cleanup();

  await shown({ ...withMerged, pull_requests: [] });
  expect(treeHeading()).toBe("Origin PRs 0 open of 3 PRs");
});

it("folds the fully merged PRs into one line that unfolds and folds them again", async () => {
  await shown(withMerged);

  const line = screen.getByRole("button", { name: "Show 3 merged PRs" });
  expect(line.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("list", { name: "Merged PRs" })).toBeNull();

  await fireEvent.click(line);
  const merged = screen.getByRole("list", { name: "Merged PRs" });
  expect(within(merged).getByRole("button", { name: "#20 gh-7: First fix" })).toBeTruthy();
  expect(within(merged).getByText(/Backports:/).textContent).toContain("3.14 Merged");
  expect(line.textContent?.trim()).toBe("Hide 3 merged PRs");
  expect(line.getAttribute("aria-expanded")).toBe("true");

  await fireEvent.click(line);
  expect(screen.queryByRole("list", { name: "Merged PRs" })).toBeNull();
  expect(line.textContent?.trim()).toBe("Show 3 merged PRs");
});

it("shows only the line for an Issue whose PRs have all merged, and names one PR as one", async () => {
  await shown({ ...withMerged, pull_requests: [], merged: withMerged.merged.slice(1), merged_count: 1 });

  expect(screen.queryByRole("list", { name: "PRs" })).toBeNull();
  expect(screen.getByRole("button", { name: "Show 1 merged PR" })).toBeTruthy();
});

it("leaves the line out when nothing has fully merged", async () => {
  await shown(view);

  expect(screen.queryByRole("button", { name: /merged PR/ })).toBeNull();
});

it("marks the Issue, each PR in its tree and each Backport with the icon of its state", async () => {
  await shown({ ...view, state: "CLOSED" });

  const heading = screen.getByRole("heading", { level: 2 });
  const tree = screen.getByRole("list", { name: "PRs" });
  const icons = within(tree)
    .getAllByRole("img")
    .map((icon) => icon.getAttribute("title"));

  expect(heading.querySelector("[role=img]")?.getAttribute("title")).toBe("Closed issue");
  expect(icons).toEqual(["Merged PR", "Merged PR", "Open PR", "Open PR", "Merged PR"]);
});

it("leaves the icon out for an Issue Tsugiki stored before it kept states", async () => {
  await shown({ ...view, state: null });

  expect(within(screen.getByRole("heading", { level: 2 })).queryByRole("img")).toBeNull();
});

it("opens the title on GitHub", async () => {
  await shown(view);

  await fireEvent.click(screen.getByRole("button", { name: "#7 It crashes" }));

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", { kind: "issue", number: 7 });
});

it("opens a PR row and a Backport in the PR view", async () => {
  const onopenpr = vi.fn();
  await shown(view, onopenpr);

  await fireEvent.click(screen.getByRole("button", { name: "#10 gh-7: Fix it" }));
  await fireEvent.click(screen.getByRole("button", { name: "3.13 Open" }));
  await fireEvent.click(screen.getByRole("button", { name: "#14 gh-7: Fix it" }));

  expect(onopenpr.mock.calls).toEqual([[10], [12], [14]]);
  expect(core.openOnGithub).not.toHaveBeenCalled();
});

it("sends a link in a comment to the browser, and says if that fails", async () => {
  core.openLink.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  await shown(view);

  await fireEvent.click(screen.getByRole("link", { name: "A link" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith("https://github.com/python/cpython/issues/1");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Tsugiki couldn't open the system browser.",
  );
});

it("sends a link in the description to the browser", async () => {
  await shown({
    ...view,
    content: content('<a href="https://github.com/">Home</a>', 0),
  });

  await fireEvent.click(screen.getByRole("link", { name: "Home" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith("https://github.com/");
});

it("shows the tree before Tsugiki has read what the Issue says", async () => {
  await shown({ ...view, content: null, author: null, assignees: ["some-assignee"], pull_requests: [] });

  expect(screen.getByText(/hasn't read what this Issue says/)).toBeTruthy();
  expect(authorLine().line.textContent).toBe("deleted user · assigned to some-assignee");
  expect(screen.queryByRole("list")).toBeNull();
});

it("says so for an Issue Tsugiki doesn't hold", async () => {
  await shown(null);

  expect(screen.getByText("Tsugiki doesn't hold Issue #7.")).toBeTruthy();
});

// jsdom doesn't keep `scrollTop`, so `e2e/issue-view.spec.ts` checks that the pane comes back there.
it("keeps how far down the user scrolled its pane", async () => {
  const state = await shown(view);
  const pane = screen.getByRole("region", { name: "Issue view" });
  Object.defineProperty(pane, "scrollTop", { value: 300 });

  await fireEvent.scroll(pane);

  expect(state.scrollTop).toBe(300);
});

it("says why it can't show the Issue", async () => {
  core.issue.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  const state = new IssueViewState("python/cpython", 7);
  await state.load();
  render(IssueDetail, {
    repository: "python/cpython",
    issue: state,
    now: NOW,
    onopenpr: vi.fn(),
    onmark: vi.fn(),
  });
  state.stop();

  expect(screen.getByRole("alert").textContent).toContain("Tsugiki can't show this Issue: no database");
});

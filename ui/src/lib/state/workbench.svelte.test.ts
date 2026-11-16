import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events, type ListOrder } from "$lib/data/bindings";
import { WorkbenchState, type ListedItem, type Seen, type WorkbenchRow } from "./workbench.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { workbench: vi.fn(), markSeen: vi.fn(), markUnseen: vi.fn(), putFilterOnWorkbench: vi.fn() },
  events: { changed: { listen: vi.fn() } },
}));

/** The mark that Mark Seen sends for `numbers`, with nothing new on them. */
const seen = (...numbers: number[]): Seen => ({
  mark: { numbers, up_to: null },
  state: "seen",
});

const item = (number: number): ListedItem => ({
  attention: null,
  kind: "standalone_pr",
  number,
  title: `PR ${String(number)}`,
  author: "octocat",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: ["own_pr"],
  your_review: { kind: "unreviewed" },
  seen: seen(number),
  backports: [],
});

/** A PR in an Issue's tree. */
const row: WorkbenchRow = {
  number: 0,
  title: "PR",
  author: "octocat",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: null,
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: { kind: "unreviewed" },
  seen: seen(0),
};

const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 1, problem: null };
const whole = { kind: "whole" } as const;
const onlyMine = { kind: "filter", query: mine.query } as const;

const answer = (...numbers: number[]) => ({
  status: "ok" as const,
  data: {
    items: numbers.map(item),
    count: numbers.length,
    review_requests: 0,
    stale_reviews: 0,
    filters: [mine],
  },
});

const refused = { status: "error" as const, error: { kind: "internal" as const, message: "no database" } };

/** Catches the listener the state registers, so a test can say a Repository changed as the core would, and
 * which items. */
const catchListener = () => {
  const stop = vi.fn();
  const send = vi.fn();
  vi.mocked(events.changed.listen).mockImplementation(async (callback) => {
    send.mockImplementation((repository: string, items: number[] = []) => {
      callback({ event: "changed", id: 1, payload: { repository, items } });
    });
    return stop;
  });
  return { stop, send };
};

const numbers = (state: WorkbenchState) => state.items?.map((i) => i.number);

/** Issue `number` with one PR, `pr`, in its tree. */
const issueWith = (number: number, pr: number): ListedItem => ({
  attention: null,
  kind: "issue",
  number,
  title: `Issue ${String(number)}`,
  state: "OPEN",
  people: [],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ ...row, kind: "pr", number: pr, seen: seen(pr), backports: [] }],
  seen: seen(number, pr),
});

describe("WorkbenchState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("moves the cursor through the list, and stops at each end", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(3, 2, 1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.underCursor).toBeUndefined();

    state.moveCursor(-1);
    expect(state.cursor?.number).toBe(3);
    state.moveCursor(-1);
    expect(state.cursor?.number).toBe(3);
    state.moveCursor(1);
    state.moveCursor(1);
    expect(state.underCursor?.number).toBe(1);
    state.moveCursor(1);
    expect(state.cursor?.number).toBe(1);
    state.stop();
  });

  it("follows the item under the cursor to its new line when a change moves it down the list", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(3, 2));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveCursor(1);
    vi.mocked(commands.workbench).mockResolvedValue(answer(2, 3));

    send("python/cpython");
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2, 3]);
    });

    expect(state.underCursor?.number).toBe(3);
    state.stop();
  });

  it("says whether the list has a line", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(3, 2));
    const state = new WorkbenchState("python/cpython");
    const pr = (number: number, issue?: number) => ({ number, kind: "pull_request" as const, issue });
    const before = state.lists(pr(3));

    await state.load();

    expect([
      before,
      state.lists(pr(3)),
      state.lists(pr(2)),
      state.lists(pr(3, 1)),
      state.lists(pr(4)),
    ]).toEqual([false, true, true, false, false]);
    state.stop();
  });

  it("knows the title of each line it lists", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(3, 2));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    expect([state.titleOf(3), state.titleOf(2), state.titleOf(4)]).toEqual(["PR 3", "PR 2", undefined]);
    state.stop();
  });

  it("names the view on show as the Sidebar does, and a Filter it no longer has by its search", async () => {
    catchListener();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [mine] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();

    const titles = [state.title];
    for (const view of [
      { kind: "review_requests" as const },
      { kind: "stale_reviews" as const },
      { kind: "filter" as const, query: mine.query },
      { kind: "filter" as const, query: "label:gone" },
    ]) {
      await state.select(view);
      titles.push(state.title);
    }

    expect(titles).toEqual(["Workbench", "Review requests", "Stale reviews", "Mine", "label:gone"]);
    state.stop();
  });

  it("stops on each PR and Backport in an Issue's tree, and knows the Issue", async () => {
    catchListener();
    const tree: ListedItem = {
      attention: null,
      kind: "issue",
      number: 10,
      title: "Issue 10",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      seen: seen(10, 11, 12, 13),
      pull_requests: [
        { ...row, kind: "pr", number: 11, backports: [{ ...row, number: 12, backport: "bot" }] },
        { ...row, kind: "backport", number: 13, backport: "manual" },
      ],
    };
    const standalone = { ...item(20), backports: [{ ...row, number: 21, backport: "bot" as const }] };
    const lone: ListedItem = {
      ...row,
      kind: "standalone_backport",
      number: 30,
      backport: "manual",
      attention: null,
    };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [tree, standalone, lone],
        count: 3,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const walk = (steps: number) =>
      Array.from({ length: steps }, () => {
        state.moveCursor(1);
        return state.underCursor;
      });

    const folded = walk(4).map((stop) => stop?.number);
    state.cursor = undefined;
    state.unfold(10);
    const stops = walk(7);

    expect(folded).toEqual([10, 20, 21, 30]);
    expect(stops).toEqual([
      { number: 10, kind: "issue", issue: undefined },
      { number: 11, kind: "pull_request", issue: 10 },
      { number: 12, kind: "pull_request", issue: 10 },
      { number: 13, kind: "pull_request", issue: 10 },
      { number: 20, kind: "pull_request", issue: undefined },
      { number: 21, kind: "pull_request", issue: undefined },
      { number: 30, kind: "pull_request", issue: undefined },
    ]);
    state.stop();
  });

  it("passes a PR that fixes two Issues once under each", async () => {
    catchListener();
    const tree = (number: number): ListedItem => ({
      attention: null,
      kind: "issue",
      number,
      title: `Issue ${String(number)}`,
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [{ ...row, kind: "pr", number: 5, backports: [] }],
      seen: seen(number, 5),
    });
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [tree(10), tree(20), item(30)],
        count: 3,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.unfold(10);
    state.unfold(20);

    const stops = [];
    for (let step = 0; step < 5; step += 1) {
      state.moveCursor(1);
      stops.push([state.underCursor?.number, state.underCursor?.issue]);
    }

    expect(stops).toEqual([
      [10, undefined],
      [5, 10],
      [20, undefined],
      [5, 20],
      [30, undefined],
    ]);
    state.stop();
  });

  it("starts each Issue folded, and keeps what each view unfolds for itself", async () => {
    catchListener();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [issueWith(10, 11)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [mine],
      },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const pr = { number: 11, kind: "pull_request" as const, issue: 10 };

    const first = [state.unfolded(10), state.lists(pr)];
    state.unfold(10);
    state.unfold(10);
    await state.select({ kind: "filter", query: mine.query });
    const inFilter = state.unfolded(10);
    state.fold(10);
    await state.select({ kind: "whole" });

    expect([...first, inFilter, state.unfolded(10)]).toEqual([false, true, false, true]);
    state.stop();
  });

  it("moves the cursor from a PR to its Issue as the Issue folds, and unfolds it to put the cursor on a PR", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [issueWith(10, 11), issueWith(20, 21)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const pr = { number: 11, kind: "pull_request" as const, issue: 10 };

    state.moveTo(pr);
    const unfolded = [state.unfolded(10), state.underCursor];
    state.fold(20);
    const otherFolded = state.cursor;
    state.fold(10);

    expect(unfolded).toEqual([true, pr]);
    expect(otherFolded).toEqual(pr);
    expect([state.unfolded(10), state.cursor]).toEqual([
      false,
      { number: 10, kind: "issue", issue: undefined },
    ]);
    state.stop();
  });

  it("keeps the folds of the items on show until the newly selected view's answer arrives", async () => {
    catchListener();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    const listed = {
      items: [issueWith(10, 11)],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [mine],
    };
    vi.mocked(commands.workbench).mockResolvedValueOnce({ status: "ok", data: listed });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.unfold(10);
    let answer: (value: Awaited<ReturnType<typeof commands.workbench>>) => void = () => {};
    vi.mocked(commands.workbench).mockReturnValueOnce(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );

    const selecting = state.select({ kind: "filter", query: mine.query });
    const meanwhile = state.unfolded(10);
    answer({ status: "ok", data: listed });
    await selecting;

    expect([meanwhile, state.unfolded(10)]).toEqual([true, false]);
    state.stop();
  });

  it("unfolds the Issue over the PR under the cursor in a view that has it folded", async () => {
    catchListener();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [issueWith(10, 11)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [mine],
      },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveTo({ number: 11, kind: "pull_request", issue: 10 });

    await state.select({ kind: "filter", query: mine.query });

    expect([state.unfolded(10), state.underCursor?.number]).toEqual([true, 11]);
    state.stop();
  });

  it("follows a PR to its new line when a change puts it under an Issue", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(5, 6));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveCursor(1);
    const tree: ListedItem = {
      attention: null,
      kind: "issue",
      number: 10,
      title: "Issue 10",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [{ ...row, kind: "pr", number: 5, backports: [] }],
      seen: seen(10, 5),
    };
    vi.mocked(commands.workbench).mockResolvedValueOnce({
      status: "ok",
      data: {
        items: [tree, item(6)],
        count: 2,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });

    send("python/cpython");

    await vi.waitFor(() => {
      expect(state.cursor).toEqual({ number: 5, kind: "pull_request", issue: 10 });
    });
    state.moveCursor(1);
    expect(state.cursor?.number).toBe(6);
    state.stop();
  });

  it("starts the cursor over when a change takes its item off the list", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(5, 6));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveCursor(1);
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(6, 7));

    send("python/cpython");
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([6, 7]);
    });

    expect(state.underCursor).toBeUndefined();
    state.moveCursor(1);
    expect(state.cursor?.number).toBe(6);
    state.stop();
  });

  it("starts the cursor over when its item has left the list", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(2, 1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.cursor = { number: 99, kind: "issue", issue: undefined };

    state.moveCursor(1);

    expect(state.cursor?.number).toBe(2);
    state.stop();
  });

  it("has no cursor to move before the list arrives", () => {
    const state = new WorkbenchState("python/cpython");

    state.moveCursor(1);

    expect(state.cursor).toBeUndefined();
  });

  it("holds what the core lists for its Repository", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(2, 1));
    const state = new WorkbenchState("python/cpython");
    expect(state.items).toBeUndefined();
    await state.load();
    expect(commands.workbench).toHaveBeenCalledWith("python/cpython", whole);
    expect(numbers(state)).toEqual([2, 1]);
    expect(state.count).toBe(2);
    expect(state.filters).toEqual([mine]);
    expect(state.error).toBeUndefined();
  });

  it("holds the counts of the Built-in views", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce({
      status: "ok",
      data: { items: [item(4)], count: 0, review_requests: 3, stale_reviews: 2, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();

    expect(state.reviewRequests).toBe(3);
    expect(state.staleReviews).toBe(2);
  });

  it("asks for a Filter the user selects, and for the whole Workbench again", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(2, 1));
    const state = new WorkbenchState("python/cpython");
    expect(state.selected).toEqual(whole);
    await state.load();

    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(1));
    await state.select(onlyMine);
    expect(state.selected).toEqual(onlyMine);
    expect(commands.workbench).toHaveBeenLastCalledWith("python/cpython", onlyMine);
    expect(numbers(state)).toEqual([1]);

    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(2, 1));
    await state.select(whole);
    expect(commands.workbench).toHaveBeenLastCalledWith("python/cpython", whole);
    expect(numbers(state)).toEqual([2, 1]);
  });

  it("knows the line of the selected Filter, and of no other view", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.selectedFilter).toBeUndefined();

    await state.select(onlyMine);
    expect(state.selectedFilter).toEqual(mine);
    await state.select({ kind: "filter", query: "label:gone" });
    expect(state.selectedFilter).toBeUndefined();
  });

  it("puts a Filter on the Workbench and asks again, or says why it couldn't", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    await state.select(onlyMine);
    vi.mocked(commands.workbench).mockClear();

    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValueOnce({ status: "ok", data: null });
    expect(await state.putOnWorkbench(mine.query, false)).toBeUndefined();
    expect(commands.putFilterOnWorkbench).toHaveBeenLastCalledWith({
      repository: "python/cpython",
      query: mine.query,
      on: false,
    });
    expect(commands.workbench).toHaveBeenCalledTimes(1);

    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValueOnce(refused);
    expect(await state.putOnWorkbench(mine.query, true)).toBe("no database");
    expect(state.switchProblem).toBe("no database");
    expect(commands.workbench).toHaveBeenCalledTimes(2);

    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValueOnce({ status: "ok", data: null });
    await state.putOnWorkbench(mine.query, true);
    expect(state.switchProblem).toBeUndefined();
  });

  it("says a Filter's switch is saving until the save and the new answer are in", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const save = Promise.withResolvers<{ status: "ok"; data: null }>();
    vi.mocked(commands.putFilterOnWorkbench).mockReturnValueOnce(save.promise);

    const switching = state.putOnWorkbench(mine.query, false);
    expect(state.switching).toBe(true);
    save.resolve({ status: "ok", data: null });
    await switching;
    expect(state.switching).toBe(false);

    vi.mocked(commands.putFilterOnWorkbench).mockRejectedValueOnce(new Error("gone"));
    await expect(state.putOnWorkbench(mine.query, false)).rejects.toThrow("gone");
    expect(state.switching).toBe(false);
  });

  it("keeps a selected Filter when the facts change", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    await state.select(onlyMine);

    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(3, 1));
    send("python/cpython");

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([3, 1]);
    });
    expect(commands.workbench).toHaveBeenLastCalledWith("python/cpython", onlyMine);
  });

  it("drops the answer for a line the user has already left", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(2, 1));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    const slow = Promise.withResolvers<ReturnType<typeof answer>>();
    vi.mocked(commands.workbench).mockReturnValueOnce(slow.promise);
    const leaving = state.select(onlyMine);
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(2, 1));
    await state.select(whole);
    slow.resolve(answer(1));
    await leaving;

    expect(numbers(state)).toEqual([2, 1]);
  });

  it("listens before it asks, so no change is lost in between", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    await new WorkbenchState("python/cpython").load();
    const listened = vi.mocked(events.changed.listen).mock.invocationCallOrder[0];
    const asked = vi.mocked(commands.workbench).mock.invocationCallOrder[0];
    expect(listened).toBeLessThan(asked as number);
  });

  it("asks again when its Repository changes", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(2, 1));
    send("python/cpython");

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2, 1]);
    });
  });

  it("doesn't ask when another Repository changes", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    send("tsugiki/tsugiki");
    expect(commands.workbench).toHaveBeenCalledOnce();
  });

  it("keeps the newest answer when an older one arrives after it", async () => {
    const { send } = catchListener();
    const first = Promise.withResolvers<ReturnType<typeof answer>>();
    vi.mocked(commands.workbench).mockReturnValueOnce(first.promise);
    const state = new WorkbenchState("python/cpython");
    const loading = state.load();
    await vi.waitFor(() => {
      expect(commands.workbench).toHaveBeenCalledOnce();
    });

    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(2));
    send("python/cpython");
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2]);
    });

    first.resolve(answer(1));
    await loading;
    expect(numbers(state)).toEqual([2]);
  });

  it("says why the core refused, and keeps the items it has", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    vi.mocked(commands.workbench).mockResolvedValueOnce(refused);
    send("python/cpython");

    await vi.waitFor(() => {
      expect(state.error).toBe("no database");
    });
    expect(numbers(state)).toEqual([1]);
  });

  it("forgets a refusal when the next answer arrives", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce(refused);
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.error).toBe("no database");

    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(1));
    send("python/cpython");

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([1]);
    });
    expect(state.error).toBeUndefined();
  });

  it("says so when a question breaks", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockRejectedValue(new Error("the core is gone"));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.error).toBe("Error: the core is gone");
  });

  it("says so when it can't listen, since its items would never change", async () => {
    vi.mocked(events.changed.listen).mockRejectedValue(new Error("no events"));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.error).toBe("Error: no events");
    expect(commands.workbench).not.toHaveBeenCalled();
  });

  it("stops listening when stopped", async () => {
    const { stop } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.stop();
    expect(stop).toHaveBeenCalledOnce();
  });

  it("stops listening and asks nothing if stopped before it could listen", async () => {
    const { stop } = catchListener();
    const state = new WorkbenchState("python/cpython");
    const loading = state.load();
    state.stop();
    await loading;
    expect(stop).toHaveBeenCalledOnce();
    expect(commands.workbench).not.toHaveBeenCalled();
  });

  it("has nothing to stop if it never loaded", () => {
    new WorkbenchState("python/cpython").stop();
  });

  it("finds each line's part in Mark Seen: an Issue's covers its tree, and a PR's the PR alone", async () => {
    catchListener();
    const standalone = {
      ...item(3),
      backports: [{ ...row, number: 4, seen: seen(4), backport: "bot" as const }],
    };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [issueWith(1, 2), standalone],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    const state = new WorkbenchState("python/cpython");
    const numbersOf = (number: number, kind: "issue" | "pull_request", issue?: number) =>
      state.seenOf({ number, kind, issue })?.mark.numbers;
    expect(numbersOf(1, "issue"), "before the first answer").toBeUndefined();
    await state.load();

    expect(numbersOf(1, "issue")).toEqual([1, 2]);
    expect(numbersOf(2, "pull_request", 1)).toEqual([2]);
    expect(numbersOf(3, "pull_request")).toEqual([3]);
    expect(numbersOf(4, "pull_request")).toEqual([4]);
    expect(numbersOf(9, "issue")).toBeUndefined();
    state.stop();
  });

  it("sends Mark Seen and Mark Unseen to the core, and says what went wrong", async () => {
    vi.mocked(commands.markSeen).mockResolvedValue({ status: "ok", data: null });
    vi.mocked(commands.markUnseen).mockResolvedValue(refused);
    const state = new WorkbenchState("python/cpython");
    const mark = { numbers: [1, 2], up_to: "2026-11-04T10:00:00Z" };

    expect(await state.markSeen(mark)).toBeUndefined();
    expect(await state.markUnseen(7)).toBe("no database");

    expect(commands.markSeen).toHaveBeenCalledWith("python/cpython", mark);
    expect(commands.markUnseen).toHaveBeenCalledWith("python/cpython", 7);
  });

  /** Item `number`, with a Review request waiting on the user. */
  const waiting = (number: number): ListedItem => ({
    ...item(number),
    attention: { since: "2026-11-04T10:00:00Z", why: [{ kind: "review_requested" }] },
  });

  /** The list the core answers: the items of `waits` Need attention, and those of `rest` below them don't. */
  const answering = (waits: number[], rest: number[] = []) => {
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [...waits.map(waiting), ...rest.map(item)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
  };

  /** A list of the waiting `items` with the cursor on its line `at`, counted from the top, what it calls
   * each time the cursor lands, and the order the window holds, which a test may change. */
  const listingWithCursor = async (items: number[], at: number) => {
    const { send } = catchListener();
    answering(items);
    const landed = vi.fn();
    const order = { now: "attention" as ListOrder };
    const state = new WorkbenchState("python/cpython", landed, () => order.now);
    await state.load();
    for (let step = 0; step <= at; step++) state.moveCursor(1);
    return { state, send, landed, order };
  };

  it("lands the cursor on the line that takes the place of the item under it once it stops waiting", async () => {
    const { state, send, landed } = await listingWithCursor([4, 3, 2, 1], 1);

    // A sync that left the line waiting.
    send("python/cpython");
    await vi.waitFor(() => {
      expect(commands.workbench).toHaveBeenCalledTimes(2);
    });
    expect(state.cursor?.number).toBe(3);
    answering([4, 2, 1], [3]);
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(state.cursor?.number).toBe(2);
    });
    expect(landed).toHaveBeenCalledOnce();
    state.stop();
  });

  it("lands the cursor with nobody told when it was made without a callback", async () => {
    const { send } = catchListener();
    answering([3, 2]);
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveCursor(1);

    answering([2], [3]);
    send("python/cpython");

    await vi.waitFor(() => {
      expect(state.cursor?.number).toBe(2);
    });
    state.stop();
  });

  it("lands the cursor on the item that came after the one under it, though one above dropped down too", async () => {
    const { state, send } = await listingWithCursor([4, 3, 2, 1], 1);

    answering([2, 1], [4, 3]);
    send("python/cpython", [3, 4]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2, 1, 4, 3]);
    });
    expect(state.cursor?.number).toBe(2);
    state.stop();
  });

  it("lands the cursor on the last line, under an unfolded Issue, when the item under it was the last and left the list", async () => {
    const { send } = catchListener();
    const listing = (...items: ListedItem[]) => ({
      status: "ok" as const,
      data: { items, count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
    });
    const issue = { ...issueWith(7, 8), attention: waiting(7).attention };
    vi.mocked(commands.workbench).mockResolvedValue(listing(issue, waiting(3)));
    const landed = vi.fn();
    const state = new WorkbenchState("python/cpython", landed);
    await state.load();
    state.unfold(7);
    state.moveTo({ number: 3, kind: "pull_request", issue: undefined });

    vi.mocked(commands.workbench).mockResolvedValue(listing(issue));
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(landed).toHaveBeenCalledOnce();
    });
    expect(state.cursor).toEqual({ number: 8, kind: "pull_request", issue: 7 });
    state.stop();
  });

  it("lands the cursor on the item that came after the one under it in an order that leaves it in place", async () => {
    const { state, send, landed } = await listingWithCursor([3, 2, 1], 0);

    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [item(3), waiting(2), waiting(1)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(landed).toHaveBeenCalledOnce();
    });
    expect(state.cursor?.number, "by last update, the item keeps its line").toBe(2);
    state.stop();
  });

  it("lands the cursor on the item after the one under it when the item stops waiting and leaves the list", async () => {
    const { state, send, landed } = await listingWithCursor([4, 3, 2, 1], 1);

    answering([4, 2, 1]);
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(state.cursor?.number).toBe(2);
    });
    expect(landed).toHaveBeenCalledOnce();
    state.stop();
  });

  it("neither lands the cursor nor says so when the item under it leaves the list, and nothing else is on it", async () => {
    const { state, send, landed } = await listingWithCursor([3], 0);

    answering([]);
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([]);
    });
    expect(landed).not.toHaveBeenCalled();
    expect(state.underCursor).toBeUndefined();
    state.stop();
  });

  it("follows the item under the cursor when it still waits after a change", async () => {
    const { state, send, landed } = await listingWithCursor([4, 3, 2, 1], 1);

    // A Review request still waits on the user, so the item stays, if no longer where it was.
    answering([3, 4, 2, 1]);
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([3, 4, 2, 1]);
    });
    expect(state.cursor?.number).toBe(3);
    expect(landed).not.toHaveBeenCalled();
    state.stop();
  });

  it("leaves the cursor alone when the item that stops waiting isn't under it", async () => {
    const { state, send, landed } = await listingWithCursor([3, 2, 1], 0);

    answering([3, 1], [2]);
    send("python/cpython", [2]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([3, 1, 2]);
    });
    expect(state.cursor?.number).toBe(3);
    expect(landed).not.toHaveBeenCalled();
    state.stop();
  });

  it("leaves the cursor in place when the item under it, with nothing waiting, changes or leaves the list", async () => {
    const { send } = catchListener();
    answering([], [3, 2, 1]);
    const landed = vi.fn();
    const state = new WorkbenchState("python/cpython", landed);
    await state.load();
    state.moveCursor(1);

    // Mark Seen on #3, and the same sync took #1 off the list, which says the answer has arrived.
    answering([], [3, 2]);
    send("python/cpython", [3]);
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([3, 2]);
    });
    expect(state.cursor?.number).toBe(3);
    answering([], [2]);
    send("python/cpython", [3]);
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2]);
    });

    expect(state.cursor?.number, "as before this change, it starts over at the next press").toBe(3);
    expect(state.underCursor).toBeUndefined();
    expect(landed).not.toHaveBeenCalled();
    state.stop();
  });

  it("lands the cursor on the item after the one under it, and stays there while that one waits", async () => {
    const { state, send, landed } = await listingWithCursor([4, 3, 2, 1], 1);

    answering([4, 2, 1], [3]);
    send("python/cpython");

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([4, 2, 1, 3]);
    });
    expect(state.cursor?.number).toBe(2);
    expect(landed).toHaveBeenCalledOnce();

    // The next sync took #1 off the list, which says its answer has arrived.
    answering([4, 2], [3]);
    send("python/cpython");
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([4, 2, 3]);
    });
    expect(state.cursor?.number, "the line still waits, so the cursor stays").toBe(2);
    expect(landed).toHaveBeenCalledOnce();
    state.stop();
  });

  it("leaves the cursor on a PR in a tree when the Issue over it stops waiting", async () => {
    const { send } = catchListener();
    const pr = (number: number) => ({
      ...row,
      kind: "pr" as const,
      number,
      seen: seen(number),
      backports: [],
    });
    const issue = (attention: ListedItem["attention"]): ListedItem => ({
      attention,
      kind: "issue",
      number: 5,
      title: "Issue 5",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [pr(10), pr(11)],
      seen: seen(5, 10, 11),
    });
    const listing = (attention: ListedItem["attention"]) => ({
      status: "ok" as const,
      data: {
        items: [issue(attention), waiting(3)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    vi.mocked(commands.workbench).mockResolvedValue(listing(waiting(5).attention));
    const landed = vi.fn();
    const state = new WorkbenchState("python/cpython", landed);
    await state.load();
    const onTen = { number: 10, kind: "pull_request" as const, issue: 5 };
    state.moveTo(onTen);

    // A sync cleared #11's failed CI, all that waited in the tree.
    vi.mocked(commands.workbench).mockResolvedValue(listing(null));
    send("python/cpython");
    await vi.waitFor(() => {
      expect(state.items?.[0]?.attention).toBeNull();
    });

    expect(state.cursor).toEqual(onTen);
    expect(landed).not.toHaveBeenCalled();
    state.stop();
  });

  it("neither lands nor says so once stopped, for an answer that comes after", async () => {
    const { state, send, landed } = await listingWithCursor([4, 3, 2, 1], 1);
    let answer: (value: Awaited<ReturnType<typeof commands.workbench>>) => void = () => {};
    vi.mocked(commands.workbench).mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );

    send("python/cpython");
    state.stop();
    answer({
      status: "ok",
      data: {
        items: [waiting(4), waiting(2), waiting(1), item(3)],
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
        filters: [],
      },
    });
    await vi.waitFor(() => {
      expect(commands.workbench).toHaveBeenCalledTimes(2);
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(numbers(state)).toEqual([4, 3, 2, 1]);
    expect(state.cursor?.number).toBe(3);
    expect(landed).not.toHaveBeenCalled();
  });

  it("follows the item under the cursor when the list's order changes, whatever its attention", async () => {
    const { state, landed, order } = await listingWithCursor([4, 3, 2, 1], 1);
    answering([4, 2, 1], [3]);

    order.now = "last_update";
    await state.reorder();

    expect(numbers(state)).toEqual([4, 2, 1, 3]);
    expect(state.cursor?.number).toBe(3);
    expect(landed).not.toHaveBeenCalled();
    state.stop();
  });

  it("compares an answer with the items on show by the order each was asked in, whatever answers fell between", async () => {
    const { state, send, landed, order } = await listingWithCursor([4, 3, 2, 1], 1);

    // The order changes and changes back, and neither answer shows.
    vi.mocked(commands.workbench).mockResolvedValue(refused);
    order.now = "last_update";
    await state.reorder();
    order.now = "attention";
    await state.reorder();
    answering([4, 2, 1], [3]);
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(state.cursor?.number).toBe(2);
    });
    expect(landed).toHaveBeenCalledOnce();
    state.stop();
  });

  it("follows the item under the cursor into a newly selected view, where nothing it showed came before", async () => {
    const { state } = await listingWithCursor([4, 3, 2, 1], 1);
    answering([4, 2, 1], [3]);

    await state.select(onlyMine);

    expect(numbers(state)).toEqual([4, 2, 1, 3]);
    expect(state.cursor?.number).toBe(3);
    state.stop();
  });
});

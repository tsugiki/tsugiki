import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events } from "$lib/data/bindings";
import { WorkbenchState, type ListedItem, type Seen, type WorkbenchRow } from "./workbench.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { workbench: vi.fn(), markSeen: vi.fn(), markUnseen: vi.fn() },
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
  your_review: null,
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
  your_review: null,
  seen: seen(0),
};

const mine = { name: "Mine", query: "is:open author:@me", count: 1, problem: null };
const whole = { kind: "whole" } as const;
const onlyMine = { kind: "filter", query: mine.query } as const;

const answer = (...numbers: number[]) => ({
  status: "ok" as const,
  data: { items: numbers.map(item), quiet: [], count: numbers.length, review_requests: 0, filters: [mine] },
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

  it("moves the cursor through the list and the quiet section, and stops at each end", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(3)], quiet: [item(2), item(1)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.underCursor).toBeUndefined();

    state.moveCursor(-1);
    expect(state.cursor?.number).toBe(3);
    state.moveCursor(-1);
    expect(state.cursor?.number).toBe(3);
    state.moveCursor(1);
    expect(state.cursor?.number, "the folded quiet section has no lines to stop on").toBe(3);
    state.showQuiet();
    state.moveCursor(1);
    state.moveCursor(1);
    expect(state.underCursor?.number).toBe(1);
    state.moveCursor(1);
    expect(state.cursor?.number).toBe(1);
    state.stop();
  });

  it("starts with the quiet section folded, and folding it moves a cursor in it to the last line above", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(4), item(3)], quiet: [item(2)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    expect(state.quietShown).toBe(false);
    state.showQuiet();
    state.moveCursor(1);
    state.moveCursor(1);
    state.moveCursor(1);
    expect(state.cursor?.number).toBe(2);

    state.hideQuiet();

    expect(state.quietShown).toBe(false);
    expect(state.cursor?.number).toBe(3);
    state.stop();
  });

  it("leaves a cursor above the quiet section where it is when the section folds", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(4), item(3)], quiet: [item(2)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.hideQuiet();
    expect(state.cursor).toBeUndefined();
    state.showQuiet();
    state.moveCursor(1);

    state.hideQuiet();

    expect(state.cursor?.number).toBe(4);
    state.stop();
  });

  it("unfolds the quiet section for a cursor put on a line in it", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(3)], quiet: [item(2)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const pr = (number: number) => ({ number, kind: "pull_request" as const, issue: undefined });

    state.moveTo(pr(3));
    const aboveIt = state.quietShown;
    state.moveTo(pr(2));

    expect(aboveIt).toBe(false);
    expect(state.quietShown).toBe(true);
    expect(state.underCursor?.number).toBe(2);
    state.stop();
  });

  it("leaves a folded quiet section folded when the item under the cursor goes quiet", async () => {
    const { send } = catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(3), item(2)], quiet: [], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveCursor(1);
    state.moveCursor(1);
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(3)], quiet: [item(2)], count: 0, review_requests: 0, filters: [] },
    });

    send("python/cpython");
    await vi.waitFor(() => {
      expect(state.quiet).toHaveLength(1);
    });

    expect(state.quietShown).toBe(false);
    expect(state.cursor?.number, "the cursor moves to the last line above, as folding would").toBe(3);
    state.stop();
  });

  it("says whether the list or its quiet section has a line", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(3)], quiet: [item(2)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    const pr = (number: number, issue?: number) => ({ number, kind: "pull_request" as const, issue });
    const before = state.lists(pr(3));

    await state.load();

    const folded = state.lists(pr(2));
    state.showQuiet();

    expect([
      before,
      folded,
      state.lists(pr(3)),
      state.lists(pr(2)),
      state.lists(pr(3, 1)),
      state.lists(pr(4)),
    ]).toEqual([false, false, true, true, false, false]);
    state.stop();
  });

  it("knows the title of each line it lists", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [item(3)], quiet: [item(2)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();

    expect([state.titleOf(3), state.titleOf(2), state.titleOf(4)]).toEqual(["PR 3", "PR 2", undefined]);
    state.stop();
  });

  it("names the view on show as the Sidebar does, and a Filter it no longer has by its search", async () => {
    catchListener();
    const mine = { name: "Mine", query: "is:open author:@me", count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [mine] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();

    const titles = [state.title];
    for (const view of [
      { kind: "review_requests" as const },
      { kind: "filter" as const, query: mine.query },
      { kind: "filter" as const, query: "label:gone" },
    ]) {
      await state.select(view);
      titles.push(state.title);
    }

    expect(titles).toEqual(["Workbench", "Review requests", "Mine", "label:gone"]);
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
      data: { items: [tree, standalone, lone], quiet: [], count: 3, review_requests: 0, filters: [] },
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
      data: { items: [tree(10), tree(20), item(30)], quiet: [], count: 3, review_requests: 0, filters: [] },
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
    const mine = { name: "Mine", query: "is:open author:@me", count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [issueWith(10, 11)], quiet: [], count: 0, review_requests: 0, filters: [mine] },
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

  it("unfolds both the quiet section and the Issue to put the cursor on a PR of a quiet Issue", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [], quiet: [issueWith(10, 11)], count: 0, review_requests: 0, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const pr = { number: 11, kind: "pull_request" as const, issue: 10 };

    const listedFolded = state.lists(pr);
    state.moveTo(pr);

    expect(listedFolded, "a folded quiet section hides even a folded Issue's PRs").toBe(false);
    expect([state.quietShown, state.unfolded(10), state.underCursor]).toEqual([true, true, pr]);
    expect(state.titleOf(11)).toBeDefined();
    state.stop();
  });

  it("moves the cursor from a PR to its Issue as the Issue folds, and unfolds it to put the cursor on a PR", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: {
        items: [issueWith(10, 11), issueWith(20, 21)],
        quiet: [],
        count: 0,
        review_requests: 0,
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
    const mine = { name: "Mine", query: "is:open author:@me", count: 0, problem: null };
    const listed = { items: [issueWith(10, 11)], quiet: [], count: 0, review_requests: 0, filters: [mine] };
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
    const mine = { name: "Mine", query: "is:open author:@me", count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: [issueWith(10, 11)], quiet: [], count: 0, review_requests: 0, filters: [mine] },
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
      data: { items: [tree, item(6)], quiet: [], count: 2, review_requests: 0, filters: [] },
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

  it("holds the quiet section and the Review requests count", async () => {
    catchListener();
    vi.mocked(commands.workbench).mockResolvedValueOnce({
      status: "ok",
      data: { items: [], quiet: [item(4)], count: 0, review_requests: 3, filters: [] },
    });
    const state = new WorkbenchState("python/cpython");
    await state.load();

    expect(state.quiet.map((listed) => listed.number)).toEqual([4]);
    expect(state.reviewRequests).toBe(3);
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
        quiet: [item(5)],
        count: 0,
        review_requests: 0,
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
    expect(numbersOf(5, "pull_request"), "the folded quiet section's lines too").toEqual([5]);
    expect(numbersOf(9, "issue")).toBeUndefined();
    state.stop();
  });

  it("sends Mark Seen and Mark Unseen to the core, and says what went wrong", async () => {
    vi.mocked(commands.markSeen).mockResolvedValue({ status: "ok", data: null });
    vi.mocked(commands.markUnseen).mockResolvedValue(refused);
    const state = new WorkbenchState("python/cpython");
    const mark = { numbers: [1, 2], up_to: "2026-11-04T10:00:00Z" };

    expect(await state.markSeen(mark, undefined)).toBeUndefined();
    expect(await state.markUnseen(7)).toBe("no database");

    expect(commands.markSeen).toHaveBeenCalledWith("python/cpython", mark);
    expect(commands.markUnseen).toHaveBeenCalledWith("python/cpython", 7);
  });

  /** Line `number` of the list, a PR by itself. */
  const lineOf = (number: number) => ({ number, kind: "pull_request" as const, issue: undefined });

  /** The list the core answers: `items` Need attention, and `quiet` don't. */
  const answering = (items: number[], quiet: number[] = []) => {
    vi.mocked(commands.workbench).mockResolvedValue({
      status: "ok",
      data: { items: items.map(item), quiet: quiet.map(item), count: 0, review_requests: 0, filters: [] },
    });
  };

  /** A list of `items` with the cursor on its line `at`, counted from the top. */
  const listingWithCursor = async (items: number[], at: number) => {
    const { send } = catchListener();
    vi.mocked(commands.markSeen).mockResolvedValue({ status: "ok", data: null });
    answering(items);
    const state = new WorkbenchState("python/cpython");
    await state.load();
    for (let step = 0; step <= at; step++) state.moveCursor(1);
    return { state, send };
  };

  it("lands the cursor on the line that takes the place of the item it marked seen, once the core says so", async () => {
    const { state, send } = await listingWithCursor([4, 3, 2, 1], 1);
    const landed = vi.fn();

    await state.markSeen(seen(3).mark, lineOf(3), landed);
    // A sync's change, which the mark isn't part of yet.
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

  it("lands the cursor on the item that came after the one it marked seen, though one above went quiet too", async () => {
    const { state, send } = await listingWithCursor([4, 3, 2, 1], 1);

    await state.markSeen(seen(3, 4).mark, lineOf(3));
    answering([2, 1], [4, 3]);
    send("python/cpython", [3, 4]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2, 1]);
    });
    expect(state.cursor?.number).toBe(2);
    state.stop();
  });

  it("lands the cursor on the last line when the item it marked seen was the last", async () => {
    const { state, send } = await listingWithCursor([3, 2], 1);

    await state.markSeen(seen(2).mark, lineOf(2));
    answering([3], [2]);
    send("python/cpython", [2]);

    await vi.waitFor(() => {
      expect(state.cursor?.number).toBe(3);
    });
    state.stop();
  });

  it("lands the cursor on no line when nothing Needs attention after Mark Seen, and the quiet section is folded", async () => {
    const { state, send } = await listingWithCursor([3], 0);
    const landed = vi.fn();

    await state.markSeen(seen(3).mark, lineOf(3), landed);
    answering([], [3]);
    send("python/cpython", [3]);

    await vi.waitFor(() => {
      expect(landed).toHaveBeenCalledOnce();
    });
    expect(state.cursor).toBeUndefined();
    state.stop();
  });

  it("forgets where to land once the item marked seen stays in its place, or the core refuses the mark", async () => {
    const { state, send } = await listingWithCursor([4, 3, 2, 1], 1);
    const landed = vi.fn();

    // A Review request still waits on the user, so the item stays, if no longer where it was.
    await state.markSeen(seen(3).mark, lineOf(3), landed);
    answering([3, 4, 2, 1]);
    send("python/cpython", [3]);
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([3, 4, 2, 1]);
    });
    answering([4, 2, 1], [3]);
    send("python/cpython");
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([4, 2, 1]);
    });
    expect(
      state.cursor?.number,
      "an item that went quiet under the cursor leaves it on the last line above",
    ).toBe(1);
    expect(landed).not.toHaveBeenCalled();

    state.moveTo(lineOf(4));
    vi.mocked(commands.markSeen).mockResolvedValue(refused);
    expect(await state.markSeen(seen(4).mark, lineOf(4), landed)).toBe("no database");
    answering([2, 1], [4, 3]);
    send("python/cpython", [4]);
    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([2, 1]);
    });
    expect(state.cursor?.number).toBe(1);
    expect(landed).not.toHaveBeenCalled();
    state.stop();
  });

  it("leaves the cursor alone when the item marked seen isn't under it, or the user moved it since", async () => {
    const { state, send } = await listingWithCursor([3, 2, 1], 0);

    await state.markSeen(seen(2).mark, lineOf(2));
    await state.markSeen(seen(3).mark, lineOf(3));
    state.moveCursor(1);
    state.moveCursor(-1);
    answering([1], [3, 2]);
    send("python/cpython", [2, 3]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([1]);
    });
    expect(state.cursor?.number, "folding moves it to the last line above, as before").toBe(1);
    state.stop();
  });
});

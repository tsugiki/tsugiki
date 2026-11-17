import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  events,
  type InboxPage_Deserialize as InboxPage,
  type ListOrder,
} from "$lib/data/bindings";
import { done, ok, refused, workbenchPage, workbenchRow } from "$lib/testing/fixtures";
import { nextTask } from "$lib/testing/helpers";
import { catchChanged } from "$lib/testing/state";
import {
  WorkbenchState,
  type FilterLine,
  type InboxCursor,
  type ListedItem,
  type Seen,
  type WorkbenchRow,
} from "./workbench.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: {
    workbench: vi.fn(),
    inbox: vi.fn(),
    markSeen: vi.fn(),
    markUnseen: vi.fn(),
    putFilterOnWorkbench: vi.fn(),
  },
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
  ...workbenchRow(number, { title: `PR ${String(number)}`, author: "octocat", reasons: ["own_pr"] }),
  backports: [],
});

/** A PR in an Issue's tree. */
const row: WorkbenchRow = workbenchRow(0, { title: "PR", author: "octocat", ci: null });

const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 1, problem: null };
const whole = { kind: "whole" } as const;
const onlyMine = { kind: "filter", query: mine.query } as const;

/** The Workbench listing items `numbers`, counting each as Needing attention, with the Filter `mine`. */
const answer = (...numbers: number[]) =>
  workbenchPage(numbers.map(item), { count: numbers.length, filters: [mine] });

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
    catchChanged();
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
    const { send } = catchChanged();
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
    catchChanged();
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
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(3, 2));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    expect([state.titleOf(3), state.titleOf(2), state.titleOf(4)]).toEqual(["PR 3", "PR 2", undefined]);
    state.stop();
  });

  it("names the view on show as the Sidebar does, and a Filter it no longer has by its search", async () => {
    catchChanged();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue(workbenchPage([], { filters: [mine] }));
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
    catchChanged();
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
    vi.mocked(commands.workbench).mockResolvedValue(workbenchPage([tree, standalone, lone], { count: 3 }));
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
    catchChanged();
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
    vi.mocked(commands.workbench).mockResolvedValue(
      workbenchPage([tree(10), tree(20), item(30)], { count: 3 }),
    );
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
    catchChanged();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue(workbenchPage([issueWith(10, 11)], { filters: [mine] }));
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
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(workbenchPage([issueWith(10, 11), issueWith(20, 21)]));
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
    catchChanged();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    const listed = workbenchPage([issueWith(10, 11)], { filters: [mine] });
    vi.mocked(commands.workbench).mockResolvedValueOnce(listed);
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
    answer(listed);
    await selecting;

    expect([meanwhile, state.unfolded(10)]).toEqual([true, false]);
    state.stop();
  });

  it("unfolds the Issue over the PR under the cursor in a view that has it folded", async () => {
    catchChanged();
    const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 0, problem: null };
    vi.mocked(commands.workbench).mockResolvedValue(workbenchPage([issueWith(10, 11)], { filters: [mine] }));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.moveTo({ number: 11, kind: "pull_request", issue: 10 });

    await state.select({ kind: "filter", query: mine.query });

    expect([state.unfolded(10), state.underCursor?.number]).toEqual([true, 11]);
    state.stop();
  });

  it("follows a PR to its new line when a change puts it under an Issue", async () => {
    const { send } = catchChanged();
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
    vi.mocked(commands.workbench).mockResolvedValueOnce(workbenchPage([tree, item(6)], { count: 2 }));

    send("python/cpython");

    await vi.waitFor(() => {
      expect(state.cursor).toEqual({ number: 5, kind: "pull_request", issue: 10 });
    });
    state.moveCursor(1);
    expect(state.cursor?.number).toBe(6);
    state.stop();
  });

  it("starts the cursor over when a change takes its item off the list", async () => {
    const { send } = catchChanged();
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
    catchChanged();
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
    catchChanged();
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
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValueOnce(
      workbenchPage([item(4)], { review_requests: 3, stale_reviews: 2 }),
    );
    const state = new WorkbenchState("python/cpython");
    await state.load();

    expect(state.reviewRequests).toBe(3);
    expect(state.staleReviews).toBe(2);
  });

  it("asks for a Filter the user selects, and for the whole Workbench again", async () => {
    catchChanged();
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
    catchChanged();
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
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    await state.select(onlyMine);
    vi.mocked(commands.workbench).mockClear();

    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValueOnce(done);
    await state.putOnWorkbench(mine.query, false);
    expect(commands.putFilterOnWorkbench).toHaveBeenLastCalledWith({
      repository: "python/cpython",
      query: mine.query,
      on: false,
    });
    expect(state.switchProblem).toBeUndefined();
    expect(commands.workbench).toHaveBeenCalledTimes(1);

    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValueOnce(refused("no database"));
    await state.putOnWorkbench(mine.query, false);
    expect(state.switchProblem).toBe("no database");
    expect(commands.workbench).toHaveBeenCalledTimes(2);

    vi.mocked(commands.putFilterOnWorkbench).mockRejectedValueOnce(new Error("gone"));
    await state.putOnWorkbench(mine.query, false);
    expect(state.switchProblem).toBe("Error: gone");
    expect(state.switching).toBe(false);

    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValueOnce(done);
    await state.putOnWorkbench(mine.query, false);
    expect(state.switchProblem).toBeUndefined();
  });

  /** A Workbench whose one Filter is off, and the save of its switch, which the test answers. */
  const switchingOn = async () => {
    const { send } = catchChanged();
    const off = () => ({
      ...answer(1),
      data: { ...answer(1).data, filters: [{ ...mine, on_workbench: false }] },
    });
    vi.mocked(commands.workbench).mockResolvedValue(off());
    const state = new WorkbenchState("python/cpython");
    await state.load();
    const save = Promise.withResolvers<typeof done | ReturnType<typeof refused>>();
    vi.mocked(commands.putFilterOnWorkbench).mockReturnValueOnce(save.promise);
    const line = () => state.filters[0] as FilterLine;
    return { state, save, line, send, off };
  };

  it("keeps the first save's problem when a later one in the same run saves", async () => {
    const { state, save } = await switchingOn();
    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValue(done);

    const switching = state.putOnWorkbench(mine.query, true);
    await state.putOnWorkbench("label:bug", true);
    save.resolve(refused("no database"));
    await switching;

    expect(commands.putFilterOnWorkbench).toHaveBeenCalledTimes(2);
    expect(state.switchProblem).toBe("no database");
  });

  it("drops an answer to a question asked before a save landed", async () => {
    const { state, save, line, send, off } = await switchingOn();
    // Each `nextTask` settles what the answers so far set going. No polling, as a question out past the HUD's
    // budget would have the HUD record a miss.
    const stale = Promise.withResolvers<ReturnType<typeof off>>();
    vi.mocked(commands.workbench).mockReturnValueOnce(stale.promise);
    send("python/cpython");
    const other = Promise.withResolvers<typeof done>();
    vi.mocked(commands.putFilterOnWorkbench).mockReturnValueOnce(other.promise);

    const switching = state.putOnWorkbench(mine.query, true);
    await state.putOnWorkbench("label:bug", true);
    save.resolve(done);
    await nextTask();
    expect(commands.putFilterOnWorkbench).toHaveBeenCalledTimes(2);
    stale.resolve(off());
    await nextTask();

    expect(commands.workbench).toHaveBeenCalledTimes(2);
    expect(state.onWorkbench(line())).toBe(true);
    other.resolve(done);
    await switching;
  });

  it("shows a switch as the user set it while it saves, and turns it back if the save fails", async () => {
    const { state, save, line } = await switchingOn();

    const switching = state.putOnWorkbench(mine.query, true);
    expect(state.switching).toBe(true);
    expect(state.onWorkbench(line())).toBe(true);
    save.resolve(refused("no database"));
    await switching;

    expect(state.switching).toBe(false);
    expect(state.onWorkbench(line())).toBe(false);
    expect(state.switchProblem).toBe("no database");
  });

  it("stops saying a switch saves once it has saved, while the new answer is still out", async () => {
    const { state, save, line } = await switchingOn();
    const asked = Promise.withResolvers<ReturnType<typeof answer>>();
    vi.mocked(commands.workbench).mockReturnValueOnce(asked.promise);

    const switching = state.putOnWorkbench(mine.query, true);
    save.resolve(done);
    await vi.waitFor(() => {
      expect(commands.workbench).toHaveBeenCalledTimes(2);
    });

    expect(state.switching).toBe(false);
    expect(state.onWorkbench(line())).toBe(true);
    asked.resolve(answer(1));
    await switching;
  });

  it("saves a switch changed while another save is out once that one is in, as last set", async () => {
    const { state, save } = await switchingOn();
    vi.mocked(commands.putFilterOnWorkbench).mockResolvedValue(done);

    const switching = state.putOnWorkbench(mine.query, true);
    await state.putOnWorkbench(mine.query, false);
    await state.putOnWorkbench("label:bug", true);
    await state.putOnWorkbench("label:bug", false);
    expect(commands.putFilterOnWorkbench).toHaveBeenCalledOnce();
    save.resolve(done);
    await switching;

    const sent = vi.mocked(commands.putFilterOnWorkbench).mock.calls.map(([change]) => change);
    expect(sent).toEqual([
      { repository: "python/cpython", query: mine.query, on: true },
      { repository: "python/cpython", query: mine.query, on: false },
      { repository: "python/cpython", query: "label:bug", on: false },
    ]);
    expect(commands.workbench).toHaveBeenCalledTimes(2);
  });

  it("sends nothing more for a switch set back while it saved", async () => {
    const { state, save } = await switchingOn();

    const switching = state.putOnWorkbench(mine.query, true);
    await state.putOnWorkbench(mine.query, false);
    await state.putOnWorkbench(mine.query, true);
    save.resolve(done);
    await switching;

    expect(commands.putFilterOnWorkbench).toHaveBeenCalledOnce();
  });

  it("keeps a selected Filter when the facts change", async () => {
    const { send } = catchChanged();
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
    catchChanged();
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
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    await new WorkbenchState("python/cpython").load();
    const listened = vi.mocked(events.changed.listen).mock.invocationCallOrder[0];
    const asked = vi.mocked(commands.workbench).mock.invocationCallOrder[0];
    expect(listened).toBeLessThan(asked as number);
  });

  it("asks again when its Repository changes", async () => {
    const { send } = catchChanged();
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
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();
    send("tsugiki/tsugiki");
    expect(commands.workbench).toHaveBeenCalledOnce();
  });

  it("keeps the newest answer when an older one arrives after it", async () => {
    const { send } = catchChanged();
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
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValueOnce(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    vi.mocked(commands.workbench).mockResolvedValueOnce(refused("no database"));
    send("python/cpython");

    await vi.waitFor(() => {
      expect(state.error).toBe("no database");
    });
    expect(numbers(state)).toEqual([1]);
  });

  it("forgets a refusal when the next answer arrives", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValueOnce(refused("no database"));
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
    catchChanged();
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
    const { stop } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const state = new WorkbenchState("python/cpython");
    await state.load();
    state.stop();
    expect(stop).toHaveBeenCalledOnce();
  });

  it("stops listening and asks nothing if stopped before it could listen", async () => {
    const { stop } = catchChanged();
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
    catchChanged();
    const standalone = {
      ...item(3),
      backports: [{ ...row, number: 4, seen: seen(4), backport: "bot" as const }],
    };
    vi.mocked(commands.workbench).mockResolvedValue(workbenchPage([issueWith(1, 2), standalone]));
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
    vi.mocked(commands.markSeen).mockResolvedValue(done);
    vi.mocked(commands.markUnseen).mockResolvedValue(refused("no database"));
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
    vi.mocked(commands.workbench).mockResolvedValue(
      workbenchPage([...waits.map(waiting), ...rest.map(item)], { count: 0 }),
    );
  };

  /** A list of the waiting `items` with the cursor on its line `at`, counted from the top, what it calls
   * each time the cursor lands, and the order the window holds, which a test may change. */
  const listingWithCursor = async (items: number[], at: number) => {
    const { send } = catchChanged();
    answering(items);
    const landed = vi.fn();
    const order = { now: "attention" as ListOrder };
    const state = new WorkbenchState("python/cpython", { landed, order: () => order.now });
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
    const { send } = catchChanged();
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
    const { send } = catchChanged();
    const listing = (...items: ListedItem[]) => workbenchPage(items, { count: 0 });
    const issue = { ...issueWith(7, 8), attention: waiting(7).attention };
    vi.mocked(commands.workbench).mockResolvedValue(listing(issue, waiting(3)));
    const landed = vi.fn();
    const state = new WorkbenchState("python/cpython", { landed });
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

    vi.mocked(commands.workbench).mockResolvedValue(
      workbenchPage([item(3), waiting(2), waiting(1)], { count: 0 }),
    );
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
    const { send } = catchChanged();
    answering([], [3, 2, 1]);
    const landed = vi.fn();
    const state = new WorkbenchState("python/cpython", { landed });
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
    const { send } = catchChanged();
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
    const listing = (attention: ListedItem["attention"]) =>
      workbenchPage([issue(attention), waiting(3)], { count: 0 });
    vi.mocked(commands.workbench).mockResolvedValue(listing(waiting(5).attention));
    const landed = vi.fn();
    const state = new WorkbenchState("python/cpython", { landed });
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
    answer(workbenchPage([waiting(4), waiting(2), waiting(1), item(3)], { count: 0 }));
    await vi.waitFor(() => {
      expect(commands.workbench).toHaveBeenCalledTimes(2);
    });
    await nextTask();

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
    vi.mocked(commands.workbench).mockResolvedValue(refused("no database"));
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

  it("tells the page before a change's answer replaces the lines on show, and again once the cursor is placed", async () => {
    const { send } = catchChanged();
    answering([], [4, 3, 2]);
    const order = { now: "attention" as ListOrder };
    /** The lines and the cursor at each call, before the answer and after it. */
    const calls: string[] = [];
    const seen = (state: WorkbenchState, when: string) => {
      calls.push(`${when} ${String(numbers(state))} at ${String(state.cursor?.number)}`);
    };
    const state: WorkbenchState = new WorkbenchState("python/cpython", {
      order: () => order.now,
      changing: () => {
        seen(state, "before");
        return () => {
          seen(state, "after");
        };
      },
    });
    await state.load();
    state.moveTo({ number: 2, kind: "pull_request", issue: undefined });
    // A newly selected view and a new order move the lines for themselves, so neither counts as a change.
    await state.select(onlyMine);
    order.now = "last_update";
    await state.reorder();
    expect(calls).toEqual([]);

    answering([], [2, 4, 3]);
    send("python/cpython", [2]);

    await vi.waitFor(() => {
      expect(calls).toEqual(["before 4,3,2 at 2", "after 2,4,3 at 2"]);
    });
    state.stop();
  });
});

const inbox = { kind: "inbox" } as const;

/** Where item `number`, updated at `hour` o'clock, stands in the Inbox's order. */
const at = (number: number, hour: number): InboxCursor => ({
  updated_at: `2026-11-04T${String(hour).padStart(2, "0")}:00:00Z`,
  number,
});

/** A page of the Inbox holding items `numbers`, whose next older page starts after `older`. */
const page = (numbers: number[], older: InboxCursor | null = null) => ok({ items: numbers.map(item), older });

type Page = ReturnType<typeof page> | ReturnType<typeof refused>;

/** How a test names a page of the Inbox: `latest`, `all`, `older 3` for the page after item 3, or `through 3`
 * for the pages down to item 3. */
const named = (asked: InboxPage): string => {
  switch (asked.kind) {
    case "latest":
    case "all":
      return asked.kind;
    case "older":
      return `older ${String(asked.after.number)}`;
    case "through":
      return `through ${String(asked.until.number)}`;
  }
};

/** Answers each page of the Inbox by its name, as `named` gives it, and refuses one the test didn't name. */
const paging = (pages: Record<string, Page | Promise<Page>>) => {
  vi.mocked(commands.inbox).mockImplementation(
    async (_, asked) => pages[named(asked)] ?? refused("no database"),
  );
};

/** The pages of the Inbox asked for, by name. */
const pagesAsked = () => vi.mocked(commands.inbox).mock.calls.map(([, asked]) => named(asked));

// Its tests wait a `nextTask` to let each read the state chained run, as no promise a test holds says when
// the last is done.
describe("WorkbenchState's Inbox", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  /** A state that has the Inbox on show. */
  async function showingInbox() {
    const state = new WorkbenchState("python/cpython");
    await state.load();
    await state.select(inbox);
    return state;
  }

  it("lists the Inbox's first page, with the Sidebar's counts from the Workbench", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(9));
    paging({ latest: page([3, 2], at(2, 9)) });
    const state = await showingInbox();

    expect(numbers(state)).toEqual([3, 2]);
    expect(state.count).toBe(1);
    expect(state.filters).toEqual([mine]);
    expect(state.title).toBe("Inbox");
    expect(state.shown).toEqual(inbox);
    expect(state.olderToRead).toBe(true);
    expect(commands.workbench).toHaveBeenLastCalledWith("python/cpython", whole);
    expect(pagesAsked()).toEqual(["latest"]);

    await state.select(whole);
    expect(numbers(state)).toEqual([9]);
    expect(state.olderToRead).toBe(false);
    state.stop();
  });

  it("reads the older pages onto the end, each item once, until the cache holds nothing older", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([3, 2], at(2, 9)), "older 2": page([2, 1]) });
    const state = await showingInbox();

    await state.readOlder();
    expect(numbers(state)).toEqual([3, 2, 1]);
    expect(commands.inbox).toHaveBeenLastCalledWith("python/cpython", { kind: "older", after: at(2, 9) });
    expect(state.olderToRead).toBe(false);

    await state.readOlder();
    expect(pagesAsked()).toEqual(["latest", "older 2"]);
    state.stop();
  });

  it("reads nothing older while a view of the Workbench is on show", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(1));
    const state = new WorkbenchState("python/cpython");
    await state.load();

    await state.readOlder();
    expect(commands.inbox).not.toHaveBeenCalled();
    state.stop();
  });

  it("reads the next page once, however often the list asks while it reads", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const next = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "older 3": next.promise });
    const state = await showingInbox();

    const reading = state.readOlder();
    await state.readOlder();
    next.resolve(page([2]));
    await reading;
    expect(numbers(state)).toEqual([3, 2]);
    expect(pagesAsked()).toEqual(["latest", "older 3"]);
    state.stop();
  });

  it("reads the pages on show again after a change in one page, which ends where they ended", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([5, 4], at(4, 9)), "older 4": page([3, 2], at(2, 7)) });
    const state = await showingInbox();
    await state.readOlder();
    expect(numbers(state)).toEqual([5, 4, 3, 2]);
    vi.mocked(commands.inbox).mockClear();

    // A sync moved 1 to the top.
    paging({ "through 2": page([1, 5, 4, 3, 2], at(2, 7)) });
    send("python/cpython");

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([1, 5, 4, 3, 2]);
    });
    expect(commands.inbox).toHaveBeenLastCalledWith("python/cpython", { kind: "through", until: at(2, 7) });
    expect(state.olderToRead).toBe(true);
    send("python/cpython");
    await nextTask();
    expect(pagesAsked()).toEqual(["through 2", "through 2"]);
    state.stop();
  });

  it("reads only the first page's items again after a change while only that page is on show", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([3], at(0, 7)), "through 0": page([4, 3], at(0, 7)) });
    const state = await showingInbox();

    send("python/cpython");
    await nextTask();
    send("python/cpython");
    await nextTask();

    // The first page's start moves with the clock, but the pages on show end where they did.
    expect(pagesAsked()).toEqual(["latest", "through 0", "through 0"]);
    expect(numbers(state)).toEqual([4, 3]);
    state.stop();
  });

  it("reads the pages again once for a burst of changes that land while a read is under way", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const next = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "older 3": next.promise, all: page([4, 3, 2]) });
    const state = await showingInbox();
    const reading = state.readOlder();

    send("python/cpython");
    send("python/cpython");
    send("python/cpython");
    next.resolve(page([2]));
    await reading;
    await nextTask();

    expect(pagesAsked()).toEqual(["latest", "older 3", "all"]);
    expect(numbers(state)).toEqual([4, 3, 2]);
    state.stop();
  });

  it("reads every page again after a change once those on show reached the end", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([3], at(3, 9)), "older 3": page([2]) });
    const state = await showingInbox();
    await state.readOlder();

    paging({ all: page([3, 2, 1]) });
    send("python/cpython");

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([3, 2, 1]);
    });
    expect(pagesAsked()).toEqual(["latest", "older 3", "all"]);
    expect(state.olderToRead).toBe(false);
    state.stop();
  });

  it("reads the first page again when the Inbox is selected anew, however far it read before", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(9));
    paging({ latest: page([3], at(3, 9)), "older 3": page([2]) });
    const state = await showingInbox();
    await state.readOlder();

    await state.select(whole);
    await state.select(inbox);

    expect(pagesAsked()).toEqual(["latest", "older 3", "latest"]);
    expect(numbers(state)).toEqual([3]);
    state.stop();
  });

  it("puts an older page on the end that lands while a change waits, then reads through it", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const next = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "older 3": next.promise, "through 2": page([4, 3, 2], at(2, 8)) });
    const state = await showingInbox();
    const reading = state.readOlder();

    send("python/cpython");
    await nextTask();
    // The change's answer waits for the page under way.
    expect(pagesAsked()).toEqual(["latest", "older 3"]);
    next.resolve(page([2], at(2, 8)));
    await reading;
    expect(numbers(state)).toEqual([3, 2]);

    await vi.waitFor(() => {
      expect(numbers(state)).toEqual([4, 3, 2]);
    });
    expect(pagesAsked()).toEqual(["latest", "older 3", "through 2"]);
    expect(state.olderToRead).toBe(true);
    state.stop();
  });

  it("reads an older page asked while a change's answer is on its way once that answer is on show", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const again = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "through 3": again.promise, "older 2": page([1]) });
    const state = await showingInbox();

    send("python/cpython");
    await nextTask();
    const reading = state.readOlder();
    await nextTask();
    expect(pagesAsked()).toEqual(["latest", "through 3"]);
    // The change put 2 below 3, so the next page starts after 2.
    again.resolve(page([4, 3, 2], at(2, 8)));
    await reading;

    expect(pagesAsked()).toEqual(["latest", "through 3", "older 2"]);
    expect(numbers(state)).toEqual([4, 3, 2, 1]);
    state.stop();
  });

  it("reads no older page that a change's answer, landing first, left nothing to read below", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const again = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "through 3": again.promise });
    const state = await showingInbox();

    send("python/cpython");
    await nextTask();
    const reading = state.readOlder();
    again.resolve(page([4, 3]));
    await reading;

    expect(pagesAsked()).toEqual(["latest", "through 3"]);
    expect(state.olderError).toBeUndefined();
    state.stop();
  });

  it("asks nothing for a change that a newly selected view followed while it waited", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const next = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "older 3": next.promise });
    const state = await showingInbox();
    const reading = state.readOlder();
    send("python/cpython");

    vi.mocked(commands.workbench).mockResolvedValue(answer(9));
    await state.select(whole);
    const asked = vi.mocked(commands.workbench).mock.calls.length;
    next.resolve(page([2]));
    await reading;
    await nextTask();

    // Nor does the older page land on the Workbench's list.
    expect(numbers(state)).toEqual([9]);
    expect(pagesAsked()).toEqual(["latest", "older 3"]);
    expect(commands.workbench).toHaveBeenCalledTimes(asked);
    state.stop();
  });

  it("goes on reading the Inbox after a page's hook throws while an answer goes on show", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({
      latest: page([3], at(3, 9)),
      // An answer that would end the pages further down, had it gone on show.
      "through 3": page([4, 3, 2], at(2, 8)),
      "older 3": page([2]),
      all: page([4, 3, 2]),
    });
    const hook = { throws: true };
    const state = new WorkbenchState("python/cpython", {
      changing: () => {
        if (hook.throws) throw new Error("the hook broke");
        return () => {};
      },
    });
    await state.load();
    await state.select(inbox);

    // Asking for the Inbox again reads its pages on show, and the change hook runs.
    await expect(state.select(inbox)).rejects.toThrow("the hook broke");
    hook.throws = false;
    expect(numbers(state)).toEqual([3]);
    await state.readOlder();
    await state.select(inbox);

    // The next page starts where the items on show end, not where the answer that didn't show did.
    expect(pagesAsked()).toEqual(["latest", "through 3", "older 3", "all"]);
    expect(numbers(state)).toEqual([4, 3, 2]);
    state.stop();
  });

  it("says why a change's answer didn't go on show when a page's hook throws", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([3], at(3, 9)), "through 3": page([4, 3], at(3, 9)) });
    const hook = { throws: true };
    const state = new WorkbenchState("python/cpython", {
      changing: () => {
        if (hook.throws) throw new Error("the hook broke");
        return () => {};
      },
    });
    await state.load();
    await state.select(inbox);

    send("python/cpython");
    await nextTask();
    expect(state.error).toBe("Error: the hook broke");
    expect(numbers(state)).toEqual([3]);

    hook.throws = false;
    send("python/cpython");
    await nextTask();
    expect(state.error).toBeUndefined();
    expect(numbers(state)).toEqual([4, 3]);
    state.stop();
  });

  it("drops an older page that lands after the window left the Repository", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    const next = Promise.withResolvers<Page>();
    paging({ latest: page([3], at(3, 9)), "older 3": next.promise });
    const state = await showingInbox();
    const reading = state.readOlder();

    state.stop();
    next.resolve(page([2]));
    await reading;
    expect(numbers(state)).toEqual([3]);
  });

  it("says why it couldn't read the Inbox, and keeps the list on show with the Workbench's counts", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(9));
    paging({});
    const state = new WorkbenchState("python/cpython");
    await state.load();
    vi.mocked(commands.workbench).mockResolvedValue(answer(7, 8));
    await state.select(inbox);

    expect(state.error).toBe("no database");
    expect(numbers(state)).toEqual([9]);
    expect(state.count).toBe(2);
    state.stop();
  });

  it("says why it couldn't read the Workbench's counts, and lists the Inbox", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(9));
    paging({ latest: page([3]) });
    const state = new WorkbenchState("python/cpython");
    await state.load();
    vi.mocked(commands.workbench).mockResolvedValue(refused("no database"));
    await state.select(inbox);

    expect(state.error).toBe("no database");
    expect(numbers(state)).toEqual([3]);
    expect(state.count).toBe(1);
    state.stop();
  });

  it("says apart why it couldn't read the next page, and clears that once a page comes", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([3], at(3, 9)) });
    const state = await showingInbox();

    await state.readOlder();
    expect(state.olderError).toBe("no database");
    expect(state.error).toBeUndefined();
    expect(numbers(state)).toEqual([3]);

    paging({ latest: page([3], at(3, 9)), "older 3": page([2]) });
    await state.readOlder();
    expect(state.olderError).toBeUndefined();
    expect(numbers(state)).toEqual([3, 2]);
    state.stop();
  });

  it("keeps saying why it couldn't read the pages again once an older page comes", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer());
    paging({ latest: page([3], at(3, 9)) });
    const state = await showingInbox();
    send("python/cpython");
    await nextTask();
    expect(state.error).toBe("no database");

    paging({ "older 3": page([2]) });
    await state.readOlder();
    expect(numbers(state)).toEqual([3, 2]);
    expect(state.error).toBe("no database");
    state.stop();
  });

  it("clears why it couldn't read the next page once another view's answer is on show", async () => {
    catchChanged();
    vi.mocked(commands.workbench).mockResolvedValue(answer(9));
    paging({ latest: page([3], at(3, 9)) });
    const state = await showingInbox();
    await state.readOlder();
    expect(state.olderError).toBe("no database");

    await state.select(whole);
    expect(state.olderError).toBeUndefined();
    state.stop();
  });
});

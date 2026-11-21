import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { ok, refused } from "$lib/testing/fixtures";
import { UnstoredViewState, type FetchedKind } from "./unstored-view.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { fetchItem: vi.fn() } }));

/** A view of #12 that has loaded, and the kinds it handed on. */
async function loaded(): Promise<[UnstoredViewState, FetchedKind[]]> {
  const view = new UnstoredViewState("python/cpython", 12);
  const kinds: FetchedKind[] = [];
  await view.load((kind) => kinds.push(kind));
  return [view, kinds];
}

describe("UnstoredViewState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("asks the core to fetch the number, and hands on the kind it found", async () => {
    vi.mocked(commands.fetchItem).mockResolvedValue(ok({ kind: "pull_request" }));

    const [view, kinds] = await loaded();

    expect(commands.fetchItem).toHaveBeenCalledWith("python/cpython", 12);
    expect(kinds).toEqual(["pull_request"]);
    expect(view.failure).toBeUndefined();
  });

  it("says why a fetch brought nothing, and hands on nothing", async () => {
    vi.mocked(commands.fetchItem).mockResolvedValue(ok({ kind: "unfetched", why: "hidden" }));

    const [view, kinds] = await loaded();

    expect(kinds).toEqual([]);
    expect(view.failure).toEqual({ why: "hidden" });
  });

  it("says what refused the command", async () => {
    vi.mocked(commands.fetchItem).mockResolvedValue(refused("Not a Repository name."));

    const [view] = await loaded();

    expect(view.failure).toEqual({ problem: "Not a Repository name." });
  });

  it("opens nothing once it has stopped, as when the user opened something else", async () => {
    let answer: (found: Awaited<ReturnType<typeof commands.fetchItem>>) => void = () => {};
    vi.mocked(commands.fetchItem).mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    const view = new UnstoredViewState("python/cpython", 12);
    const kinds: FetchedKind[] = [];

    const loading = view.load((kind) => kinds.push(kind));
    view.stop();
    answer(ok({ kind: "issue" }));
    await loading;

    expect(kinds).toEqual([]);
    expect(view.failure).toBeUndefined();
  });
});

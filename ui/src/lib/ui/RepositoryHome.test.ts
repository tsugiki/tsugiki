import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import RepositoryHome from "./RepositoryHome.svelte";

const core = vi.hoisted(() => ({
  workbench: vi.fn(),
  listen: vi.fn(),
  stopListening: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: { workbench: core.workbench },
  events: { workbenchChanged: { listen: core.listen } },
}));

const row = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "johng",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  reasons: ["review_request"],
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(core.stopListening);
  core.workbench.mockResolvedValue({ status: "ok", data: { rows: [] } });
});

it("names the Repository", () => {
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  expect(screen.getByRole("heading", { name: "python/cpython" })).toBeTruthy();
});

it("offers to add another", async () => {
  const onadd = vi.fn();
  render(RepositoryHome, { name: "python/cpython", onadd });
  await fireEvent.click(screen.getByRole("button", { name: "Add repository" }));
  expect(onadd).toHaveBeenCalledOnce();
});

it("lists each PR the core lists", async () => {
  core.workbench.mockResolvedValue({ status: "ok", data: { rows: [row] } });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const item = await screen.findByRole("listitem");

  expect(core.workbench).toHaveBeenCalledWith("python/cpython");
  expect(within(item).getByText("#160662 gh-112564: Fix UAF for gc debug print")).toBeTruthy();
  expect(within(item).getByText("Review request · johng · into main · CI passed")).toBeTruthy();
});

it("says so while there is nothing to list", async () => {
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  expect(await screen.findByText(/Nothing here yet/)).toBeTruthy();
});

it("says why it can't read the Workbench", async () => {
  core.workbench.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Tsugiki can't read this Workbench: no database",
  );
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("opens the other Repository's Workbench and stops listening for the last", async () => {
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  await screen.findByText(/Nothing here yet/);

  await rerender({ name: "tsugiki/tsugiki", onadd: () => {} });

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("tsugiki/tsugiki");
  });
  expect(core.stopListening).toHaveBeenCalledOnce();
});

it("stops listening when it goes", async () => {
  const { unmount } = render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  await screen.findByText(/Nothing here yet/);
  unmount();
  expect(core.stopListening).toHaveBeenCalledOnce();
});

import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import type { RepositoryCheck } from "$lib/state/add-repository.svelte";
import AddRepository from "./AddRepository.svelte";

const core = vi.hoisted(() => ({
  checkRepository: vi.fn(),
  addRepository: vi.fn(),
  pickFolder: vi.fn(),
  checkClone: vi.fn(),
}));
vi.mock("$lib/data/bindings", () => ({ commands: core }));

const check: RepositoryCheck = {
  repository: { name: "some-maintainer/gcmon", conventions: "github", filters: [] },
  open_issues: 10,
  open_prs: 3,
  size: "small",
  window_days: 30,
  first_sync_items: 48,
  first_sync_seconds: 5,
  first_sync_points: 12,
  mirror: { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 },
  access: "other",
};

const home = { screen: "repository", repository: check.repository, others: [] as string[] } as const;

const button = (name: string) => screen.getByRole<HTMLButtonElement>("button", { name });

async function checked() {
  core.checkRepository.mockResolvedValue({ status: "ok", data: { kind: "new", check } });
  const input = screen.getByRole("textbox", { name: "Repository" });
  await fireEvent.input(input, { target: { value: "some-maintainer/gcmon" } });
  await fireEvent.click(button("Check"));
  await screen.findByRole("heading", { name: "some-maintainer/gcmon" });
}

beforeEach(() => {
  vi.resetAllMocks();
});

it("is a dialog with a name that takes the focus", () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  expect(screen.getByRole("dialog", { name: "Add repository" })).toBeTruthy();
  expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Repository" }));
});

it("says what checking found: the counts, the choice, the time and the cost", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await checked();
  expect(core.checkRepository).toHaveBeenCalledWith("some-maintainer/gcmon");
  expect(screen.getByText("10 open Issues, 3 open PRs.")).toBeTruthy();
  expect(screen.getByText(/^Small:/)).toBeTruthy();
  expect(screen.getByText(/under a minute, about 12 points/)).toBeTruthy();
  expect(screen.getByText("Conventions: GitHub.")).toBeTruthy();
  expect(button("Add")).toBeTruthy();
});

it("waits for GitHub with its button off", async () => {
  core.checkRepository.mockReturnValue(new Promise(() => {}));
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await fireEvent.click(button("Check"));
  expect(screen.getByText("Asking GitHub…")).toBeTruthy();
  expect(button("Check").disabled).toBe(true);
});

it("can be left while GitHub hasn't answered", async () => {
  core.checkRepository.mockReturnValue(new Promise(() => {}));
  const onclose = vi.fn();
  render(AddRepository, { onclose, onadded: () => {}, onswitch: () => {} });
  await fireEvent.click(button("Check"));
  await fireEvent.click(button("Cancel"));
  expect(onclose).toHaveBeenCalledOnce();
});

it("says what's wrong and lets the user try again", async () => {
  const error = { kind: "offline", message: "Tsugiki can't reach GitHub." } as const;
  core.checkRepository.mockResolvedValue({ status: "error", error });
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await fireEvent.click(button("Check"));
  expect((await screen.findByRole("alert")).textContent).toBe("Tsugiki can't reach GitHub.");
  expect(button("Check").disabled).toBe(false);
});

it("typing again asks for another check", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await checked();
  await fireEvent.input(screen.getByRole("textbox", { name: "Repository" }), { target: { value: "a/b" } });
  expect(screen.queryByRole("heading")).toBeNull();
  expect(button("Check")).toBeTruthy();
});

it("adds what was checked and hands on what the window opens on", async () => {
  core.addRepository.mockResolvedValue({ status: "ok", data: home });
  const onadded = vi.fn();
  render(AddRepository, { onclose: () => {}, onadded, onswitch: () => {} });
  await checked();
  await fireEvent.click(button("Add"));
  await vi.waitFor(() => {
    expect(onadded).toHaveBeenCalledWith(home);
  });
});

/** Types `typed` and checks it, and the core says Tsugiki has python/cpython. */
async function held(typed: string) {
  core.checkRepository.mockResolvedValue({ status: "ok", data: { kind: "held", name: "python/cpython" } });
  await fireEvent.input(screen.getByRole("textbox", { name: "Repository" }), { target: { value: typed } });
  await fireEvent.click(button("Check"));
  await screen.findByText("Tsugiki has python/cpython already.");
}

it("says Tsugiki has a Repository already, with no mirror choices, and offers Switch to it", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await held("Python/CPython");

  expect(core.checkRepository).toHaveBeenCalledWith("Python/CPython");
  expect(screen.queryByRole("group")).toBeNull();
  expect(screen.queryByRole("radio")).toBeNull();
  expect(screen.queryByRole("button", { name: "Add" })).toBeNull();
  expect(button("Switch to it").disabled).toBe(false);
});

it("switches to the Repository Tsugiki has, by its name there, and adds nothing", async () => {
  const onswitch = vi.fn();
  const onadded = vi.fn();
  render(AddRepository, { onclose: () => {}, onadded, onswitch });
  await held("Python/CPython");

  await fireEvent.click(button("Switch to it"));

  expect(onswitch).toHaveBeenCalledExactlyOnceWith("python/cpython");
  expect(onadded).not.toHaveBeenCalled();
  expect(core.addRepository).not.toHaveBeenCalled();
});

it("asks for another check when the user types over a Repository Tsugiki has", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await held("python/cpython");
  await fireEvent.input(screen.getByRole("textbox", { name: "Repository" }), { target: { value: "a/b" } });
  expect(screen.queryByText("Tsugiki has python/cpython already.")).toBeNull();
  expect(button("Check")).toBeTruthy();
});

const lender = "D:/gcmon";
const radio = (name: RegExp) => screen.getByRole<HTMLInputElement>("radio", { name });

it("offers to download the mirror or borrow objects from a clone, with the time and the disk", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await checked();

  const mirror = screen.getByRole("group", { name: "Mirror, for the diff: 891 MB on the disk either way." });
  expect(mirror).toBeTruthy();
  expect(radio(/^Download it from GitHub, about 3 minutes/).checked).toBe(true);
  expect(radio(/^Borrow objects from a clone you have, about 1 minute/).checked).toBe(false);
});

it("borrows from a clone the user picks, and adds with it", async () => {
  core.pickFolder.mockResolvedValue({ status: "ok", data: lender });
  core.checkClone.mockResolvedValue({ status: "ok", data: null });
  core.addRepository.mockResolvedValue({ status: "ok", data: home });
  const onadded = vi.fn();
  render(AddRepository, { onclose: () => {}, onadded, onswitch: () => {} });
  await checked();

  await fireEvent.click(radio(/^Borrow objects/));
  await screen.findByText("Borrows from D:/gcmon");
  await fireEvent.click(button("Add"));

  await vi.waitFor(() => {
    expect(onadded).toHaveBeenCalledWith(home);
  });
  expect(core.addRepository).toHaveBeenCalledWith({
    name: "some-maintainer/gcmon",
    access: "other",
    source: { kind: "borrow", folder: lender },
  });
});

it("keeps Add off while the clone can't lend, until the user picks another or downloads", async () => {
  core.pickFolder.mockResolvedValue({ status: "ok", data: lender });
  core.checkClone.mockResolvedValue({ status: "ok", data: { kind: "no_remote" } });
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await checked();

  await fireEvent.click(radio(/^Borrow objects/));
  await screen.findByText(/has no remote for some-maintainer\/gcmon/);
  const refused = button("Add").disabled;
  core.checkClone.mockResolvedValue({ status: "ok", data: null });
  await fireEvent.click(button("Choose another clone…"));
  await screen.findByText("Borrows from D:/gcmon");
  const lends = button("Add").disabled;
  await fireEvent.click(radio(/^Download it/));

  expect(refused).toBe(true);
  expect(lends).toBe(false);
  expect(button("Add").disabled).toBe(false);
  expect(screen.queryByText("Borrows from D:/gcmon")).toBeNull();
});

it("holds its text still while it adds", async () => {
  core.addRepository.mockReturnValue(new Promise(() => {}));
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  await checked();
  await fireEvent.click(button("Add"));
  expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Repository" }).readOnly).toBe(true);
  expect(button("Add").disabled).toBe(true);
});

it("stays open and says why when an add fails", async () => {
  const error = { kind: "internal", message: "settings.toml, line 3: expected a value" } as const;
  core.addRepository.mockResolvedValue({ status: "error", error });
  const onadded = vi.fn();
  render(AddRepository, { onclose: () => {}, onadded, onswitch: () => {} });
  await checked();
  await fireEvent.click(button("Add"));
  expect((await screen.findByRole("alert")).textContent).toBe(error.message);
  expect(onadded).not.toHaveBeenCalled();
});

it("closes on Esc, on Cancel and on a press outside it", async () => {
  const onclose = vi.fn();
  render(AddRepository, { onclose, onadded: () => {}, onswitch: () => {} });
  await fireEvent.keyDown(window, { key: "Escape" });
  await fireEvent.click(button("Cancel"));
  await fireEvent.mouseDown(screen.getByTestId("add-repository-scrim"));
  expect(onclose).toHaveBeenCalledTimes(3);
});

it("keeps the browser from moving the focus on a press outside it, as the focus has gone back", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {}, onswitch: () => {} });
  // A browser focuses the body on a press it isn't stopped from, after the dialog gave the focus back.
  expect(await fireEvent.mouseDown(screen.getByTestId("add-repository-scrim"))).toBe(false);
});

it("stays open on other keys and on a press inside it", async () => {
  const onclose = vi.fn();
  render(AddRepository, { onclose, onadded: () => {}, onswitch: () => {} });
  await fireEvent.keyDown(window, { key: "a" });
  await fireEvent.mouseDown(screen.getByRole("dialog"));
  expect(onclose).not.toHaveBeenCalled();
});

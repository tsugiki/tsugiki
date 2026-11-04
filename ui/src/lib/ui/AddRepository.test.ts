import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import type { RepositoryCheck } from "$lib/state/add-repository.svelte";
import AddRepository from "./AddRepository.svelte";

const core = vi.hoisted(() => ({ checkRepository: vi.fn(), addRepository: vi.fn() }));
vi.mock("$lib/data/bindings", () => ({ commands: core }));

const check: RepositoryCheck = {
  repository: { name: "sergey-miryanov/gcmon", conventions: "github" },
  open_issues: 10,
  open_prs: 3,
  size: "small",
  window_days: 30,
  first_sync_items: 48,
  first_sync_seconds: 5,
  first_sync_points: 12,
};

const home = { screen: "repository", repository: check.repository } as const;

const button = (name: string) => screen.getByRole<HTMLButtonElement>("button", { name });

async function checked() {
  core.checkRepository.mockResolvedValue({ status: "ok", data: check });
  const input = screen.getByRole("textbox", { name: "Repository" });
  await fireEvent.input(input, { target: { value: "sergey-miryanov/gcmon" } });
  await fireEvent.click(button("Check"));
  await screen.findByRole("heading", { name: "sergey-miryanov/gcmon" });
}

beforeEach(() => {
  vi.resetAllMocks();
});

it("is a dialog with a name that takes the focus", () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {} });
  expect(screen.getByRole("dialog", { name: "Add repository" })).toBeTruthy();
  expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Repository" }));
});

it("says what checking found: the counts, the choice, the time and the cost", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {} });
  await checked();
  expect(core.checkRepository).toHaveBeenCalledWith("sergey-miryanov/gcmon");
  expect(screen.getByText("10 open Issues, 3 open PRs.")).toBeTruthy();
  expect(screen.getByText(/^Small:/)).toBeTruthy();
  expect(screen.getByText(/under a minute, about 12 points/)).toBeTruthy();
  expect(screen.getByText("Conventions: GitHub.")).toBeTruthy();
  expect(button("Add")).toBeTruthy();
});

it("waits for GitHub with its button off", async () => {
  core.checkRepository.mockReturnValue(new Promise(() => {}));
  render(AddRepository, { onclose: () => {}, onadded: () => {} });
  await fireEvent.click(button("Check"));
  expect(screen.getByText("Asking GitHub…")).toBeTruthy();
  expect(button("Check").disabled).toBe(true);
});

it("can be left while GitHub hasn't answered", async () => {
  core.checkRepository.mockReturnValue(new Promise(() => {}));
  const onclose = vi.fn();
  render(AddRepository, { onclose, onadded: () => {} });
  await fireEvent.click(button("Check"));
  await fireEvent.click(button("Cancel"));
  expect(onclose).toHaveBeenCalledOnce();
});

it("says what's wrong and lets the user try again", async () => {
  const error = { kind: "offline", message: "Tsugiki can't reach GitHub." } as const;
  core.checkRepository.mockResolvedValue({ status: "error", error });
  render(AddRepository, { onclose: () => {}, onadded: () => {} });
  await fireEvent.click(button("Check"));
  expect((await screen.findByRole("alert")).textContent).toBe("Tsugiki can't reach GitHub.");
  expect(button("Check").disabled).toBe(false);
});

it("typing again asks for another check", async () => {
  render(AddRepository, { onclose: () => {}, onadded: () => {} });
  await checked();
  await fireEvent.input(screen.getByRole("textbox", { name: "Repository" }), { target: { value: "a/b" } });
  expect(screen.queryByRole("heading")).toBeNull();
  expect(button("Check")).toBeTruthy();
});

it("adds what was checked and hands on what the window opens on", async () => {
  core.addRepository.mockResolvedValue({ status: "ok", data: home });
  const onadded = vi.fn();
  render(AddRepository, { onclose: () => {}, onadded });
  await checked();
  await fireEvent.click(button("Add"));
  await vi.waitFor(() => {
    expect(onadded).toHaveBeenCalledWith(home);
  });
});

it("holds its text still while it adds", async () => {
  core.addRepository.mockReturnValue(new Promise(() => {}));
  render(AddRepository, { onclose: () => {}, onadded: () => {} });
  await checked();
  await fireEvent.click(button("Add"));
  expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Repository" }).readOnly).toBe(true);
  expect(button("Add").disabled).toBe(true);
});

it("stays open and says why when an add fails", async () => {
  const error = { kind: "internal", message: "settings.toml, line 3: expected a value" } as const;
  core.addRepository.mockResolvedValue({ status: "error", error });
  const onadded = vi.fn();
  render(AddRepository, { onclose: () => {}, onadded });
  await checked();
  await fireEvent.click(button("Add"));
  expect((await screen.findByRole("alert")).textContent).toBe(error.message);
  expect(onadded).not.toHaveBeenCalled();
});

it("closes on Esc, on Cancel and on a press outside it", async () => {
  const onclose = vi.fn();
  render(AddRepository, { onclose, onadded: () => {} });
  await fireEvent.keyDown(window, { key: "Escape" });
  await fireEvent.click(button("Cancel"));
  await fireEvent.mouseDown(screen.getByTestId("add-repository-scrim"));
  expect(onclose).toHaveBeenCalledTimes(3);
});

it("stays open on other keys and on a press inside it", async () => {
  const onclose = vi.fn();
  render(AddRepository, { onclose, onadded: () => {} });
  await fireEvent.keyDown(window, { key: "a" });
  await fireEvent.mouseDown(screen.getByRole("dialog"));
  expect(onclose).not.toHaveBeenCalled();
});

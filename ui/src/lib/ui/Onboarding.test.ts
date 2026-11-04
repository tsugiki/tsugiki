import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import Onboarding from "./Onboarding.svelte";

it("says where the token comes from", () => {
  render(Onboarding, { token: "gh", onadd: () => {} });
  expect(screen.getByText("Your token comes from the GitHub CLI (gh auth token).")).toBeTruthy();
});

it("offers to add a Repository", async () => {
  const onadd = vi.fn();
  render(Onboarding, { token: "gh", onadd });
  await fireEvent.click(screen.getByRole("button", { name: "Add repository" }));
  expect(onadd).toHaveBeenCalledOnce();
});

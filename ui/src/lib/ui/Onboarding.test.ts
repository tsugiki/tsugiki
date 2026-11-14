import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import Onboarding from "./Onboarding.svelte";

it("says where the token comes from", () => {
  render(Onboarding, { token: "gh", onadd: () => {}, onsettings: () => {} });
  expect(screen.getByText("Your token comes from the GitHub CLI (gh auth token).")).toBeTruthy();
});

it("offers to add a Repository", async () => {
  const onadd = vi.fn();
  render(Onboarding, { token: "gh", onadd, onsettings: () => {} });
  await fireEvent.click(screen.getByRole("button", { name: "Add repository" }));
  expect(onadd).toHaveBeenCalledOnce();
});

it("has ⚙ Settings, which asks for the Settings dialog", async () => {
  const onsettings = vi.fn();
  render(Onboarding, { token: "gh", onadd: () => {}, onsettings });
  const settings = screen.getByRole("button", { name: "Settings" });
  expect(settings.textContent).toBe("⚙ Settings");
  expect(settings.getAttribute("aria-haspopup")).toBe("dialog");
  await fireEvent.click(settings);
  expect(onsettings).toHaveBeenCalledOnce();
});

it("names ⚙ Settings' key in its tooltip as the OS calls it", () => {
  const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  const { unmount } = render(Onboarding, { token: "gh", onadd: () => {}, onsettings: () => {} });
  expect(screen.getByRole("button", { name: "Settings" }).title).toBe("Settings (Cmd+,)");
  unmount();

  platform.mockReturnValue("Win32");
  render(Onboarding, { token: "gh", onadd: () => {}, onsettings: () => {} });
  expect(screen.getByRole("button", { name: "Settings" }).title).toBe("Settings (Ctrl+,)");
  platform.mockRestore();
});

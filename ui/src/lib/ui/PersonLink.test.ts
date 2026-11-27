import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import PersonLink from "./PersonLink.svelte";

it("fills the Inbox's search box from the login on a click", async () => {
  const onopen = vi.fn();
  render(PersonLink, { login: "octocat", onopen });

  const link = screen.getByRole("button", { name: "octocat" });
  expect(link.tagName).toBe("BUTTON");

  await fireEvent.click(link);
  expect(onopen).toHaveBeenCalledExactlyOnceWith("octocat");
});

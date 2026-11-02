import { render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import Page from "./+page.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: {
    about: async () => ({ name: "Tsugiki", version: "1.2.3" }),
    status: async () => ({ connection: "online", login: "octocat", problem: null }),
  },
  events: { statusChanged: { listen: async () => () => {} } },
}));

it("asks the core when it opens and shows the answer", async () => {
  render(Page);
  expect(screen.getByText("Asking the core…")).toBeTruthy();
  expect(await screen.findByText("Tsugiki 1.2.3")).toBeTruthy();
});

it("shows who is signed in", async () => {
  render(Page);
  expect(await screen.findByText("octocat")).toBeTruthy();
});

import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import AboutLine from "./AboutLine.svelte";

describe("AboutLine", () => {
  it("names the app and its version", () => {
    render(AboutLine, { about: { name: "Tsugiki", version: "1.2.3" } });
    expect(screen.getByText("Tsugiki 1.2.3")).toBeTruthy();
  });

  it("waits for the core", () => {
    render(AboutLine, { about: undefined });
    expect(screen.getByText("Asking the core…")).toBeTruthy();
  });

  it("says when the core can't be reached", () => {
    render(AboutLine, { about: undefined, error: "Error: no IPC" });
    expect(screen.getByText("The core didn't answer: Error: no IPC")).toBeTruthy();
  });
});

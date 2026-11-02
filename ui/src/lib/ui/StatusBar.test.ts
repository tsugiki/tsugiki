import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import StatusBar from "./StatusBar.svelte";

const bar = () => screen.getByRole("status");

describe("StatusBar", () => {
  it("shows who is signed in", () => {
    render(StatusBar, { status: { state: "online", login: "octocat" } });
    expect(bar().textContent).toContain("octocat");
    expect(bar().classList.contains("offline")).toBe(false);
  });

  it("says offline and tints the bar", () => {
    render(StatusBar, { status: { state: "offline" } });
    expect(bar().textContent).toContain("Offline: read-only");
    expect(bar().classList.contains("offline")).toBe(true);
  });

  it("shows what stops Tsugiki reading, in the core's words", () => {
    const problem = { kind: "unauthorized", message: "Run `gh auth login`." } as const;
    render(StatusBar, { status: { state: "problem", problem } });
    expect(bar().textContent).toContain("Run `gh auth login`.");
  });

  it("waits while the core is connecting", () => {
    render(StatusBar, { status: { state: "connecting" } });
    expect(bar().textContent).toContain("Connecting…");
  });

  it("waits until the core has answered", () => {
    render(StatusBar, { status: undefined });
    expect(bar().textContent).toContain("Connecting…");
  });

  it("says when it can't read the status", () => {
    render(StatusBar, { status: undefined, error: "Error: no IPC" });
    expect(bar().textContent).toContain("Tsugiki can't read its status: Error: no IPC");
  });
});

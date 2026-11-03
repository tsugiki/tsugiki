import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, expect, it, vi } from "vitest";
import Palette from "./Palette.svelte";

afterEach(() => {
  vi.restoreAllMocks();
});

it("takes the focus when it opens", () => {
  render(Palette, { onclose: () => {} });
  expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Command" }));
});

it("is a dialog with a name", () => {
  render(Palette, { onclose: () => {} });
  expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
});

it("says it has no commands yet", () => {
  render(Palette, { onclose: () => {} });
  expect(screen.getByText("No commands yet")).toBeTruthy();
});

it("closes on Esc", async () => {
  const onclose = vi.fn();
  render(Palette, { onclose });
  await fireEvent.keyDown(window, { key: "Escape" });
  expect(onclose).toHaveBeenCalledOnce();
});

it("stays open on other keys", async () => {
  const onclose = vi.fn();
  render(Palette, { onclose });
  await fireEvent.keyDown(window, { key: "a" });
  expect(onclose).not.toHaveBeenCalled();
});

it("closes on a press outside it", async () => {
  const onclose = vi.fn();
  render(Palette, { onclose });
  await fireEvent.mouseDown(screen.getByTestId("palette-scrim"));
  expect(onclose).toHaveBeenCalledOnce();
});

it("stays open on a press inside it, even if the mouse lets go outside", async () => {
  const onclose = vi.fn();
  render(Palette, { onclose });
  await fireEvent.mouseDown(screen.getByRole("dialog"));
  await fireEvent.mouseUp(screen.getByTestId("palette-scrim"));
  await fireEvent.click(screen.getByTestId("palette-scrim"));
  expect(onclose).not.toHaveBeenCalled();
});

it("gives the focus back when it closes", () => {
  const button = document.createElement("button");
  document.body.append(button);
  button.focus();
  const { unmount } = render(Palette, { onclose: () => {} });
  expect(document.activeElement).not.toBe(button);
  unmount();
  expect(document.activeElement).toBe(button);
  button.remove();
});

it("closes quietly when nothing had the focus to give back", () => {
  vi.spyOn(document, "activeElement", "get").mockReturnValue(null);
  const { unmount } = render(Palette, { onclose: () => {} });
  expect(() => {
    unmount();
  }).not.toThrow();
});

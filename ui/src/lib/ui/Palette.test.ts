import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, expect, it, vi } from "vitest";
import Palette from "./Palette.svelte";

afterEach(() => {
  vi.restoreAllMocks();
});

it("takes the focus when it opens", () => {
  render(Palette, { commands: [], onclose: () => {} });
  expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Command" }));
});

it("is a dialog with a name", () => {
  render(Palette, { commands: [], onclose: () => {} });
  expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
});

/** The palette with two commands, and what running each did. */
function withCommands() {
  const ran: string[] = [];
  const onclose = vi.fn();
  const command = (title: string) => ({
    title,
    run: () => {
      ran.push(title);
    },
  });
  render(Palette, { commands: [command("Show performance HUD"), command("Refresh now")], onclose });
  const input = screen.getByRole("combobox", { name: "Command" });
  return { ran, onclose, input };
}

it("lists its commands, with the first one picked", () => {
  withCommands();
  const options = screen.getAllByRole("option");
  expect(options.map((option) => option.textContent)).toEqual(["Show performance HUD", "Refresh now"]);
  expect(options.map((option) => option.getAttribute("aria-selected"))).toEqual(["true", "false"]);
});

it("lists only the commands that hold what was typed, in any case", async () => {
  const { input } = withCommands();
  await fireEvent.input(input, { target: { value: "  hud" } });
  expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["Show performance HUD"]);
});

it("says when no command holds what was typed", async () => {
  const { input } = withCommands();
  await fireEvent.input(input, { target: { value: "print" } });
  expect(screen.queryAllByRole("option")).toEqual([]);
  expect(screen.getByText("No command matches")).toBeTruthy();
  expect(input.getAttribute("aria-expanded")).toBe("false");
});

it("closes and runs the picked command on Enter", async () => {
  const { ran, onclose, input } = withCommands();
  await fireEvent.keyDown(input, { key: "Enter" });
  expect(onclose).toHaveBeenCalledOnce();
  expect(ran).toEqual(["Show performance HUD"]);
});

it("moves the pick with the arrows, and stops at each end", async () => {
  const { ran, input } = withCommands();
  await fireEvent.keyDown(input, { key: "ArrowUp" });
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(input.getAttribute("aria-activedescendant")).toBe(screen.getByRole("option", { selected: true }).id);
  await fireEvent.keyDown(input, { key: "Enter" });
  expect(ran).toEqual(["Refresh now"]);
});

it("picks the first command again after typing", async () => {
  const { ran, input } = withCommands();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.input(input, { target: { value: "" } });
  await fireEvent.keyDown(input, { key: "Enter" });
  expect(ran).toEqual(["Show performance HUD"]);
});

it("runs nothing on Enter when no command matches", async () => {
  const { ran, onclose, input } = withCommands();
  await fireEvent.input(input, { target: { value: "print" } });
  await fireEvent.keyDown(input, { key: "Enter" });
  expect(ran).toEqual([]);
  expect(onclose).not.toHaveBeenCalled();
});

it("runs a command that is clicked", async () => {
  const { ran, onclose } = withCommands();
  await fireEvent.click(screen.getByRole("option", { name: "Refresh now" }));
  expect(onclose).toHaveBeenCalledOnce();
  expect(ran).toEqual(["Refresh now"]);
});

it("closes on Esc", async () => {
  const onclose = vi.fn();
  render(Palette, { commands: [], onclose });
  await fireEvent.keyDown(window, { key: "Escape" });
  expect(onclose).toHaveBeenCalledOnce();
});

it("stays open on other keys", async () => {
  const onclose = vi.fn();
  render(Palette, { commands: [], onclose });
  await fireEvent.keyDown(window, { key: "a" });
  expect(onclose).not.toHaveBeenCalled();
});

it("closes on a press outside it", async () => {
  const onclose = vi.fn();
  render(Palette, { commands: [], onclose });
  await fireEvent.mouseDown(screen.getByTestId("palette-scrim"));
  expect(onclose).toHaveBeenCalledOnce();
});

it("stays open on a press inside it, even if the mouse lets go outside", async () => {
  const onclose = vi.fn();
  render(Palette, { commands: [], onclose });
  await fireEvent.mouseDown(screen.getByRole("dialog"));
  await fireEvent.mouseUp(screen.getByTestId("palette-scrim"));
  await fireEvent.click(screen.getByTestId("palette-scrim"));
  expect(onclose).not.toHaveBeenCalled();
});

it("gives the focus back when it closes", () => {
  const button = document.createElement("button");
  document.body.append(button);
  button.focus();
  const { unmount } = render(Palette, { commands: [], onclose: () => {} });
  expect(document.activeElement).not.toBe(button);
  unmount();
  expect(document.activeElement).toBe(button);
  button.remove();
});

it("closes quietly when nothing had the focus to give back", () => {
  vi.spyOn(document, "activeElement", "get").mockReturnValue(null);
  const { unmount } = render(Palette, { commands: [], onclose: () => {} });
  expect(() => {
    unmount();
  }).not.toThrow();
});

it("leaves other keys to the text box", async () => {
  const { ran, onclose, input } = withCommands();
  await fireEvent.keyDown(input, { key: "a" });
  expect(ran).toEqual([]);
  expect(onclose).not.toHaveBeenCalled();
});

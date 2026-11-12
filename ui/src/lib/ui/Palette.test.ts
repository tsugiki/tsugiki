import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
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

it("keeps the browser from moving the focus on a press outside it, as the focus has gone back", async () => {
  render(Palette, { commands: [], onclose: () => {} });
  // A browser focuses the body on a press it isn't stopped from, after the palette gave the focus back.
  expect(await fireEvent.mouseDown(screen.getByTestId("palette-scrim"))).toBe(false);
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

it("runs the command once it has gone and given the focus back", async () => {
  const button = document.createElement("button");
  document.body.append(button);
  button.focus();
  let focusedOnRun: Element | null = null;
  const run = () => {
    focusedOnRun = document.activeElement;
  };
  // The page closes the palette on its next update, as `onclose` only changes the page's state.
  const { unmount } = render(Palette, {
    commands: [{ title: "Hide Sidebar", run }],
    onclose: () => {
      queueMicrotask(unmount);
    },
  });

  await fireEvent.click(screen.getByRole("option", { name: "Hide Sidebar" }));

  expect(focusedOnRun).toBe(button);
  button.remove();
});

it("closes quietly when nothing had the focus to give back", () => {
  vi.spyOn(document, "activeElement", "get").mockReturnValue(null);
  const { unmount } = render(Palette, { commands: [], onclose: () => {} });
  expect(() => {
    unmount();
  }).not.toThrow();
});

describe("as another quick pick", () => {
  const words = {
    dialog: "Repository picker",
    input: "Repository",
    placeholder: "Type a Repository's name",
    list: "Repositories",
    none: "No Repository matches",
  };
  /** A row's text as it reads: the mark stays on the same line as the name. */
  const spaced = (row: HTMLElement | undefined) => row?.textContent.replace(/\s/g, " ");
  const rows = [
    { title: "a/open", run: () => {}, current: true },
    { title: "a/previous", run: () => {} },
  ];

  it("names its dialog, its box, its list and an empty list with the words it is given", async () => {
    render(Palette, { commands: rows, onclose: () => {}, words });
    expect(screen.getByRole("dialog", { name: "Repository picker" })).toBeTruthy();
    expect(screen.getByRole("listbox", { name: "Repositories" })).toBeTruthy();
    const input = screen.getByRole("combobox", { name: "Repository" });
    expect(input.getAttribute("placeholder")).toBe("Type a Repository's name");
    await fireEvent.input(input, { target: { value: "nothing" } });
    expect(screen.getByText("No Repository matches")).toBeTruthy();
  });

  it("opens with the row it is told picked, and picks the first again after typing", async () => {
    render(Palette, { commands: rows, onclose: () => {}, words, start: 1 });
    expect(screen.getByRole("option", { selected: true }).textContent).toBe("a/previous");
    await fireEvent.input(screen.getByRole("combobox"), { target: { value: "a/" } });
    expect(spaced(screen.getByRole("option", { selected: true }))).toBe("a/open open");
  });

  it("marks the current row", () => {
    render(Palette, { commands: rows, onclose: () => {}, words });
    const [open, previous] = screen.getAllByRole("option");
    expect(open?.getAttribute("aria-current")).toBe("true");
    expect(spaced(open)).toBe("a/open open");
    expect(previous?.getAttribute("aria-current")).toBeNull();
  });

  it("marks the current row with the word it is given", () => {
    render(Palette, { commands: rows, onclose: () => {}, words: { ...words, mark: "current" } });
    expect(spaced(screen.getAllByRole("option")[0])).toBe("a/open current");
  });
});

describe("as a picker that previews", () => {
  /** Rows that note each preview, and a last row that stays open. */
  function withPreviews(start = 0) {
    const previewed: string[] = [];
    const ran: string[] = [];
    const row = (title: string) => ({
      title,
      preview: () => {
        previewed.push(title);
      },
      run: () => {
        ran.push(title);
      },
    });
    const onclose = vi.fn();
    const oncancel = vi.fn();
    const stays = { title: "Dark themes…", keepsOpen: true, run: () => ran.push("Dark themes…") };
    render(Palette, { commands: [row("Light"), row("Gruvbox"), stays], onclose, oncancel, start });
    return { previewed, ran, onclose, oncancel, input: screen.getByRole("combobox") };
  }

  it("previews the row it opens on, and each row the pick moves to", async () => {
    const { previewed, input } = withPreviews(1);
    await fireEvent.keyDown(input, { key: "ArrowUp" });
    await fireEvent.keyDown(input, { key: "ArrowUp" });
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(previewed).toEqual(["Gruvbox", "Light", "Gruvbox"]);
  });

  it("previews the first row that matches what was typed", async () => {
    const { previewed, input } = withPreviews(0);
    await fireEvent.input(input, { target: { value: "gruv" } });
    expect(previewed).toEqual(["Light", "Gruvbox"]);
  });

  it("runs a row that keeps it open without closing", async () => {
    const { ran, onclose, oncancel, input } = withPreviews(2);
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(ran).toEqual(["Dark themes…"]);
    expect(onclose).not.toHaveBeenCalled();
    expect(oncancel).not.toHaveBeenCalled();
  });

  it("cancels on Esc or a press outside, but not on a pick", async () => {
    const { onclose, oncancel, input } = withPreviews();
    await fireEvent.keyDown(window, { key: "Escape" });
    await fireEvent.mouseDown(screen.getByTestId("palette-scrim"));
    expect(oncancel).toHaveBeenCalledTimes(2);
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(oncancel).toHaveBeenCalledTimes(2);
    expect(onclose).toHaveBeenCalledTimes(3);
  });
});

it("leaves other keys to the text box", async () => {
  const { ran, onclose, input } = withCommands();
  await fireEvent.keyDown(input, { key: "a" });
  expect(ran).toEqual([]);
  expect(onclose).not.toHaveBeenCalled();
});

import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import HistoryBar from "./HistoryBar.svelte";

const shown = (back: string[], forward: string[]) => {
  const ongo = vi.fn();
  render(HistoryBar, { back, forward, current: "#3 Here", ongo });
  return { ongo, bar: within(screen.getByRole("navigation", { name: "History" })) };
};

it("greys out an arrow with nothing that way, and says so", () => {
  const { bar } = shown([], ["#4 Ahead"]);

  const back = bar.getByRole("button", { name: "Back" });
  expect(back.hasAttribute("disabled")).toBe(true);
  expect(back.getAttribute("title")).toBe("Back (Alt+←): nothing there");
  expect(bar.getByRole("button", { name: "Forward" }).getAttribute("title")).toBe(
    "Forward to #4 Ahead (Alt+→)",
  );
  expect(bar.getByText("#3 Here")).toBeTruthy();
});

it("goes one step on a click, and as many as the item picked from the list", async () => {
  const { ongo, bar } = shown(["#2 Before", "#1 First"], ["#4 Ahead", "#5 Last"]);

  await fireEvent.click(bar.getByRole("button", { name: "Back" }));
  expect(ongo).toHaveBeenLastCalledWith(-1);

  await fireEvent.contextMenu(bar.getByRole("button", { name: "Forward" }));
  const menu = within(screen.getByRole("menu", { name: "Recent items forward" }));
  expect(document.activeElement?.textContent).toBe("#4 Ahead");
  await fireEvent.click(menu.getByRole("menuitem", { name: "#5 Last" }));

  expect(ongo).toHaveBeenLastCalledWith(2);
  expect(screen.queryByRole("menu")).toBeNull();
});

it("closes the list on Esc, a second right-click or a click elsewhere", async () => {
  const { ongo, bar } = shown(["#2 Before"], []);
  const back = bar.getByRole("button", { name: "Back" });
  const opened = async () => {
    await fireEvent.contextMenu(back);
    return screen.getByRole("menu", { name: "Recent items back" });
  };

  await fireEvent.keyDown(await opened(), { key: "ArrowDown" });
  expect(screen.queryByRole("menu")).toBeTruthy();
  await fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();

  await opened();
  await fireEvent.contextMenu(back);
  expect(screen.queryByRole("menu")).toBeNull();

  await opened();
  await fireEvent.click(document.body);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(ongo).not.toHaveBeenCalled();
});

it("leaves the title blank until the item on show is known", () => {
  render(HistoryBar, { back: [], forward: [], current: undefined, ongo: vi.fn() });

  expect(screen.getByRole("navigation", { name: "History" }).textContent?.replace(/[‹›\s]/g, "")).toBe("");
});

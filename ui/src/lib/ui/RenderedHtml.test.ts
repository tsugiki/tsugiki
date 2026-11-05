import { createEvent, fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import RenderedHtml from "./RenderedHtml.svelte";

const shown = (html: string) => {
  const onlink = vi.fn();
  render(RenderedHtml, { html, onlink });
  return onlink;
};

/** Clicks `element`, and says whether the browser may still act on the click. */
const clickFollows = (element: Element, kind: "click" | "auxclick" = "click", button = 0) => {
  const click = new MouseEvent(kind, { bubbles: true, cancelable: true, button });
  void fireEvent(element, click);
  return !click.defaultPrevented;
};

it("shows GitHub's rendering", () => {
  shown('<p>It <em>crashes</em></p><pre><code class="language-python">x = 1</code></pre>');

  expect(screen.getByText("crashes").tagName).toBe("EM");
  expect(screen.getByText("x = 1").tagName).toBe("CODE");
});

it("sends a clicked link to the browser and keeps the window where it is", () => {
  const onlink = shown('<p><a href="https://github.com/python/cpython/issues/1">gh-1</a></p>');

  expect(clickFollows(screen.getByText("gh-1"))).toBe(false);

  expect(onlink).toHaveBeenCalledExactlyOnceWith("https://github.com/python/cpython/issues/1");
});

it("does the same for a middle click, and for a click on what a link holds", () => {
  const onlink = shown('<a href="https://github.com/"><em>inside</em></a>');

  expect(clickFollows(screen.getByText("inside"), "auxclick", 1)).toBe(false);

  expect(onlink).toHaveBeenCalledExactlyOnceWith("https://github.com/");
});

it("leaves a right click and the side buttons alone", () => {
  const onlink = shown('<a href="https://github.com/">Home</a>');

  for (const button of [2, 3, 4]) {
    expect(clickFollows(screen.getByText("Home"), "auxclick", button)).toBe(true);
  }

  expect(onlink).not.toHaveBeenCalled();
});

it("keeps a link within the text and one with nowhere to go from leaving", () => {
  const onlink = shown('<a href="#a-heading">Near</a><a>Nowhere</a>');

  expect(clickFollows(screen.getByText("Near"))).toBe(false);
  expect(clickFollows(screen.getByText("Nowhere"))).toBe(false);

  expect(onlink).not.toHaveBeenCalled();
});

it("leaves other clicks to the browser, so a folded section still unfolds", () => {
  const onlink = shown("<details><summary>More</summary>Inside</details>");

  expect(clickFollows(screen.getByText("More"))).toBe(true);

  expect(onlink).not.toHaveBeenCalled();
});

it("keeps a form from submitting the window away", () => {
  shown('<form action="https://github.com/" data-testid="form"></form>');
  const form = screen.getByTestId("form");
  const submit = createEvent.submit(form);

  void fireEvent(form, submit);

  expect(submit.defaultPrevented).toBe(true);
});
